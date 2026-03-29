# Hydra: Product Strategy

**Last updated:** 2026-03-29

---

## 1. Product Positioning

Hydra is **not a VPN**. Hydra is a **personal AI agent** that lives on the user's phone and:

- Summarizes incoming messages (TLDR-folding with 4 depth levels)
- Manages attention (tracks what matters, filters noise)
- Administers the network (routing, diagnostics, relay selection)
- Ensures connectivity as a side effect of its operation

Proxying and censorship bypass are **means**, not the product's purpose. This distinction is critical for:
- App store compliance (AI assistant, not circumvention tool)
- Marketing (productivity tool, not political statement)
- Legal positioning (communication tool with resilient transport)

---

## 2. Architecture Summary

```
User's Phone
  └── Hydra Agent (Rust + Flutter)
        ├── On-device SLM (Qwen 0.5B-1.5B GGUF, llama.cpp)
        ├── Content Intelligence (Telegram via grammers MTProto)
        │     ├── Summarizer (LLM or heuristic fallback)
        │     ├── TLDR Folding (4 levels: headline → summary → key points → full)
        │     └── Attention Tracker (reading depth, time, interaction type)
        ├── Network Layer
        │     ├── SOCKS5 local proxy
        │     ├── P2P mesh (libp2p: Kademlia DHT, gossipsub, Noise+Yamux)
        │     ├── Cloudflare Worker WSS relay (censorship bypass)
        │     └── tun2proxy (Android VPN interface)
        └── Economic Layer
              ├── Trust/reputation scoring per peer
              ├── Traffic debt accounting
              └── USDC settlement (Circle developer-controlled wallets)
```

---

## 3. Target Markets and TAM

### Primary: Users under internet censorship

| Region | Situation (March 2026) | Internet users | VPN demand spike |
|--------|----------------------|----------------|-----------------|
| Russia | Telegram 80% blocked, 439 VPNs blocked, AI-DPI in deployment | 130M | Sustained high |
| Iran | Near-total blackouts, Strait of Hormuz crisis | 67M | +400,000% |
| Egypt | Discord/VoIP blocked | 82M | +320% |
| Sub-Saharan Africa | Election blackouts (Uganda +8,500%, Gabon +60,000%) | 500M+ mobile | Episodic spikes |
| South/SE Asia | Food crisis -> protests -> expected blackouts | 2B+ | Emerging |

### Secondary: Privacy-conscious AI users globally

- 2B smartphones now run local SLMs (2026 estimate)
- 80% of AI inference predicted on-device by end of 2026
- AI apps: $5B revenue, 3.8B downloads in 2025
- Privacy-first trend: users moving from cloud LLMs to on-device

### Conservative TAM estimate

- Censored regions: 800M internet users, 5% adoption = 40M users
- AI assistant market: 2B devices with NPU, 1% = 20M users
- Combined addressable: 50-60M users at scale

---

## 4. Quota and Subscription Model

### Free tier
- 50 MB/day proxied traffic
- 20 AI summarizations/day
- P2P relay participation earns bonus quota

### Tier 1 ($2/month)
- 500 MB/day + 200 summarizations
- Priority relay selection

### Tier 2 ($5/month)
- 2 GB/day + unlimited summarizations
- Custom relay endpoints
- Attention analytics dashboard

### P2P contribution bonus
- Users who relay traffic for others earn +X MB per Y MB relayed
- Incentivizes network growth organically

### Content Intelligence as traffic saver
- Headline (50 bytes) vs full text (5 KB) = 99x reduction at consumption level
- Cloud-summarized public channels: client downloads only folded version
- Full text loaded on-demand (user expands)

---

## 5. Franchise / Relay Operator Economics

```
User pays $5/month subscription
  ├── 60% → Platform (infrastructure, development, Workers hosting)
  ├── 25% → Relay node operators (proportional to traffic served)
  └── 15% → Franchise partner (user acquisition, local support)
```

