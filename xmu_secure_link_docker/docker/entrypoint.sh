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
# --- Session isolation + bidirectional sync ----------------------------------
# The mounted volume (SRC_DIR) is the shared source of truth with the host.
# The client does NOT read/write it directly; instead it runs against an ISOLATED
# working copy (WORK_DIR) via HOME/XDG_DATA_HOME. This lets us tell apart WHO
# changed the session files by WHERE the change landed:
#
#   * WORK_DIR changed  -> the client refreshed its own token internally.
#                          We copy WORK -> SRC (reflect outward). NO restart.
#   * SRC_DIR changed   -> something EXTERNAL (host / another login) edited the
#                          files. We copy SRC -> WORK (reflect inward) and
#                          RESTART the client so it reloads them. Logs on reload.
#
# So: internal refreshes propagate to the host, external edits propagate into the
# container and trigger a reload — without a restart storm from the client's own
# routine token refresh.
#
set -uo pipefail   # NOT -e: the watch loop must survive transient failures

SOCKS_PORT="${SOCKS_PORT:-1080}"
BIN_DIR="/opt/xmu_secure_link"
SRC_DIR="/root/.local/share/mysecurelinkrs"          # the mounted volume (shared)
LOG_FILE="/tmp/xmu_secure_link.log"
WATCH_INTERVAL="${WATCH_INTERVAL:-5}"                 # seconds between checks
WATCH_DEBOUNCE="${WATCH_DEBOUNCE:-2}"                 # settle time for external writes

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
#
# NOTE: login mode intentionally does NOT isolate — it writes straight to the
# mount (default HOME=/root) so the refreshed session persists to the host.
if [ "${1:-}" = "login" ]; then
  log "interactive login mode (writes straight to the mounted volume)"
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
log "checking SecureLink data dir (mount): $SRC_DIR"
if [ ! -f "$SRC_DIR/session.json" ] || [ ! -f "$SRC_DIR/device_id" ]; then
  log "ERROR: session data not found at $SRC_DIR"
  log "       Expected files: session.json and device_id."
  log "       Set SECURELINK_DATA_DIR to the host directory that contains those files."
  log "       Example: C:/Users/vintces/Desktop/rust/xmu_assistant_sign_bot/data/securelink"
  ls -la "$SRC_DIR" 2>/dev/null || true
  exit 1
fi
log "session data present: session.json + device_id"

# --- 2b. set up the ISOLATED working copy ------------------------------------
# The client resolves its data dir via directories-rs, which on Linux honours
# XDG_DATA_HOME (falling back to $HOME/.local/share). Point both at WORK_HOME so
# the client reads/writes WORK_DIR instead of the mount.
NAME="$(basename "$SRC_DIR")"                         # mysecurelinkrs
WORK_HOME="/run/securelink-home"
export HOME="$WORK_HOME"
export XDG_DATA_HOME="$WORK_HOME/.local/share"
WORK_DIR="$XDG_DATA_HOME/$NAME"
mkdir -p "$WORK_DIR"
log "isolated working copy: $WORK_DIR (HOME=$WORK_HOME)"

# fingerprint = sha256 of the two session files in a dir (order-stable).
# Missing files just contribute nothing; the digest still changes when they appear.
fingerprint() {
  local d="$1"
  sha256sum "$d/session.json" "$d/device_id" 2>/dev/null | awk '{print $1}' | tr -d '\n'
}

# copy the two session files from one dir to another (best-effort, atomic-ish).
sync_files() {
  local from="$1" to="$2" f
  mkdir -p "$to"
  for f in session.json device_id; do
    [ -f "$from/$f" ] || continue
    cp -f "$from/$f" "$to/$f.tmp" 2>/dev/null && mv -f "$to/$f.tmp" "$to/$f" 2>/dev/null || true
  done
}

# seed the working copy from the mount so the client starts from the host state.
sync_files "$SRC_DIR" "$WORK_DIR"
LAST_SRC="$(fingerprint "$SRC_DIR")"
LAST_WORK="$(fingerprint "$WORK_DIR")"

# --- 3. remember the ORIGINAL (eth0) default route ---------------------------
ORIG_DEFAULT_GW="$(ip route show default | awk '/default/ {print $3; exit}')"
ORIG_DEFAULT_DEV="$(ip route show default | awk '/default/ {print $5; exit}')"
log "original default route: gw=${ORIG_DEFAULT_GW:-<none>} dev=${ORIG_DEFAULT_DEV:-<none>}"

# --- 3b. resolve the XMU domains BEFORE the VPN starts -----------------------
# Resolve via the normal (pre-VPN) uplink to capture the real public IPs, then
# pin them to the tun once it is up so their traffic goes through the VPN.
XMU_ROUTE_DOMAINS=""
XMU_ROUTE_STATIC_IPS="121.192.180.236 59.77.5.59 219.229.81.200"
XMU_ROUTE_IPS=""

