#!/usr/bin/env bash
#
# Bring up xmu_secure_link (VPN tun) + microsocks (SOCKS5) inside ONE network
# namespace. We do NOT force the container default route through the VPN; we rely
# on the split-tunnel routes xmu_secure_link installs itself (campus/ACL targets
# via the tun, everything else direct). microsocks just follows that route table.
#
# Data path (campus target): App -> 127.0.0.1:1080 -> microsocks
#            -> VPN tun (client route) -> xmu_secure_link (OpenVPN3) -> server
# Data path (other target):  App -> 127.0.0.1:1080 -> microsocks -> direct (eth0)
#
set -euo pipefail

SOCKS_PORT="${SOCKS_PORT:-1080}"
BIN_DIR="/opt/xmu_secure_link"
DATA_DIR="/root/.local/share/mysecurelinkrs"
LOG_FILE="/tmp/xmu_secure_link.log"

log() { echo "[entrypoint] $*"; }

# --- 0. one-time interactive login -------------------------------------------
# The saved session (refresh_token) eventually expires; when it does, the client
# needs a fresh SSO login. Run this once to refresh the session INTO the mounted
# volume, then normal `up` reconnects non-interactively:
#
#   docker compose -f docker/docker-compose.yml run --rm -it xmu-securelink-socks login
#
# It prints an SSO URL: open it, finish login, paste the callback URL back here.
# Once you see it start connecting / "VPN connected", press Ctrl-C. The refreshed
# session is already written to /root/.local/share/mysecurelinkrs (the mount).
if [ "${1:-}" = "login" ]; then
  log "interactive login mode"
  log "  -> open the SSO URL below, finish login, paste the callback URL here"
  log "  -> once it starts connecting, press Ctrl-C; the session is saved to the volume"
  cd "$BIN_DIR"
  exec ./xmu_secure_link
fi

# --- 1. sanity: the tun char device must be passed in ------------------------
if [ ! -c /dev/net/tun ]; then
  log "ERROR: /dev/net/tun not found."
  log "       Run with --cap-add NET_ADMIN --device /dev/net/tun (compose already does)."
  exit 1
fi

# --- 2. verify the mounted SecureLink data is visible ------------------------
log "checking SecureLink data dir: $DATA_DIR"
if [ ! -f "$DATA_DIR/session.json" ] || [ ! -f "$DATA_DIR/device_id" ]; then
  log "ERROR: session data not found at $DATA_DIR"
  log "       Expected files: session.json and device_id."
  log "       Set SECURELINK_DATA_DIR to the host directory that contains those files."
  log "       Example: C:/Users/vintces/Desktop/rust/xmu_assistant_sign_bot/data/securelink"
  ls -la "$DATA_DIR" 2>/dev/null || true
  exit 1
fi
log "session data present: session.json + device_id"

# --- 3. remember the ORIGINAL (eth0) default route ---------------------------
# Captured before the VPN starts so we can keep the OpenVPN server reachable
# even after we flip the default route to the tunnel.
ORIG_DEFAULT_GW="$(ip route show default | awk '/default/ {print $3; exit}')"
ORIG_DEFAULT_DEV="$(ip route show default | awk '/default/ {print $5; exit}')"
log "original default route: gw=${ORIG_DEFAULT_GW:-<none>} dev=${ORIG_DEFAULT_DEV:-<none>}"
log "original route table:"
ip route || true

# snapshot interfaces so the newly created VPN one is easy to spot
BEFORE_IFACES="$(ip -o link show | awk -F': ' '{print $2}' | sed 's/@.*//' | sort -u)"

# --- 4. start the VPN client (auto-connects from the mounted session) --------
log "starting xmu_secure_link"
cd "$BIN_DIR"
: > "$LOG_FILE"
./xmu_secure_link >>"$LOG_FILE" 2>&1 &
VPN_PID=$!

SOCKS_PID=""
cleanup() {
  log "cleanup"
  [ -n "$SOCKS_PID" ] && kill "$SOCKS_PID" 2>/dev/null || true
  kill "$VPN_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# --- 5. wait for the VPN tun/tap interface to appear -------------------------
log "waiting for VPN interface (up to 120s)"
VPN_DEV=""
for _ in $(seq 1 120); do
  if ! kill -0 "$VPN_PID" 2>/dev/null; then
    log "ERROR: xmu_secure_link exited early. Last log lines:"
    tail -n 40 "$LOG_FILE" || true
    exit 1
  fi

  AFTER_IFACES="$(ip -o link show | awk -F': ' '{print $2}' | sed 's/@.*//' | sort -u)"
  NEW_IFACES="$(comm -13 <(printf '%s\n' "$BEFORE_IFACES") <(printf '%s\n' "$AFTER_IFACES") || true)"

  for dev in $NEW_IFACES; do
    if echo "$dev" | grep -Eqi '^(tun|tap|ovpn)' \
       || ip link show "$dev" 2>/dev/null | grep -Eqi 'POINTOPOINT|NOARP'; then
      VPN_DEV="$dev"; break
    fi
  done
  # fallback: any tun/tap/ovpn interface at all
  [ -z "$VPN_DEV" ] && VPN_DEV="$(ip -o link show | awk -F': ' '{print $2}' \
      | sed 's/@.*//' | grep -E '^(tun|tap|ovpn)' | head -n1 || true)"

  if [ -n "$VPN_DEV" ]; then
    log "VPN interface detected: $VPN_DEV"
    break
  fi
  sleep 1
done

if [ -z "$VPN_DEV" ]; then
  log "ERROR: VPN interface never appeared. Diagnostics:"
  ip addr || true
  tail -n 60 "$LOG_FILE" || true
  exit 1
fi

# let the client finish pushing its own routes / bringing the link up
sleep 2

# --- 6. rely on xmu_secure_link's own (split-tunnel) routes ------------------
# We deliberately DO NOT pin the server IP and DO NOT flip the container default
# route to the VPN. The client already installs its own /32 routes for the
# campus intranet/ACL targets via the tun; everything else keeps the original
# uplink (eth0). microsocks then simply follows this table:
#   campus/ACL targets -> VPN tun (hidden campus exit IP)
#   everything else     -> direct via eth0
# This avoids the fragile "pin server IP + flip default" dance and its
# route-eats-its-own-tail failure mode. The bot only sends *.xmu.edu.cn to the
# SOCKS5 anyway, and those campus IPs are exactly what the client routes through
# the tunnel — so no forced default route is needed.
log "using xmu_secure_link's own split-tunnel routes; container default route left untouched"

log "final route table:"
ip route || true
log "probe target routes:"
ip route get 121.192.180.236 2>/dev/null || true
ip route get 59.77.5.59 2>/dev/null || true

# --- 8. expose SOCKS5 --------------------------------------------------------
# microsocks does a plain connect() from inside this namespace, so it follows
# the route table set up above. It knows nothing about OpenVPN.
log "starting microsocks on 0.0.0.0:${SOCKS_PORT}"
microsocks -i 0.0.0.0 -p "$SOCKS_PORT" &
SOCKS_PID=$!

# exit as soon as either process dies so `restart: unless-stopped` recovers us
wait -n "$VPN_PID" "$SOCKS_PID"
log "a child process exited; shutting down container"
exit 1
