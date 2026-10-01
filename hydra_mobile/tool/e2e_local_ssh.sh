#!/usr/bin/env bash
# End-to-end check of the Rust proxy runtime on the Mac, no phone needed.
#
# Starts an unprivileged sshd on 127.0.0.1:$SSH_PORT, runs the
# `proxy_harness` example (same Rust API the app calls) with an SSH server,
# and verifies SOCKS5 on 127.0.0.1:1080 with curl, including reconnect after
# sshd restarts, a wrong key, and a changed host key.
#
#   hydra_mobile/tool/e2e_local_ssh.sh            # all scenarios
#   KEEP=1 hydra_mobile/tool/e2e_local_ssh.sh     # leave the proxy running
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WORK="${WORK:-/tmp/hydra-e2e}"
SSH_PORT="${SSH_PORT:-2222}"
SOCKS="127.0.0.1:${SOCKS_PORT:-1080}"
HARNESS="$ROOT/target/debug/examples/proxy_harness"
PASS=0
FAIL=0

say() { printf '\n== %s\n' "$*"; }
ok() { PASS=$((PASS + 1)); printf '  ok   %s\n' "$*"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL %s\n' "$*"; }

start_sshd() {
  /usr/sbin/sshd -f "$WORK/sshd_config" -E "$WORK/sshd.log"
  for _ in $(seq 1 50); do nc -z 127.0.0.1 "$SSH_PORT" 2>/dev/null && return; sleep 0.1; done
  echo "sshd did not start"; exit 1
}
descendants() { local c; for c in $(pgrep -P "$1" 2>/dev/null); do descendants "$c"; echo "$c"; done; }
# Also kills the live sessions of this sshd: they otherwise survive a daemon restart.
stop_sshd() {
  local pid
  [[ -f "$WORK/sshd.pid" ]] || return 0
  pid="$(cat "$WORK/sshd.pid")"
  kill $(descendants "$pid") "$pid" 2>/dev/null || true
  rm -f "$WORK/sshd.pid"
  sleep 0.5
}
make_host_key() { rm -f "$WORK/host_ed25519"*; ssh-keygen -q -t ed25519 -N '' -f "$WORK/host_ed25519"; }

run_harness() { # base_dir key
  "$HARNESS" --base-dir "$1" --ssh "$USER@127.0.0.1:$SSH_PORT" --key "$2" >"$1.log" 2>&1 &
  HARNESS_PID=$!
  for _ in $(seq 1 100); do grep -q "node started" "$1.log" 2>/dev/null && return 0; sleep 0.1; done
  kill -0 $HARNESS_PID 2>/dev/null || return 1
}
HARNESS_PID=""
stop_harness() { # never `kill 0`: that signals the whole process group
  [[ -n "$HARNESS_PID" ]] || return 0
  kill "$HARNESS_PID" 2>/dev/null || true
  wait "$HARNESS_PID" 2>/dev/null || true
  HARNESS_PID=""
}
fetch() { curl -sS -m 20 -o /dev/null -w '%{http_code}' --socks5-hostname "$SOCKS" "$1" 2>/dev/null || true; }

trap 'stop_harness; stop_sshd' EXIT

say "build harness"
(cd "$ROOT" && cargo build -q -p rust_lib_hydra_mobile --example proxy_harness)

say "local sshd on 127.0.0.1:$SSH_PORT"
stop_sshd
rm -rf "$WORK" && mkdir -p "$WORK"
make_host_key
ssh-keygen -q -t ed25519 -N '' -f "$WORK/client_ed25519"
ssh-keygen -q -t ed25519 -N '' -f "$WORK/wrong_ed25519"
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
start_sshd

say "proxy through SSH"
run_harness "$WORK/app" "$WORK/client_ed25519" || { bad "harness did not start"; tail -30 "$WORK/app.log"; exit 1; }
grep -q '"ok":true' "$WORK/app.log" && ok "server test reports ok" || bad "server test failed: $(grep 'test result' "$WORK/app.log")"
for url in https://example.com https://www.google.com/generate_204; do
  code=$(fetch "$url"); [[ "$code" =~ ^(200|204)$ ]] && ok "$url -> $code" || bad "$url -> $code"
done
codes=$(seq 1 25 | xargs -P 25 -I{} curl -sS -m 30 -o /dev/null -w '%{http_code}\n' --socks5-hostname "$SOCKS" "https://example.com/?n={}" 2>/dev/null | sort | uniq -c | tr -s ' ')
[[ "$codes" == " 25 200" ]] && ok "25 parallel requests" || bad "parallel requests: $codes"
grep -q 'route_event' "$WORK/app/logs"/hydra-*.log && ok "log file written ($(ls "$WORK/app/logs" | head -1))" || bad "no log file"
grep -q 'host_key_pinned' "$WORK/app.log" && ok "host key pinned on first use" || bad "host key not pinned"
grep -q 'wss' <(grep "Active route" "$WORK/app.log") && bad "WSS relay still active next to SSH" || ok "only the SSH route is active"

say "sshd restart -> automatic reconnect"
stop_sshd
code=$(fetch https://example.com); [[ "$code" != 200 ]] && ok "fails while sshd is down ($code)" || bad "succeeded with sshd down?"
start_sshd
sleep 3.5 # FAILURE_HOLD_DOWN
code=$(fetch https://example.com); [[ "$code" == 200 ]] && ok "recovers after sshd restart" || bad "no recovery after restart ($code)"

say "host key change is refused"
stop_sshd; make_host_key; start_sshd
sleep 3.5
code=$(fetch https://example.com); [[ "$code" != 200 ]] && ok "request refused after host key change ($code)" || bad "accepted a changed host key"
grep -q 'host_key_mismatch' "$WORK/app.log" && ok "host_key_mismatch logged" || bad "mismatch not logged"
stop_harness

say "wrong key gives a clear error"
run_harness "$WORK/wrong" "$WORK/wrong_ed25519" || true
grep -q 'authentication rejected' "$WORK/wrong.log" && ok "auth rejection explained: $(grep -o 'SSH pubkey authentication rejected[^"]*' "$WORK/wrong.log" | head -1 | cut -c1-90)..." || bad "no auth error message"
stop_harness

if [[ "${KEEP:-}" == 1 ]]; then
  say "KEEP=1: proxy left running on $SOCKS"
  run_harness "$WORK/keep" "$WORK/client_ed25519"
  trap - EXIT
  echo "  harness pid $HARNESS_PID, sshd pid $(cat "$WORK/sshd.pid")"
fi

say "result: $PASS passed, $FAIL failed"
[[ $FAIL -eq 0 ]]
