#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

FOUNDRY_BIN="${FOUNDRY_BIN:-$HOME/.foundry/bin}"
CAST_BIN="${CAST_BIN:-$FOUNDRY_BIN/cast}"
RPC_URL="${HRX_BASE_SEPOLIA_RPC_URL:-https://sepolia.base.org}"
ROUTE_BOOK="${HRX_BASE_SEPOLIA_ROUTE_BOOK:-0x70594C7C33544fc0F22592005004B219dfbb012E}"
USDC="${HRX_BASE_SEPOLIA_USDC:-0x036CbD53842c5426634e7929541eC2318f3dCF7e}"
AGENT_ID="${HRX_SEED_AGENT_ID:-3377}"
OWNER_ADDRESS="${HRX_SEED_OWNER_ADDRESS:-0x6c69ee6e524f12d20c14c4b8caaa754012c9dc63}"
PRICE_PER_GB="${HRX_XRAY_PRICE_PER_GB:-1000000}"
STAKE_AMOUNT="${HRX_XRAY_STAKE_AMOUNT:-1000000}"
PROTOCOLS='["vless"]'

if [[ ! -x "$CAST_BIN" ]]; then
  echo "cast not found at $CAST_BIN" >&2
  exit 1
fi

if [[ $# -eq 0 ]]; then
  cat >&2 <<'EOF'
usage: ./script/seed-xray-vless-routes.sh --account <foundry-account> [wallet args...]

Seeds three VLESS offers derived from xray.json:
  1. Kazakhstan tcp+reality
  2. Finland grpc+reality
  3. US tcp+reality

Notes:
  - The grpc offer is valid on-chain and in discovery, but current hydra-core
    does not yet implement grpc VLESS connect-path, so it remains discoverable
    rather than data-plane usable.
  - This script approves 3x stake amount if current allowance is insufficient.
EOF
  exit 1
fi

total_stake_needed=$((STAKE_AMOUNT * 3))

current_allowance=$("$CAST_BIN" call \
  "$USDC" \
  "allowance(address,address)(uint256)" \
  "$OWNER_ADDRESS" \
  "$ROUTE_BOOK" \
  --rpc-url "$RPC_URL" | awk '{print $1}')

if [[ "${current_allowance:-0}" -lt "$total_stake_needed" ]]; then
  echo "Approving RouteBook to spend $total_stake_needed USDC base units..."
  "$CAST_BIN" send \
    "$USDC" \
    "approve(address,uint256)" \
    "$ROUTE_BOOK" \
    "$total_stake_needed" \
    --rpc-url "$RPC_URL" \
    "$@"
fi

seed_offer() {
  local label="$1"
  local region="$2"
  local bandwidth="$3"
  local endpoint="$4"

  echo
  echo "Seeding $label ($region, ${bandwidth} Mbps)"
  "$CAST_BIN" send \
    "$ROUTE_BOOK" \
    "createOffer(uint256,string,string[],string,uint256,uint256,uint256)" \
    "$AGENT_ID" \
    "$endpoint" \
    "$PROTOCOLS" \
    "$region" \
    "$PRICE_PER_GB" \
    "$STAKE_AMOUNT" \
    "$bandwidth" \
    --rpc-url "$RPC_URL" \
    "$@"
}

seed_offer \
  "xray-kz-tcp-reality" \
  "KZ" \
  "260" \
  "vless://b3985e56-9dd3-4bae-b330-9805d8611fea@kz-mg-01.org:8443?security=reality&type=tcp&sni=rbc.ru&fp=random&pbk=unyuuAPGgKOlZDNcwmKK6re8DwGMp0Npy675AOhQTD8&sid=7765629371647733&flow=xtls-rprx-vision" \
  "$@"

seed_offer \
  "xray-fi-grpc-reality" \
  "FI" \
  "180" \
  "vless://f5241b24-a5e2-3577-94f3-4fbc23ea6d32@213.176.92.19:36931?security=reality&type=grpc&path=festiveecclesia&sni=cloudflare.com&fp=chrome&pbk=sZ05YkXN0R1zve7XcBtR20xfSt7OrAhyEjOqJ6TpXEw&sid=0072ded4af" \
  "$@"

seed_offer \
  "xray-us-tcp-reality" \
  "US" \
  "240" \
  "vless://118c2bd4-4d09-44c1-8b72-c879922d4a45@132.243.172.102:443?security=reality&type=tcp&sni=github.com&fp=chrome&pbk=K5mJc5Yb-7EtatgE4lK3smUel4r0VYYr7AVQRnJdGHM&sid=267d9234db73217e" \
  "$@"

echo
echo "Done. Current total offers:"
"$CAST_BIN" call "$ROUTE_BOOK" "totalOffers()(uint256)" --rpc-url "$RPC_URL"
