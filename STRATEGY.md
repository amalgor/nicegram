# Hydra: Product Strategy

**Last updated:** 2026-04-05

> Этот файл описывает продуктовую стратегию и forward-looking direction. Текущий shipping target в репозитории — Android network utility с WSS relay + imported VLESS; актуальный runtime/UI status фиксируется в `SERVICE_MANUAL.md`.

---

## 0. Current Shipping Direction (April 2026)

Near-term product strategy changed:
- **Ship first:** Android APK that works as a practical network utility
- **Do not ship yet:** embedded-wallet marketplace, automated monetization, in-app Cloudflare balance, bundled on-device model

Current MVP surface:
- Android VPN runtime with `Auto | Direct | WSS | VLESS | Block`
- built-in `Hydra WSS Relay`
- user-imported `vless://` credentials and V2Ray base64 subscriptions
- grouped `Connections` view with saved routing policies
- `Relay Usage` with local Cloudflare cost estimate and external donation/support link
- optional AI model download in Settings instead of bundling GGUF into APK

Reason for the pivot:
- it produces a shippable APK faster
- it avoids premature monetization and wallet friction
- it turns the existing PoC into a usable product before marketplace economics are hardened
- it preserves the long-term AI / crypto direction without forcing it into release-critical scope

---

## 1. Product Positioning

Long-term, Hydra is **not just a VPN**. Hydra is a **personal AI agent** that lives on the user's phone and:

- Summarizes incoming messages (TLDR-folding with 4 depth levels)
- Manages attention (tracks what matters, filters noise)
- Administers the network (routing, diagnostics, relay selection)
- Ensures connectivity as a side effect of its operation

Long-term proxying and censorship bypass are **means**, not the product's purpose. This distinction is still critical for:
- App store compliance (AI assistant, not circumvention tool)
- Marketing (productivity tool, not political statement)
- Legal positioning (communication tool with resilient transport)

Short-term shipping reality is different:
- first APK is allowed to be a narrower **network utility**
- AI assistant, wallet, marketplace, and content intelligence remain strategic differentiators, but not release blockers

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
              └── HRX client on Base Sepolia (local EOA wallet + on-chain offers/feedback)
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
- Revenue share tracked locally via hydra-econ debt ledger today
- Formal marketplace coordination moves to Hydra Route Exchange on Base Sepolia with app-local wallets

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
| Base gas friction / wallet backup UX | Medium | Medium | Base Sepolia first, explicit backup flow, local-wallet UX before mainnet |
| GGUF model too large for distribution | Low | Medium | Download-on-first-run for >0.5B models |
| P2P DHT bootstrap fails behind strict NAT | Medium | Medium | Relay-first architecture; DHT as optimization, not requirement |

---

## 9. Immediate Roadmap (Current Sprint)

1. Android APK with built-in WSS relay + imported VLESS profiles
2. Connections table with persisted app/domain route policies
3. Relay usage accounting and local Cloudflare cost estimate
4. Release hardening for Android VPN lifecycle, stop/start stability, and APK size
5. Optional AI download path in Settings without bundling GGUF into the package
6. Documentation and QA flow that treat marketplace/payments as postponed, not partially shipped

### Post-sprint
- Device validation and first public Android release
- Better app ownership resolution for non-Telegram traffic groups
- Safer donation / balance refill flow for relay operations
- Reintroduce AI-assisted analysis only after optional model install UX is proven
- Return to HRX marketplace / x402 / on-chain coordination once core utility adoption is real
- Folding UI (expandable content cards)
- LoRA personalization
- Cloud summarization of public channels
- Production USDC (mainnet)
- iOS build
- App store distribution
