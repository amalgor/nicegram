#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

FOUNDRY_BIN="${FOUNDRY_BIN:-$HOME/.foundry/bin}"
FORGE_BIN="${FORGE_BIN:-$FOUNDRY_BIN/forge}"
CAST_BIN="${CAST_BIN:-$FOUNDRY_BIN/cast}"
RPC_URL="${HRX_BASE_SEPOLIA_RPC_URL:-https://sepolia.base.org}"

if [[ ! -x "$FORGE_BIN" ]]; then
  echo "forge not found at $FORGE_BIN" >&2
  exit 1
fi

if [[ ! -x "$CAST_BIN" ]]; then
  echo "cast not found at $CAST_BIN" >&2
  exit 1
fi

FORGE_TARGET="script/DeployHydraRouteBook.s.sol:DeployHydraRouteBookScript"
BROADCAST_FILE="$ROOT_DIR/broadcast/DeployHydraRouteBook.s.sol/84532/run-latest.json"

"$FORGE_BIN" script "$FORGE_TARGET" --rpc-url "$RPC_URL" --broadcast "$@"

if [[ ! -f "$BROADCAST_FILE" ]]; then
  echo "Broadcast artifact not found: $BROADCAST_FILE" >&2
  exit 1
fi

TX_HASH="$(jq -r '[.transactions[] | select((.transactionType // "") == "CREATE")] | last.hash // empty' "$BROADCAST_FILE")"
if [[ -z "$TX_HASH" || "$TX_HASH" == "null" ]]; then
  TX_HASH="$(jq -r '.transactions[-1].hash // empty' "$BROADCAST_FILE")"
fi

if [[ -z "$TX_HASH" || "$TX_HASH" == "null" ]]; then
  echo "Unable to resolve deployment transaction hash from $BROADCAST_FILE" >&2
  exit 1
fi

RECEIPT_JSON="$("$CAST_BIN" receipt "$TX_HASH" --rpc-url "$RPC_URL" --json)"
CONTRACT_ADDRESS="$(jq -r '.contractAddress // empty' <<<"$RECEIPT_JSON")"
BLOCK_NUMBER_RAW="$(jq -r '.blockNumber // empty' <<<"$RECEIPT_JSON")"

if [[ -z "$CONTRACT_ADDRESS" || "$CONTRACT_ADDRESS" == "null" ]]; then
  echo "Unable to resolve deployed contract address for tx $TX_HASH" >&2
  exit 1
fi

if [[ -z "$BLOCK_NUMBER_RAW" || "$BLOCK_NUMBER_RAW" == "null" ]]; then
  echo "Unable to resolve block number for tx $TX_HASH" >&2
  exit 1
fi

if [[ "$BLOCK_NUMBER_RAW" == 0x* ]]; then
  BLOCK_NUMBER="$("$CAST_BIN" to-dec "$BLOCK_NUMBER_RAW")"
else
  BLOCK_NUMBER="$BLOCK_NUMBER_RAW"
fi

"$FORGE_BIN" script "$FORGE_TARGET" \
  --sig "writeDeploymentMetadata(address,bytes32,uint256)" \
  "$CONTRACT_ADDRESS" \
  "$TX_HASH" \
  "$BLOCK_NUMBER"

echo "HydraRouteBook deployed to $CONTRACT_ADDRESS"
echo "Deployment metadata written to $ROOT_DIR/deployments.json"
