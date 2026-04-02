# Hydra Route Exchange Contracts

Phase 1 contracts live here as an isolated Foundry project targeting **Base Sepolia**.

## Environment

Required:

- `HRX_BASE_SEPOLIA_RPC_URL`
- `HRX_ERC8004_IDENTITY_REGISTRY`

Optional:

- `HRX_BASE_SEPOLIA_USDC`
- `HRX_ERC8004_REPUTATION_REGISTRY`

Defaults:

- Base Sepolia USDC: `0x036CbD53842c5426634e7929541eC2318f3dCF7e`
- Withdrawal delay: `1 days`

## Common Commands

```bash
cd contracts
forge test
forge build
./script/deploy-base-sepolia.sh --account <foundry-account>
```

The deploy wrapper writes metadata to `contracts/deployments.json` after a successful broadcast.
