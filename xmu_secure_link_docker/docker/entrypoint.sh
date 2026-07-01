#!/usr/bin/env bash
#
# Bring up xmu_secure_link (VPN tun) + microsocks (SOCKS5) inside ONE network
# namespace, then wire the container route table so SOCKS5 connect()s exit
# through the VPN interface.
#
# Data path: App -> host 127.0.0.1:1080 -> microsocks -> container route table
#            -> VPN tun -> xmu_secure_link (OpenVPN3) -> OpenVPN server
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

# --- 6. pin the OpenVPN server IP(s) to the ORIGINAL uplink -------------------
# So that flipping the default route to the tunnel below does not cut the
# tunnel's own transport (the classic "route eats its own tail" problem).
collect_server_ips() {
  # (a) live established sockets owned by the client -> the peer is the server
  ss -Hnp -t -u state established 2>/dev/null \
    | grep -E "pid=${VPN_PID},|xmu_secure_link" \
    | awk '{print $5}' | awk -F: '{print $1}' || true
  # (b) any /32 host route the client itself installed via the physical dev
  if [ -n "${ORIG_DEFAULT_DEV:-}" ]; then
    ip route show dev "$ORIG_DEFAULT_DEV" 2>/dev/null \
      | grep -oE '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+/32' | awk -F/ '{print $1}' || true
  fi
  # (c) best-effort log parse as a last resort
  grep -iE 'remote' "$LOG_FILE" 2>/dev/null \
    | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' || true
}

# drop noise: any-address, loopback, link-local, and RFC1918 (the container's
# own eth0 address); the campus OpenVPN endpoint is a routable public IP.
SERVER_IPS="$(collect_server_ips \
  | grep -vE '^(0\.|127\.|169\.254\.|10\.|172\.(1[6-9]|2[0-9]|3[01])\.|192\.168\.)' \
  | sort -u || true)"

PINNED=0
if [ -n "$SERVER_IPS" ] && [ -n "${ORIG_DEFAULT_GW:-}" ] && [ -n "${ORIG_DEFAULT_DEV:-}" ]; then
  while read -r ip; do
    [ -z "$ip" ] && continue
    log "pin server route: ${ip}/32 via ${ORIG_DEFAULT_GW} dev ${ORIG_DEFAULT_DEV}"
    ip route replace "${ip}/32" via "$ORIG_DEFAULT_GW" dev "$ORIG_DEFAULT_DEV" && PINNED=1 || true
  done <<< "$SERVER_IPS"
fi

# --- 7. send everything else through the VPN ---------------------------------
# Only if we managed to pin the server; otherwise leave the client's own routes
# alone rather than risk killing the tunnel uplink.
if [ "$PINNED" -eq 1 ]; then
  log "routing default via VPN interface $VPN_DEV"
  ip route replace default dev "$VPN_DEV" || true
else
  log "WARN: could not identify the OpenVPN server IP."
  log "WARN: leaving the default route untouched and relying on the routes"
  log "WARN: xmu_secure_link installed itself (intranet targets should still work)."
fi

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
