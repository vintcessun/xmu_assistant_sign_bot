#!/usr/bin/env bash
#
# Bring up xmu_secure_link (VPN tun) + microsocks (SOCKS5) inside ONE network
# namespace. We do NOT force the container default route through the VPN; instead
# we pin the key XMU targets (lnt/jw + the intranet probes) to the tun with /32
# routes — everything else stays direct. microsocks just follows that route table.
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

# --- 3b. resolve the XMU domains BEFORE the VPN starts -----------------------
# Resolve via the normal (pre-VPN) uplink to capture the real public IPs, then
# pin them to the tun once it is up (step 6) so their traffic goes through the VPN.
# Resolving BEFORE the VPN matters: DNS still exits via eth0 here, giving the real
# addresses; after the tunnel routes are in place resolution could differ/hang.
XMU_ROUTE_DOMAINS="ids.xmu.edu.cn lnt.xmu.edu.cn jw.xmu.edu.cn"
XMU_ROUTE_STATIC_IPS="121.192.180.236 59.77.5.59"   # extra intranet targets (no DNS)
XMU_ROUTE_IPS=""

resolve_xmu_domains() {
  local ips="" d r
  for d in $XMU_ROUTE_DOMAINS; do
    # getent uses the container resolver; dig is the fallback (dnsutils installed).
    r="$(getent ahostsv4 "$d" 2>/dev/null | awk '{print $1}' | sort -u)"
    [ -z "$r" ] && r="$(dig +short A "$d" 2>/dev/null | grep -E '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$' || true)"
    if [ -n "$r" ]; then
      log "resolved $d -> $(echo $r | tr '\n' ' ')"
      ips="$ips $r"
    else
      log "WARN: could not resolve $d (skipping)"
    fi
  done
  XMU_ROUTE_IPS="$(printf '%s\n' $ips $XMU_ROUTE_STATIC_IPS \
    | grep -E '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$' | sort -u | tr '\n' ' ')"
  log "XMU pin IPs: ${XMU_ROUTE_IPS:-<none>}"
}

log "resolving XMU domains before starting VPN"
resolve_xmu_domains

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

# --- 6. pin the resolved XMU targets to the VPN tun -------------------------
# The client's own split-tunnel routes did NOT cover lnt/jw, so we explicitly add
# /32 routes (via the tun) for the XMU IPs resolved at startup in step 3b. We
# still do NOT flip the container default route — only these targets go through
# the VPN, everything else stays direct. (Edit XMU_ROUTE_DOMAINS to add hosts.)
add_xmu_routes() {
  local dev="$VPN_DEV"
  local tun_ip
  tun_ip="$(ip -4 addr show "$dev" 2>/dev/null | awk '/inet / {print $2}' | cut -d/ -f1 | head -n1)"
  if [ -n "$tun_ip" ]; then
    log "tun ($dev) IPv4: $tun_ip"
  else
    log "WARN: $dev has no IPv4 yet; adding routes without src"
  fi

  log "adding XMU /32 routes via $dev"
  for ip in $XMU_ROUTE_IPS; do
    if [ -n "$tun_ip" ]; then
      ip route replace "${ip}/32" dev "$dev" src "$tun_ip" || log "WARN: failed to add ${ip}/32"
    else
      ip route replace "${ip}/32" dev "$dev" || log "WARN: failed to add ${ip}/32"
    fi
  done

  log "XMU route check:"
  for ip in $XMU_ROUTE_IPS; do
    ip route get "$ip" 2>/dev/null || true
  done
}
add_xmu_routes

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