### Relay operator model
- Anyone can run a Hydra relay node (Cloudflare Worker, VPS, or home server)
- Operators deploy their own Workers with custom domains
- Revenue share tracked via hydra-econ debt/settlement system
- Settlement in USDC via Circle developer-controlled wallets

### Franchise model
- Regional partners handle distribution, support, payment collection
- Partner deploys relay infrastructure in their region
- Revenue share automated through smart contracts (future) or centralized accounting (MVP)

### Risk mitigation for Cloudflare account sanctions
- Multiple accounts across franchise partners
- Custom domains (not *.workers.dev)
- Rapid rotation: if one Worker blocked, DHT propagates new endpoint within minutes
- P2P fallback: direct node-to-node relay when all Workers down

---

## 6. Competitive Landscape

| Product | What it does | Hydra's advantage |
|---------|-------------|-------------------|
| tg-ws-proxy (3600+ stars) | Desktop WebSocket proxy for Telegram | Hydra: mobile-first, AI agent, P2P mesh, not just Telegram |
| NymVPN | Mixnet-based privacy VPN | Hydra: on-device AI, content intelligence, lighter weight |
| Outline VPN | Self-hosted Shadowsocks | Hydra: no server needed for basic use, P2P relay |
| Cloudflare WARP | Consumer VPN by Cloudflare | Hydra: decentralized, AI features, not dependent on single provider |
| Lantern / Psiphon | Censorship circumvention tools | Hydra: AI-native, content summarization, economic incentives |

### Key differentiator
No competitor combines: on-device AI + content intelligence + P2P mesh + censorship bypass + crypto economics in a single mobile app.

---

## 7. Geopolitical Context and Timing (March 2026)

### Why now

1. **Strait of Hormuz crisis**: largest oil supply disruption in history (-8M bbl/day). Oil >$110, fertilizer prices +44%. 318M people in food crisis, +45M projected. This drives instability, protests, and government internet shutdowns globally.

2. **Russia's Telegram block**: 80% blocked as of March 17, full ban planned April 1. 50-80M users affected. TSPU (DPI infrastructure) overloaded by MTProxy traffic, periodically entering bypass mode.

3. **Global censorship wave**: Iran blackouts, Uganda/Gabon election shutdowns, Egypt VoIP blocks, Australia age restrictions. VPN demand spikes of 100x-400,000x in affected regions.

4. **On-device AI maturity**: Qualcomm Snapdragon 45 TOPS, Apple A18 40 TOPS. 0.5B-1.5B parameter models run comfortably on mid-range phones. The hardware is ready.

5. **Fertilizer → food → migration**: 25-33% of global fertilizer trade disrupted. Spring planting season at risk. Sub-Saharan Africa and South Asia face 17-24% increases in food insecurity. Mass migration events historically trigger communication blackouts.

### Window of opportunity
The combination of acute demand (censorship), mature technology (on-device LLM), and global instability creates a narrow window where a well-executed product can achieve rapid organic adoption. Users in crisis don't comparison-shop — they use what works.

---

## 8. Technical Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| CF Worker CPU limit (10ms) too tight for WSS relay | Medium | High | Prototype first; fallback to Durable Objects or VPS |
| SNI-based blocking of Worker domains | High (Russia) | Medium | Custom domains, ECH (Encrypted Client Hello), domain rotation |
| Circle KYC friction for settlement | Medium | Low | Testnet first; keep settlement interface abstract |
| GGUF model too large for distribution | Low | Medium | Download-on-first-run for >0.5B models |
| P2P DHT bootstrap fails behind strict NAT | Medium | Medium | Relay-first architecture; DHT as optimization, not requirement |

---

## 9. Immediate Roadmap (Current Sprint)

1. Cloudflare Worker WSS relay + Rust client
2. USDC settlement via Circle (testnet)
3. Gossipsub relay endpoint sharing
4. Quota system (KV + client tracking + UI)
5. Settings screen (proxy mode selection)
6. Android APK build and real-device test

### Post-sprint
- Folding UI (expandable content cards)
- LoRA personalization
- Cloud summarization of public channels
- Production USDC (mainnet)
- iOS build
- App store distribution
