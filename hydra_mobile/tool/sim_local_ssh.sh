#!/usr/bin/env bash
# Runs the real iOS app in the Simulator against a throwaway local sshd and
# checks the SOCKS5 proxy from the Mac (the Simulator shares the Mac's
# loopback, so 127.0.0.1:1080 inside the app is 127.0.0.1:1080 here).
#
# The server profile is created by the host harness and copied into the
# app's Documents directory, so no UI taps are needed.
#
#   hydra_mobile/tool/sim_local_ssh.sh                  # build, run, check, stop
#   SKIP_BUILD=1 KEEP=1 hydra_mobile/tool/sim_local_ssh.sh
#   SIM="iPhone 16" hydra_mobile/tool/sim_local_ssh.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP_DIR="$ROOT/hydra_mobile"
WORK="${WORK:-/tmp/hydra-sim}"
SSH_PORT="${SSH_PORT:-2223}"
SIM="${SIM:-iPhone 16 Pro}"
BUNDLE_ID="work.hydra-net.nicegram"
APP="$APP_DIR/build/ios/iphonesimulator/Runner.app"
HARNESS="$ROOT/target/debug/examples/proxy_harness"
SOCKS="127.0.0.1:1080"
PASS=0
FAIL=0

say() { printf '\n== %s\n' "$*"; }
ok() { PASS=$((PASS + 1)); printf '  ok   %s\n' "$*"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL %s\n' "$*"; }
fetch() { curl -sS -m 20 -o /dev/null -w '%{http_code}' --socks5-hostname "$SOCKS" "$1" 2>/dev/null || true; }
wait_for() { # pattern file seconds
  for _ in $(seq 1 $(($3 * 10))); do grep -q "$1" "$2" 2>/dev/null && return 0; sleep 0.1; done
  return 1
}
descendants() { local c; for c in $(pgrep -P "$1" 2>/dev/null); do descendants "$c"; echo "$c"; done; }
stop_sshd() {
  [[ -f "$WORK/sshd.pid" ]] || return 0
  local pid; pid="$(cat "$WORK/sshd.pid")"
  kill $(descendants "$pid") "$pid" 2>/dev/null || true
  rm -f "$WORK/sshd.pid"
}
CONSOLE_PID=""
cleanup() {
  [[ -n "$CONSOLE_PID" ]] && kill "$CONSOLE_PID" 2>/dev/null || true
  xcrun simctl terminate "$UDID" "$BUNDLE_ID" >/dev/null 2>&1 || true
  stop_sshd
}

if nc -z 127.0.0.1 1080 2>/dev/null; then
  echo "Port 1080 is already in use on the Mac; stop that process first."; exit 1
fi

if [[ -z "${SKIP_BUILD:-}" ]]; then
  say "build app for simulator + harness"
  (cd "$APP_DIR" && flutter build ios --simulator --debug)
fi
(cd "$ROOT" && cargo build -q -p rust_lib_hydra_mobile --example proxy_harness)

say "local sshd on 127.0.0.1:$SSH_PORT"
stop_sshd
rm -rf "$WORK" && mkdir -p "$WORK"
ssh-keygen -q -t ed25519 -N '' -f "$WORK/host_ed25519"
ssh-keygen -q -t ed25519 -N '' -f "$WORK/client_ed25519"
cp "$WORK/client_ed25519.pub" "$WORK/authorized_keys"
cat >"$WORK/sshd_config" <<EOF
Port $SSH_PORT
ListenAddress 127.0.0.1
HostKey $WORK/host_ed25519
AuthorizedKeysFile $WORK/authorized_keys
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
StrictModes no
AllowTcpForwarding yes
PidFile $WORK/sshd.pid
LogLevel VERBOSE
EOF
/usr/sbin/sshd -f "$WORK/sshd_config" -E "$WORK/sshd.log"
UDID=""
trap cleanup EXIT

say "seed server profile with the harness"
"$HARNESS" --base-dir "$WORK/seed" --ssh "$USER@127.0.0.1:$SSH_PORT" --key "$WORK/client_ed25519" >"$WORK/seed.log" 2>&1 &
SEED_PID=$!
wait_for "node started" "$WORK/seed.log" 30 || { tail -20 "$WORK/seed.log"; exit 1; }
kill "$SEED_PID"; wait "$SEED_PID" 2>/dev/null || true
ls "$WORK/seed"

say "simulator '$SIM'"
UDID="$(xcrun simctl list devices available | grep -F "    $SIM (" | head -1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/')"
[[ -n "$UDID" ]] || { echo "No simulator named '$SIM'"; exit 1; }
xcrun simctl boot "$UDID" 2>/dev/null || true
xcrun simctl bootstatus "$UDID" -b >/dev/null
open -a Simulator --args -CurrentDeviceUDID "$UDID"
xcrun simctl terminate "$UDID" "$BUNDLE_ID" >/dev/null 2>&1 || true
xcrun simctl install "$UDID" "$APP"
DATA="$(xcrun simctl get_app_container "$UDID" "$BUNDLE_ID" data)"
mkdir -p "$DATA/Documents"
rm -f "$DATA/Documents/mobile_routes.json" "$DATA/Documents/ssh_known_hosts.json"
cp "$WORK/seed/mobile_routes.json" "$WORK/seed/ssh_known_hosts.json" "$DATA/Documents/" 2>/dev/null || cp "$WORK/seed/mobile_routes.json" "$DATA/Documents/"
echo "  app data: $DATA"

say "launch app"
xcrun simctl launch --console-pty --terminate-running-process "$UDID" "$BUNDLE_ID" >"$WORK/app.log" 2>&1 &
CONSOLE_PID=$!
wait_for "Startup complete" "$WORK/app.log" 60 && ok "startup complete: $(grep -o 'Startup complete in [0-9]* ms' "$WORK/app.log" | head -1)" || bad "no 'Startup complete' in console"
wait_for "Proxy started" "$WORK/app.log" 30 && ok "proxy auto-started" || bad "proxy did not auto-start"

say "SOCKS5 through the app"
for url in https://example.com https://www.google.com/generate_204; do
  code=$(fetch "$url"); [[ "$code" =~ ^(200|204)$ ]] && ok "$url -> $code" || bad "$url -> $code"
done
codes=$(seq 1 20 | xargs -P 20 -I{} curl -sS -m 30 -o /dev/null -w '%{http_code}\n' --socks5-hostname "$SOCKS" "https://example.com/?n={}" 2>/dev/null | sort | uniq -c | tr -s ' ')
[[ "$codes" == " 20 200" ]] && ok "20 parallel requests" || bad "parallel requests: $codes"
wait_for "SSH .*-> connected" "$WORK/app.log" 10 && ok "controller saw SSH connected" || bad "no SSH connected transition"
ls "$DATA/Library/Application Support/logs"/hydra-*.log >/dev/null 2>&1 && ok "log files written" || bad "no log files in Application Support/logs"
grep -E "\[(ERROR)\]" "$WORK/app.log" | head -5 || true

if [[ "${KEEP:-}" == 1 ]]; then
  trap - EXIT
  say "KEEP=1: app and sshd left running (console log $WORK/app.log)"
fi
say "result: $PASS passed, $FAIL failed"
[[ $FAIL -eq 0 ]]
