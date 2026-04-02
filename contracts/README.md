# Hydra Route Exchange Contracts

Phase 1 contracts live here as an isolated Foundry project targeting **Base Sepolia**.

Current live deployment:

- `HydraRouteBook`: `0x70594C7C33544fc0F22592005004B219dfbb012E`
- Deploy tx: `0x258c73e8d1b85299739028e57f65399a13397c17893682d30fbea19f821f3aad`
- Deploy block: `39671496`
- Seeded agent / offer: `3377` / `#1`
- Agent registration tx: `0x869a57c910f7063ad45ff64e960538848e13aac225ed2e86b43f44be6f44506a`
- Offer creation tx: `0xc6460e57553ffce315421110baaefe184f9aac17cec84e55a996bf191a9f7751`
- Live seeding note: `register()` / `ownerOf()` on the live ERC-8004 proxy returned spurious `NotActivated` errors under Foundry script simulation. The acceptance seed was finalized with direct `cast send` transactions instead of `seed-base-sepolia.sh`.

## Environment

Defaults:

- Base Sepolia RPC: `https://sepolia.base.org`
- Base Sepolia USDC: `0x036CbD53842c5426634e7929541eC2318f3dCF7e`
- Base Sepolia Identity Registry: `0x8004A818BFB912233c491871b3d84c89A494BD9e`
- Base Sepolia Reputation Registry: `0x8004B663056A597Dffe9eCcC1965A193B7388713`
- Withdrawal delay: `1 days`

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
