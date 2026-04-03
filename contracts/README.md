# Hydra Route Exchange Contracts

Phase 1–4 contracts live here as an isolated Foundry project targeting **Base Sepolia**.

Current live deployment:

- `HydraRouteBook`: `0x70594C7C33544fc0F22592005004B219dfbb012E`
- Deploy tx: `0x258c73e8d1b85299739028e57f65399a13397c17893682d30fbea19f821f3aad`
- Deploy block: `39671496`
- Seeded agent / offer: `3377` / `#1`

**HydraDealBoard** (P2P Fiat Economy):
- `HydraDealBoard`: `0x0c811902c990c4D330c1269cc955140d975f7035`
- Deploy tx: `0xa2a3bce4789177f5337b1421dfa854e1ebe1a1c745cb4a71f89d9e39e2bf37e7`
- Deploy block: `39717554`
- Seeded dealer agent: `3377` owned by `0x6c69ee6e524f12d20c14c4b8caaa754012c9dc63`
- Seeded deal offer: `#1`
- Seed tx: `0x4137cc1811329072f3dd206937e34f214b43d7662318cad569341df2d237e46a`
- Seed block: `39719689`
- Seed params: `RUB`, rate `100_000_000` (100 RUB per 1 USDC with 6 decimals), min `1 USDC`, max `100 USDC`, payment method `bank_transfer`
- Live seeding note: `register()` / `ownerOf()` on the live ERC-8004 proxy returned spurious `NotActivated` errors under Foundry script simulation. The acceptance seed was finalized with direct `cast send` transactions instead of `seed-base-sepolia.sh`.

## Environment

Defaults:

- Base Sepolia RPC: `https://sepolia.base.org`
- Base Sepolia USDC: `0x036CbD53842c5426634e7929541eC2318f3dCF7e`
- Base Sepolia Identity Registry: `0x8004A818BFB912233c491871b3d84c89A494BD9e`
- Base Sepolia Reputation Registry: `0x8004B663056A597Dffe9eCcC1965A193B7388713`
- Withdrawal delay: `1 days`
- Escrow timeout: `1 days`

Required for live broadcast:

- `HRX_BASE_SEPOLIA_RPC_URL`
- Foundry signer flags such as `--account <foundry-account>` or `--private-key <hex>`

Optional:

- `HRX_BASE_SEPOLIA_USDC`
- `HRX_ERC8004_REPUTATION_REGISTRY`
- `HRX_ERC8004_IDENTITY_REGISTRY`
- `HRX_SEED_AGENT_ID`
- `HRX_BASE_SEPOLIA_ROUTE_BOOK`

If `forge` / `cast` are not in `PATH`, the shell wrappers default to `$HOME/.foundry/bin`.

## Common Commands

```bash
cd contracts
~/.foundry/bin/forge test
~/.foundry/bin/forge build
./script/deploy-base-sepolia.sh --account <foundry-account>
HRX_BASE_SEPOLIA_ROUTE_BOOK=0x... ./script/seed-base-sepolia.sh --account <foundry-account>
```

The deploy wrapper writes metadata to `contracts/deployments.json` after a successful broadcast.
The seed wrapper registers an ERC-8004 agent if `HRX_SEED_AGENT_ID` is unset, approves minimal USDC stake, and creates one `vless` offer in region `US` so Marketplace can be validated against non-empty live data.
For the current live Base Sepolia ERC-8004 proxy, direct `cast send` may be more reliable than Foundry script simulation.