resolve_xmu_domains() {
  local ips="" d r
  for d in $XMU_ROUTE_DOMAINS; do
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

# --- 4. VPN lifecycle: start / stop ------------------------------------------
VPN_PID=""
VPN_DEV=""

add_xmu_routes() {
  local dev="$VPN_DEV" tun_ip ip
  tun_ip="$(ip -4 addr show "$dev" 2>/dev/null | awk '/inet / {print $2}' | cut -d/ -f1 | head -n1)"
  [ -n "$tun_ip" ] && log "tun ($dev) IPv4: $tun_ip" || log "WARN: $dev has no IPv4 yet; adding routes without src"

  log "adding XMU /32 routes via $dev"
  for ip in $XMU_ROUTE_IPS; do
    if [ -n "$tun_ip" ]; then
      ip route replace "${ip}/32" dev "$dev" src "$tun_ip" || log "WARN: failed to add ${ip}/32"
    else
      ip route replace "${ip}/32" dev "$dev" || log "WARN: failed to add ${ip}/32"
    fi
  done

  log "probe target routes:"
  ip route get 121.192.180.236 2>/dev/null || true
  ip route get 59.77.5.59 2>/dev/null || true
}

# start_vpn: launch the client and wait for its tun to come up + pin routes.
# Returns 0 on success, 1 on failure (caller keeps watching, does not exit).
start_vpn() {
  local before after new dev i
  before="$(ip -o link show | awk -F': ' '{print $2}' | sed 's/@.*//' | sort -u)"

  log "starting xmu_secure_link"
  cd "$BIN_DIR"
  : > "$LOG_FILE"
  ./xmu_secure_link >>"$LOG_FILE" 2>&1 &
  VPN_PID=$!
  VPN_DEV=""

  for i in $(seq 1 120); do
    if ! kill -0 "$VPN_PID" 2>/dev/null; then
      log "ERROR: xmu_secure_link exited early. Last log lines:"
      tail -n 40 "$LOG_FILE" || true
      VPN_PID=""
      return 1
    fi
    after="$(ip -o link show | awk -F': ' '{print $2}' | sed 's/@.*//' | sort -u)"
    new="$(comm -13 <(printf '%s\n' "$before") <(printf '%s\n' "$after") || true)"
    for dev in $new; do
      if echo "$dev" | grep -Eqi '^(tun|tap|ovpn)' \
         || ip link show "$dev" 2>/dev/null | grep -Eqi 'POINTOPOINT|NOARP'; then
        VPN_DEV="$dev"; break
      fi
    done
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
    tail -n 60 "$LOG_FILE" || true
    return 1
  fi

  sleep 2                     # let the client finish pushing its own routes
  add_xmu_routes
  return 0
}

# ensure_routes: the client reconnects INTERNALLY on transient errors (process
# stays alive, tun is destroyed + recreated, kernel purges our /32 routes with
# it). Re-detect the tun and re-pin the routes whenever they are missing.
ensure_routes() {
  [ -n "$VPN_PID" ] && kill -0 "$VPN_PID" 2>/dev/null || return 0

  local dev ip missing=0
  dev="$(ip -o link show | awk -F': ' '{print $2}' | sed 's/@.*//' \
      | grep -E '^(tun|tap|ovpn)' | head -n1 || true)"
  [ -n "$dev" ] || return 0     # tun currently down (mid-reconnect); retry next tick
  VPN_DEV="$dev"

  for ip in $XMU_ROUTE_IPS; do
    ip route get "$ip" 2>/dev/null | grep -q "dev $dev" || { missing=1; break; }
  done
  if [ "$missing" = 1 ]; then
    log "XMU routes missing (client reconnected) -> re-pinning via $dev"
    add_xmu_routes
  fi
}

# stop_vpn: terminate the client and wait for its tun to be torn down.
stop_vpn() {
  [ -n "$VPN_PID" ] || return 0
  log "stopping xmu_secure_link (pid $VPN_PID)"
  kill "$VPN_PID" 2>/dev/null || true
  local i
  for i in $(seq 1 15); do
    kill -0 "$VPN_PID" 2>/dev/null || break
    sleep 1
  done
  kill -9 "$VPN_PID" 2>/dev/null || true
  wait "$VPN_PID" 2>/dev/null || true
  VPN_PID=""
  sleep 1                     # give the kernel a moment to drop the tun
}

# --- 5. SOCKS5 (independent of VPN; survives client restarts) -----------------
SOCKS_PID=""
ensure_socks() {
  if [ -n "$SOCKS_PID" ] && kill -0 "$SOCKS_PID" 2>/dev/null; then
    return 0
  fi
  log "starting microsocks on 0.0.0.0:${SOCKS_PORT}"
  microsocks -i 0.0.0.0 -p "$SOCKS_PORT" &
  SOCKS_PID=$!
}

# Graceful shutdown matters here: if the client is SIGKILLed the VPN TCP
# connection is dropped without a proper disconnect, the server keeps a
# half-open session, and every later connection gets kicked (NETWORK_EOF_ERROR
# reconnect loop). So: give the client time to disconnect cleanly, and make the
# TERM handler actually EXIT (otherwise the watch loop below keeps running,
# resurrects the client, and docker escalates to SIGKILL after the timeout).
STOPPING=0
cleanup() {
  [ "$STOPPING" = 1 ] && return
  STOPPING=1
  log "cleanup: stopping SOCKS + VPN (graceful)"
  [ -n "$SOCKS_PID" ] && kill "$SOCKS_PID" 2>/dev/null || true
  if [ -n "$VPN_PID" ]; then
    kill "$VPN_PID" 2>/dev/null || true
    local i
    for i in $(seq 1 10); do
      kill -0 "$VPN_PID" 2>/dev/null || break
      sleep 1
    done
    kill -9 "$VPN_PID" 2>/dev/null || true
  fi
  log "cleanup done"
}
on_term() { log "received stop signal"; cleanup; trap - EXIT; exit 0; }
trap on_term INT TERM
trap cleanup EXIT

# rebaseline: after a (re)start the client may have refreshed its token while
# connecting (login() refreshes the access_token). That write went to WORK, so
# mirror it OUT to the host and reset both baselines — otherwise the startup
# refresh would be swallowed by the baseline capture and never reach the host.
rebaseline() {
  if [ "$(fingerprint "$WORK_DIR")" != "$(fingerprint "$SRC_DIR")" ]; then
    log "mirroring client session update to host mount"
    sync_files "$WORK_DIR" "$SRC_DIR"
  fi
  LAST_WORK="$(fingerprint "$WORK_DIR")"
  LAST_SRC="$(fingerprint "$SRC_DIR")"
}

# --- 6. initial bring-up -----------------------------------------------------
ensure_socks
if start_vpn; then
  rebaseline
else
  log "WARN: initial VPN bring-up failed; watching for an external session update to recover"
fi

# --- 7. watch loop: bidirectional session sync + reload-on-external-change ----
log "watching session files (interval ${WATCH_INTERVAL}s): external edit -> reload; internal refresh -> mirror to host"
while true; do
  sleep "$WATCH_INTERVAL"

  # a) VPN process not running (died, or a previous start failed) -> bring it back.
  if [ -z "$VPN_PID" ] || ! kill -0 "$VPN_PID" 2>/dev/null; then
    if [ -n "$VPN_PID" ]; then
      log "xmu_secure_link exited unexpectedly; last log lines:"
      tail -n 20 "$LOG_FILE" || true
      VPN_PID=""
    fi
    if start_vpn; then
      rebaseline
    else
      log "WARN: restart failed; will retry"
      sleep 10          # back off a little so a broken session can't restart-storm
    fi
  fi
  ensure_socks
  # b) client reconnected internally -> tun was recreated, re-pin lost routes.
  ensure_routes

  cur_src="$(fingerprint "$SRC_DIR")"
  cur_work="$(fingerprint "$WORK_DIR")"

  if [ "$cur_src" != "$LAST_SRC" ]; then
    # EXTERNAL change on the mount. Debounce (host editors may write partially).
    sleep "$WATCH_DEBOUNCE"
    if [ "$(fingerprint "$SRC_DIR")" != "$cur_src" ]; then
      continue          # still being written; re-check next tick
    fi
    log "EXTERNAL session/device change detected on the mount -> reloading client"
    sync_files "$SRC_DIR" "$WORK_DIR"      # reflect inward
    stop_vpn
    if start_vpn; then
      rebaseline
      log "reload complete"
    else
      log "WARN: reload restart failed; will keep watching"
      LAST_SRC="$(fingerprint "$SRC_DIR")"
      LAST_WORK="$(fingerprint "$WORK_DIR")"
    fi

  elif [ "$cur_work" != "$LAST_WORK" ]; then
    # INTERNAL refresh: the client rewrote its own session. Mirror it OUT to the
    # host so the shared copy stays current. NO restart.
    log "internal token refresh detected -> mirroring to host mount (no restart)"
    sync_files "$WORK_DIR" "$SRC_DIR"      # reflect outward
    LAST_WORK="$(fingerprint "$WORK_DIR")"
    LAST_SRC="$(fingerprint "$SRC_DIR")"   # our own write; don't treat as external
  fi
done
