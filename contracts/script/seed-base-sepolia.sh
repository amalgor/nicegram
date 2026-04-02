#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

FOUNDRY_BIN="${FOUNDRY_BIN:-$HOME/.foundry/bin}"
FORGE_BIN="${FORGE_BIN:-$FOUNDRY_BIN/forge}"
RPC_URL="${HRX_BASE_SEPOLIA_RPC_URL:-https://sepolia.base.org}"

if [[ ! -x "$FORGE_BIN" ]]; then
  echo "forge not found at $FORGE_BIN" >&2
  exit 1
fi

: "${HRX_BASE_SEPOLIA_ROUTE_BOOK:?HRX_BASE_SEPOLIA_ROUTE_BOOK must be set}"

FORGE_TARGET="script/SeedHydraRouteBook.s.sol:SeedHydraRouteBookScript"

"$FORGE_BIN" script "$FORGE_TARGET" --rpc-url "$RPC_URL" --broadcast "$@"
