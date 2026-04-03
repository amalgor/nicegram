# Техническая документация и архитектура (Service Manual)

**Непрерывность между сессиями:** дорожная карта этапов, статус P1–P8 и решения по компромиссам зафиксированы в [`hydra-architecture-review-3ce054.md`](hydra-architecture-review-3ce054.md) (корень репозитория). После сжатия контекста чата начинайте с того файла, затем с этого Service Manual.

## Обзор архитектуры
Hydra — это мульти-агентная P2P сеть, предназначенная для интеллектуальной маршрутизации трафика, обхода сетевых ограничений (DPI/цензуры) и проактивной обработки контента (суммаризация, TLDR-фолдинг, факт-чекинг). Главная инновация — локальная Small Language Model (SLM) на каждом узле для динамического принятия решений и обработки контента.

Проект написан на **Rust** и разделен на следующие крейты:
1. **hydra-config** — Централизованная конфигурация всего проекта (TOML). Все настраиваемые параметры сети, AI, экономики, Telegram и content в одном месте.
2. **hydra-core** — Ядро приложения. SOCKS5-сервер, координатор компонентов, multi-hop relay.
3. **hydra-p2p** — Сетевой слой на базе `libp2p`. Управляет соединениями, телеметрией и протоколами связи между узлами.
4. **hydra-ai** — Модуль искусственного интеллекта. Инференс локальной LLM (llama.cpp через llama-cpp-2 + GGUF) для маршрутизации, обработки контента и P2P deal scoring/re-ranking (`DealAgent`).
5. **hydra-content** — Content Intelligence. Telegram-клиент (grammers MTProto), TLDR-фолдинг, суммаризация через LLM, attention tracking (хранилище по конфигу `[content]`).
6. **hydra-econ** — Локальный экономический слой и репутационная система (на базе `sled`) для trust/debt без ончейн settlement.
7. **hydra-exchange** — Base Sepolia HRX client. Локальный EOA wallet (BIP-39), `alloy` bindings для `HydraRouteBook` и `HydraDealBoard`, ERC-8004 identity/reputation, USDC balance и P2P deal escrow lifecycle (`DealBoardClient`).
8. **hydra_mobile/rust** — Мост Flutter-Rust (flutter_rust_bridge). VPN-интерфейс (tun2proxy), менеджер моделей, телеметрия, API Content, credit-first Balance API, Share & Earn provider runtime и advanced Marketplace API для мобильного UI.

## Конфигурация
Все параметры вынесены в `hydra.toml` (TOML-файл в рабочей директории). При отсутствии файла используются значения по умолчанию. Пример конфигурации: `hydra.toml.example`.

Основные секции:
- **[network]** — `socks5_port`, `p2p_listen_port`, `bootstrap_nodes`
- **[ai]** — `model_path`, `max_generation_tokens`, `cache_ttl_seconds`, `cache_max_items`
- **[econ]** — `db_path`, `settlement_threshold_bytes`
- **[telegram]** — `api_id`, `api_hash`, `session_path`
- **[content]** — `db_path`, `summarization_max_tokens`, `cache_ttl_seconds`
- **[[transports]]** — transport list в порядке failover/приоритета: `type = "wss" | "vless"`, `mode = "telegram" | "all"`, transport-specific поля (`endpoints`, `device_id`, `url`)
- **[crypto]** — `enabled`, `chain`, `rpc_url`, `route_book_address`, `deal_board_address`, `identity_registry_address`, `reputation_registry_address`, `usdc_address`
- **[agent]** — P2P deal agent: `auto_spend_limit`, `max_rate_premium`, `preferred_payment_methods`, `min_dealer_reputation`
- **[discovery]** — параметры опроса `HydraRouteBook`: `poll_interval_secs`, `max_offers`, `prefer_free`, `rpc_timeout_secs`
- **[credit]** — локальная кредитная политика для premium routes: `trial_credit_usdc`, `linked_credit_usdc`, `growth_factor`, thresholds для nudge/throttle/fallback, `advanced_after_payments`
- **[bootstrap]** — `listen_port` (для bootstrap-нод)

На мобильном устройстве конфигурация загружается из `{app_documents_dir}/hydra.toml`, относительные пути автоматически разрешаются относительно `app_documents_dir`.
На Android/iOS этот файл теперь materialize-ится из bundled asset `hydra_mobile/assets/hydra.toml` при первом запуске приложения, если в documents dir ещё нет `hydra.toml`. Это защищает release build от silent fallback на `HydraConfig::default()`.

## Применяемые технологии и библиотеки
- **Сеть**: `libp2p` (TCP, Noise, Yamux, Kademlia DHT, Gossipsub, mDNS).
- **Gossipsub**: Топики `hydra/relay-endpoints/1.0` (relay endpoint sharing) и `hydra/services/1.0` (minimal unstaked service announcements для provider growth).
- **Асинхронность**: `tokio` (полный асинхронный рантайм).
- **WSS Relay**: `tokio-tungstenite` (клиент), Cloudflare Workers + Durable Object `HydraProviderSession` (сервер/provider pairing).
- **Локальный AI**: `llama-cpp-2` v0.1.140 (Rust binding к llama.cpp) — инференс GGUF моделей.
- **Модели на выбор**: Qwen 2.5 (0.5B, 1.5B), Qwen 3.5 (0.8B) — GGUF Q4_K_M квантизация.
- **База данных**: `sled` (встраиваемая key-value СУБД для хранения репутации пиров).
- **Конфигурация**: `toml` crate.
- **Кэширование AI**: `moka` (concurrent cache с TTL).
- **On-chain HRX client**: `alloy` + локальные ABI bindings, Base Sepolia, native Circle USDC `0x036CbD53842c5426634e7929541eC2318f3dCF7e`.
- **Форматы данных**: JSON (промпты AI, API), CBOR (бинарные сетевые протоколы).
- **Мобильный мост**: `flutter_rust_bridge` 2.11.1.
- **VPN перехват**: `tun2proxy` (SOCKS5 bridge через TUN-интерфейс Android).
- **Flutter UI**: Material 3, `shared_preferences` для runtime/filter state, `flutter_secure_storage` для хранения mnemonic в Android Keystore / iOS Keychain.

---

## Подробное описание компонентов

### 1. Конфигурация (hydra-config)
Единый крейт конфигурации для всех компонентов. Загружает `hydra.toml`, поддерживает значения по умолчанию и автоматическое разрешение относительных путей.

### 2. Сетевой слой (hydra-p2p)
Реализует P2P-сеть с использованием `libp2p`.
- **Обнаружение пиров (Discovery)**: Kademlia DHT для глобального поиска, mDNS для локального.
- **Туннелирование**: `libp2p-stream` с протоколом `TUNNEL_PROTOCOL` (`/hydra/tunnel/1.0.0`), определённым как константа.
- **Телеметрия**: 
  - Протокол `ping` замеряет RTT (задержку) до соседей.
  - Экспоненциальное сглаживание пропускной способности (Bandwidth) после каждого туннелированного соединения.
- **Co-Agent Diagnostics**: Кастомный протокол `request-response` (CBOR). Узел опрашивает соседей о доступности ресурса (TCP Ping) с их точки зрения.
- **Bootstrap**: Список bootstrap-нод загружается из конфигурации `[network].bootstrap_nodes`.

### 3. Искусственный интеллект (hydra-ai)
Мозг маршрутизации.
- **Конфигурируемый инференс**: Параметры (max_tokens, cache TTL/capacity) загружаются из `[ai]` секции конфигурации.
- **Подготовка промпта**: Приложение собирает список доступных пиров и их метрики (`trust_score`, `current_debt`, `rtt_ms`, `bandwidth_bps`). 
- **Структурированный вывод**: Модель возвращает JSON `{"path": [...], "transport": "vless"|"raw", "max_price": <float>}`.
- **Диагностический контекст**: При ошибке результаты Co-Agent Diagnostics включаются в промпт для перестроения маршрута.
- **Кэширование**: `moka` cache с конфигурируемым TTL и ёмкостью.
- **Fallback**: При недоступности модели — эвристика по trust_score и debt.
- **Горячая замена модели**: На мобильном устройстве пользователь может переключить модель через UI (`set_active_model`), модель перезагружается без перезапуска узла через `SHARED_AI`.

### 4. Экономика и Репутация (hydra-econ)
Каждый узел ведет локальный учет:
- **Trust Score (0-100)**: Повышается при успешном пропуске трафика (+1), понижается при обрывах (-10).
- **Debt (Долг)**: Учет объема переданных данных (в байтах).
- **Хранение**: Персистентно на диске, путь из `[econ].db_path`.
- **Proof of Transfer**: Каркас для верификации доставки данных.

### 4A. On-Chain HRX Client (hydra-exchange)
- **Chain target**: только Base Sepolia в рамках текущего runtime.
- **Wallet model**: локальный EOA wallet из 12-word BIP-39 mnemonic, derivation path `m/44'/60'/0'/0/0`.
- **Pinned public addresses**: USDC `0x036CbD53842c5426634e7929541eC2318f3dCF7e`, Identity Registry `0x8004A818BFB912233c491871b3d84c89A494BD9e`, Reputation Registry `0x8004B663056A597Dffe9eCcC1965A193B7388713`.
- **Live route book**: `0x70594C7C33544fc0F22592005004B219dfbb012E` on Base Sepolia, deploy tx `0x258c73e8d1b85299739028e57f65399a13397c17893682d30fbea19f821f3aad`, block `39671496`.
- **Acceptance seed data**: provider wallet `0x6c69ee6e524f12d20c14c4b8caaa754012c9dc63`, agent `3377` (tx `0x869a57c910f7063ad45ff64e960538848e13aac225ed2e86b43f44be6f44506a`), offer `#1` (tx `0xc6460e57553ffce315421110baaefe184f9aac17cec84e55a996bf191a9f7751`) with `region = US`, `protocol = vless`, `price = 1 USDC/GB`, `stake = 1 USDC`, `bandwidth = 100 Mbps`.
- **Live deal board**: `0x0c811902c990c4D330c1269cc955140d975f7035` on Base Sepolia, deploy tx `0xa2a3bce4789177f5337b1421dfa854e1ebe1a1c745cb4a71f89d9e39e2bf37e7`, block `39717554`.
- **Deal acceptance seed data**: same dealer wallet `0x6c69ee6e524f12d20c14c4b8caaa754012c9dc63`, agent `3377`, deal offer `#1` (tx `0x4137cc1811329072f3dd206937e34f214b43d7662318cad569341df2d237e46a`, block `39719689`) with `currency = RUB`, `rate = 100_000_000` (100 RUB per 1 USDC, 6 decimals), `min = 1 USDC`, `max = 100 USDC`, `payment_method = bank_transfer`.
- **Operational quirk**: live ERC-8004 proxy calls `register()` / `ownerOf()` return spurious `NotActivated` errors under Foundry script simulation. Actual on-chain `cast call/send` works. Current acceptance seed was completed with direct `cast send` instead of `seed-base-sepolia.sh`.
- **Bindings**: `HydraRouteBook`, official ERC-8004 ABI JSON for `IdentityRegistry.register()` / `ReputationRegistry.giveFeedback(...)`, ERC-20 `balanceOf`.
- **Read path**: `query_offers(region, protocol)` нормализует offers в мобильную view model и при наличии registry подмешивает reputation summary.
- **Write path**: agent registration и manual feedback submission подписываются локально через mnemonic, передаваемый из Flutter только на время операции.
- **RouteBook lifecycle**: `create_offer`, `deactivate_offer` и `withdraw_stake` подняты в `hydra-exchange` и mobile FRB; `create_offer` сам делает allowance check + `approve(route_book, stake)` перед `createOffer(...)`.

### 4B. Provider Growth & Soft Safety (Phase 4A/4B + 5A/5B-soft, 2026-04-02)
- **Mobile-first provider publication**: canonical consumer-facing mobile provider endpoint — `wss://relay.hydra-net.work?agent=<agent_id>`. Приложение публикует его через `hydra/services/1.0` как unstaked service announcement до RouteBook graduation.
- **Worker protocol**: `hydra-relay-worker` поддерживает два WebSocket режима:
  - provider: long-lived registration session (`X-Hydra-Mode: provider`, `X-Hydra-Agent`)
  - consumer: адресный доступ к конкретному agent (`X-Hydra-Mode: consumer`, `X-Hydra-Target-Agent`, `X-Hydra-Target`)
- **Transport update**: `hydra-core::transport::WssTransport` понимает `?agent=` и вместо legacy raw relay headers отправляет consumer-mode headers. Старый direct WSS relay path остаётся совместимым.
- **Provider runtime**: `hydra_mobile/rust/src/provider_runtime.rs` держит provider relay session, публикует gossip announcements, обслуживает control frames `connect/close/ready/closed` и считает local earnings.
- **Unstaked-first discovery**: `RouteDiscoveryService` теперь умеет объединять RouteBook offers и `hydra/services/1.0` announcements. Unstaked gossip routes score-ятся ниже staked on-chain routes и не вытесняют их при прочих равных.
- **Local-first reputation**: `hydra-econ::provider::ProviderMetricsLedger` хранит session count, bytes relayed, latency, throughput, uptime ratio, local routing score, pending reputation delta и earnings estimate. Route selection использует local score сразу.
- **Batched on-chain sync**: ERC-8004 feedback не пишется per-session. Ledger готовит threshold/window-based pending sync; из мобильного runtime он отправляется только в explicit secret-bearing operations, потому что mnemonic остаётся в `flutter_secure_storage` и не хранится в Rust-процессе постоянно.
- **Soft safety penalties**: discovery уже штрафует price outliers, very fresh routes, unstaked/zero-stake routes, low-feedback offers и relay concentration. Это penalty model, не hard block.

### 5. Ядро (hydra-core)
- **SOCKS5 Server**: Принимает соединения от локальных приложений, порт из `[network].socks5_port`.
- **Multi-hop Relay**: Последовательно устанавливает stream-каналы через промежуточные узлы.
- **Transport layer** (`transport/`): `Socks5Server` работает со списком `ConfiguredTransport` в порядке TOML-конфига. Поддерживаются `WssTransport` и `VlessTransport` (tcp+reality; grpc+reality URL currently parse-only).
- **Routing policy**: глобальный `proxy_mode` остаётся в `[network]` (`off | telegram | full`). Для proxied-трафика действует fail-closed: если подходящие transports исчерпаны, соединение закрывается без direct fallback.

### 6. Cloudflare Worker Relay (hydra-relay-worker/)
Отдельный проект (TypeScript, вне Rust workspace). Развёрнут на Cloudflare Workers free tier.
- **URL (primary):** `https://relay.hydra-net.work` (custom domain, устойчив к SNI-блокировке *.workers.dev)
- **URL (fallback):** `https://hydra-relay.hydra-net.workers.dev`
- **Домен:** `hydra-net.work` (Cloudflare Registrar, zone active, SSL auto-provisioned)
- **KV namespace:** `HYDRA_QUOTAS` (id: `82ed22205d834b94b87f42502823a59e`)
- **Durable Object namespace:** `HydraProviderSession` via `HYDRA_PROVIDER_SESSIONS` (current live worker version `93b673f4-6ccc-4e94-9256-72c9bdafc5f4`, deployed `2026-04-03T09:07:31Z`).
- **WSS-to-TCP relay**: Принимает WebSocket-соединения с заголовком `X-Hydra-Target: host:port`, открывает TCP-соединение к цели через `connect()` API.
- **Quota tracking**: KV namespace `HYDRA_QUOTAS` для учёта трафика по device_id с дневным лимитом (50 MB free tier).
- **Безопасность**: Whitelist Telegram DC IP-диапазонов (149.154.*, 91.108.*).
- **Endpoints**: `/health` (healthcheck), `/quota?device_id=...` (проверка квоты), WebSocket upgrade (relay).
- **Migration note**: в `wrangler.toml` используется `new_classes = ["HydraProviderSession"]`, а не `new_sqlite_classes`, потому что DO хранит только hibernated WebSocket session state и не использует persistent storage/SQLite API. Это осознанный KV-backed выбор; если позже понадобится durable storage или free-plan portability под новые DO defaults, схему миграции нужно пересмотреть.
- **Live verification (2026-04-03)**: `/health` возвращает `ok`; legacy direct relay проверен через `X-Hydra-Target` на `httpbin.org:80` с реальным HTTP payload и quota decrement (`codex-httpbin-org`); consumer без провайдера получает HTTP `503 Provider is offline`; provider registration через `X-Hydra-Mode: provider` успешно открывает WebSocket и получает `{"type":"registered"}`.
- **Ops report**: точные deploy/verification outputs сохранены в `reports/2026-04-03-ops-validation.md`.

### 7. Мобильный слой (hydra_mobile)
- **Flutter UI**: 7 вкладок — Connect, Network, Balance, AI, Content, Logs, Settings. Advanced Marketplace больше не является default entry-point и открывается из Balance.
- **UI Architecture**: Добавлены `credit/` слой (`CreditRepository`, backend, models), `screens/balance_screen.dart`, `widgets/credit_status_widget.dart`; `screens/marketplace_screen.dart` сохранён как advanced surface для power users/provider flows.
- **Rust bridge**: `flutter_rust_bridge` используется для network/runtime API, credit API (`get_credit_status()`, `get_nudge()`, `dismiss_nudge()`, `accept_trial_route()`, `get_telegram_anchor_info()`), provider API (`get_share_earn_status()`, `set_share_earn_enabled()`, `get_provider_earnings()`, `update_share_settings()`) и advanced Marketplace API (`get_marketplace_config_status()`, `create_wallet()`, `import_wallet()`, `get_wallet_balances()`, `list_route_offers()`, `register_agent()`, `create_offer()`, `deactivate_offer()`, `withdraw_stake()`, `submit_feedback()`).
- **Share & Earn in Balance**: Level 2 provider surface встроен в `BalanceScreen`. Там живут единый toggle, sharing status, starter/staked state, local score и earnings summary; full Marketplace остаётся advanced surface.
- **VPN**: Android VpnService → TUN FD → `tun2proxy` → локальный SOCKS5 → hydra-core. **Исправлен баг**: `vpn.rs` теперь читает `socks5_port` из загруженной конфигурации через `SOCKS5_PORT` AtomicU16, а не из `NetworkConfig::default()`.
- **Connection Tracking** (`hydra-core/src/connections.rs`): `ConnectionRegistry` — thread-safe реестр всех соединений с tracking bytes, route type, Telegram detection, AI reasoning, force-proxy override.
- **Route Discovery**: `hydra-core/src/discovery.rs` опрашивает `HydraRouteBook`, кеширует offers через `moka` и конвертирует transport-ready `endpoint_ciphertext` (`wss://` / `vless://`) в runtime transports.
- **Credit-first routing**: runtime сначала предпочитает free static relay, затем free discovered routes; premium discovered routes становятся доступными только после AI-assisted trial/credit approval. При перерасходе premium path мягко душится и затем fallback-ится на free route без hard disconnect.
- **Balance UX**: `Connect` и `Balance` показывают starter balance, current route state, AI nudge “Found a faster route”, reminder про local assistant memory и placeholder CTA для будущего top-up flow. Default copy избегает слов wallet/mnemonic/blockchain/ERC.
- **Content Intelligence**: Tap on dialog → `fetch_channel_messages()` → `MessageHandler::fetch_and_process()` → `Summarizer::process()` → `FoldableMessageCard` с 4 уровнями (Headline/Summary/KeyPoints/FullText). Attention tracking при expand/collapse.
- **Model Manager**: Скачивание моделей с HuggingFace, горячая замена через `SHARED_AI`.
- **Quota Manager** (`quota.rs`): Локальный трекинг потреблённого трафика с периодической синхронизацией с CF Worker KV.
- **Marketplace**: локальный wallet onboarding, balances (ETH + USDC), ERC-8004 agent registration, filters `region/protocol` с `SharedPreferences`, offers list и manual feedback. По умолчанию скрыт за advanced toggle или unlock после 3+ successful payments.
- **Marketplace runtime states**: экран явно различает `disabled`, `incomplete config`, `no offers` и `load failed`; пустой `route_book_address` в `hydra.toml` больше не вываливает сырой backend exception в UI.
- **Settings**: Proxy mode, краткая on-chain status card и advanced tools toggle. Legacy settlement UI удалён.

---

## Архитектура сети (Как проходит пакет)

1. **User Application** отправляет запрос на `127.0.0.1:{socks5_port}` (SOCKS5).
2. **hydra-core** извлекает целевой IP/Domain и порт.
3. Ядро запрашивает телеметрию по всем известным пирам из **hydra-p2p** и баланс из **hydra-econ**.
4. Формируется запрос в **hydra-ai** (включая diagnostic_context при повторных попытках).
5. SLM генерирует JSON: `{"path": ["PeerA", "PeerB"], "transport": "vless", "max_price": 0.0}`.
6. **hydra-core** открывает `TUNNEL_PROTOCOL` stream к PeerA, передает команду соединиться с PeerB.
7. После "рукопожатия", данные проксируются через P2P stream. Последний хоп открывает TCP сокет к Target.
8. При ошибке запускается **Co-Agent Diagnostics** — опрос других пиров о доступности Target, повторная генерация маршрута AI.

---

## Roadmap (План дальнейшего развития)

Синхронизировано с `hydra-architecture-review-3ce054.md` (часть 5) и `STRATEGY.md`.

### Этап 0–1 (стабилизация + Content baseline) — ВЫПОЛНЕНО
- Конфигурация `hydra-config`, исправления P2P/AI/путей модели, `set_active_model`, крейт `hydra-content`, Flutter вкладка Content и Android-сборка.

### Sprint: Network + Crypto + Content (март 2026) — ВЫПОЛНЕНО
- **Cloudflare Worker WSS relay** (`hydra-relay-worker/`): WSS-to-TCP proxy с квотами в KV.
- **Rust WSS relay client** (`hydra-core/src/relay.rs`): `tokio-tungstenite`, интеграция в SOCKS5 handler с fallback.
- **Gossipsub relay sharing**: топик `hydra/relay-endpoints/1.0` в `hydra-p2p`, publish/subscribe.
- **Quota system**: CF Worker KV tracking + Rust `quota.rs` + Flutter circular progress widget.
- **Settings screen**: proxy mode control и runtime wiring для transport policy.
- **STRATEGY.md**: продуктовое позиционирование, экономика, целевые рынки.
- **Android APK**: успешная сборка с новыми компонентами.

### HRX Phase 1: On-Chain Route Book (апрель 2026) — ВЫПОЛНЕНО
- **Foundry project** (`contracts/`): isolated контрактный workspace для Base Sepolia.
- **HydraRouteBook**: escrow-модель офферов, query API, delayed withdrawal, slash-claim hook.
- **Deploy flow**: Base Sepolia deploy script + `deployments.json` для dev/test артефактов.
- **Runtime direction**: Arbitrum/Circle settlement path признан legacy и выведен из дальнейшего плана.

### HRX Phase 2: Embedded Wallet + Marketplace (апрель 2026) — ВЫПОЛНЕНО
- **`hydra-exchange`**: новый Rust crate на `alloy` для Base Sepolia reads/writes.
- **Config migration**: `[crypto]` переведён на Base Sepolia schema без legacy REST settlement ключей.
- **Tracked config**: корневой `hydra.toml` мигрирован на `[[transports]]` + новый `[crypto]`, официальные Base Sepolia registry addresses pinned.
- **Local wallet custody**: mnemonic хранится только через `flutter_secure_storage`, в Rust передаётся лишь на время подписания.
- **Marketplace tab**: wallet, balances, ERC-8004 agent registration, on-chain offers, manual feedback.
- **Live deployment**: `contracts/deployments.json` заполнён реальным Base Sepolia deploy; `route_book_address` записан в корневой `hydra.toml`.
- **Seeded acceptance data**: создан live agent `3377` и offer `#1`, так что Marketplace можно валидировать против непустого on-chain state.
- **Docs cleanup**: legacy settlement wording удалён из активной архитектуры.

### HRX Phase 3 MVP: Credit-First Route Discovery (апрель 2026) — РЕПОЗИТОРИЙ ОБНОВЛЁН
- **Config expansion**: добавлены `[discovery]` и `[credit]` в `hydra-config`, `hydra.toml` и `hydra.toml.example`.
- **Dynamic route discovery**: `hydra-core::discovery::RouteDiscoveryService` читает активные offers из `HydraRouteBook`, кеширует их и подмешивает discovered transports в `Socks5Server` без отказа от static transports.
- **Credit runtime**: `hydra-econ::credit::CreditLedger` хранит локальный trial/linked credit state, usage, debt, thresholds и правила unlock-а advanced tier.
- **Anchor strategy**: primary anchor — локально salted Telegram-derived hash, fallback — local installation id. При появлении Telegram авторизации install anchor аккуратно merge-ится в linked anchor.
- **Balance-first UX**: вкладка Balance и `CreditStatusWidget` на Connect screen заменяют crypto-native entry flow. Advanced Marketplace остаётся в кодовой базе, но спрятан за progressive disclosure.
- **Provider growth path**: Share & Earn больше не рассматривается как CLI/dashboard feature. Основной provider onboarding теперь mobile-first и живёт внутри Balance; отдельный CLI для VPS-операторов остаётся вторичным thin wrapper на тот же write path.
- **Graceful degradation**: premium routes скрыты до accept trial, при превышении лимита runtime возвращается на free routes вместо hard disconnect.
- **Verification**: локально проходят `cargo test -p hydra-econ`, `cargo test -p hydra-exchange`, `cargo test -p hydra-core`, `cargo test -p rust_lib_hydra_mobile --lib`, `npm exec tsc --noEmit` в `hydra-relay-worker`, `flutter analyze`, `flutter test`. Device validation premium/provider routing и будущий top-up flow остаются следующим шагом.

### Sprint: Mobile MVP Readiness (март 2026) — ВЫПОЛНЕНО
- **Unit tests (64 теста)**: SOCKS5 parsing, relay logic, quota, AI routing, config loading, connection registry, integration tests (SOCKS5 end-to-end, domain connect, auth rejection, registry tracking, AI routing).
- **Connection tracking** (`hydra-core/src/connections.rs`): `ConnectionRegistry` с bytes tracking, route type, Telegram detection, AI reasoning, GC.
- **Selective Telegram routing**: Telegram DC IPs (149.154.*/91.108.*) автоматически через relay, остальное — direct. Per-connection proxy toggle.
- **VPN port bug fix**: `vpn.rs` теперь использует `SOCKS5_PORT` AtomicU16 из загруженного конфига.
- **SOCKS5 pure functions** (`hydra-core/src/socks.rs`): Извлечены testable функции из `handle_connection`.
- **AI content bridge**: `fetch_channel_messages()` → `MessageHandler::fetch_and_process()` → summarization → JSON для Flutter.
- **Flutter UI restructure**: Разделение `main.dart` (1050 строк) на 5 screen-файлов + 3 widget-файла.
- **Connections screen**: Live connection list с route badges (DIRECT/RELAY/P2P), bytes, duration, Telegram tag, AI reasoning, per-connection proxy Switch.
- **Content screen**: Tap on dialog → message list с `FoldableMessageCard` (4 fold levels + attention tracking).
- **Connect screen**: Connection summary (active/proxied/direct counts).
- **Config `#[serde(default)]`**: Все config structs теперь поддерживают partial TOML (missing sections → defaults).

### Android Build & Runtime Fixes (29 марта 2026) — ВЫПОЛНЕНО
- **Logs screen** (`lib/screens/logs_screen.dart`): Добавлен экран логов с фильтрацией, auto-scroll, copy-all, цветовой маркировкой по уровню (ERROR/WARN/INFO/DEBUG/TRACE). После Marketplace расширена навигация до 7 вкладок.
- **Double-start protection**: `start_hydra_node()` защищён `AtomicBool` — повторные вызовы игнорируются (idempotent).
- **P2P graceful fallback**: На Android libp2p не может инициализироваться (нет `/etc/resolv.conf`). `P2PNode::dummy_handle()` создаёт no-op handle, SOCKS5 сервер продолжает работу без peer discovery (relay-only mode).
- **Direct-first routing strategy**: Telegram-трафик сначала пробует прямое подключение (5s timeout), и только при неудаче переключается на WSS relay. Это решает проблему routing loop (relay через VPN) и обеспечивает минимальную задержку когда Telegram не заблокирован.
- **Relay TLS fix**: `tokio-tungstenite` переключён с `rustls-tls-native-roots` на `rustls-tls-webpki-roots` (встроенные Mozilla CA). Android не имеет стандартного trust store для rustls.
- **Relay Worker fix**: `secureTransport: "off"` — Worker теперь прозрачный TCP-прокси, TLS делает клиент (без двойного TLS).
- **Relay timeout**: WebSocket handshake ограничен 10 секундами.
- **UDP ASSOCIATE silenced**: SOCKS5 CMD 0x03 (UDP от tun2proxy DNS) теперь DEBUG вместо ERROR.
- **Default relay endpoint**: `wss://relay.hydra-net.work` в `RelayConfig::default()`.
- **VPN callback fix**: `HydraVpnService` теперь вызывает `onVpnStarted?.invoke(fd)` вместо broadcast.
- **Node auto-start**: `startHydraNode()` вызывается при запуске приложения (`main.dart`), не только при нажатии Connect.
- **Flutter deprecation fixes**: `RadioGroup` вместо deprecated `Radio.groupValue/onChanged`, `activeThumbColor` вместо `activeColor`.
- **Проверено на устройстве**: Xiaomi M2101K7BNY, APK 522.9MB (включает bundled qwen2.5-0.5b.gguf 462MB). VPN перехватывает трафик, Telegram подключается напрямую, остальной трафик (Google, Xiaomi, CapCut и др.) проходит direct.

### UX & Routing Improvements (29 марта 2026) — ВЫПОЛНЕНО
- **Logs UX overhaul**: Лимит логов поднят до 10000 строк. Добавлены временные метки (HH:MM:SS.mmm) в начале каждой строки. Фильтр по LOG_LEVEL (ALL/ERROR/WARN/INFO/DEBUG/TRACE) через FilterChip. Постоянно видимые скроллбары (`thumbVisibility: true`). Auto-scroll вниз только когда пользователь в самом низу списка (не прыгает при просмотре старых логов). Кнопка "Scroll to bottom" для возврата.
- **Cloudflare DNS (DoH)**: `libp2p` SwarmBuilder переключён с `.with_dns()` (читает `/etc/resolv.conf`, падает на Android) на `.with_dns_config(ResolverConfig::cloudflare(), ...)` — использует Cloudflare 1.1.1.1/1.0.0.1 напрямую. P2P node теперь должен инициализироваться на Android без `dummy_handle()`.
- **Relay-first routing**: Telegram-трафик теперь идёт через relay в первую очередь (а не direct-first). Логика: если пользователь включил проксирование, значит прямое соединение неудобно (замедление). Direct используется только как fallback при недоступности relay.
- **Proxy mode runtime control**: Настройка proxy_mode из Settings (off/telegram/full) теперь передаётся в Rust SOCKS5 сервер через `Arc<RwLock<String>>`. При изменении в UI — мгновенно применяется к новым соединениям. При старте приложения — восстанавливается из SharedPreferences.
- **Simplified routing logic**: Удалён мёртвый P2P routing code из `handle_connection` (P2P ещё не работает на мобильном). Код сократился с ~500 строк до ~250. Извлечён `do_direct()` helper. AI/P2P/Econ параметры убраны из `handle_connection` (остались в `Socks5Server` struct для будущего использования).
- **Telegram domain detection**: Добавлен `telegram-cdn.org` в список доменов для автоматического проксирования.
- **Config mode rename**: `relay.mode` переименован с "auto" на "telegram" (более понятно). Обновлены hydra.toml, hydra.toml.example, все тесты.

### Network & Connect UI + LLM Analysis (30 марта 2026) — ВЫПОЛНЕНО
- **Network screen полностью переписан**: Соединения группируются по домену приложения (google.com, telegram.org и т.д.). Каждая группа показывает: иконку, домен, badge RELAY/DIRECT/TG, количество соединений, объём трафика, route summary, количество активных. Разворачивается в список индивидуальных соединений с компактным отображением (host:port, route badge, bytes, duration).
- **Async LLM security comments**: При развороте группы асинхронно запрашивается анализ безопасности от встроенной Qwen 2.5 модели (`analyze_host()`). Результат кешируется и отображается с цветовой маркировкой [OK]/[WARN]/[ALERT].
- **Connect screen переделан**: Кнопка Connect стала компактнее (160px). Добавлен таймер uptime. Stats grid с 4 карточками (Active/Relayed/Direct/Total) с иконками. Traffic bar (Up/Down) с цветовой индикацией. LLM Security Analysis card — каждые 30 секунд автоматически запрашивает `analyze_connections()` с обзором всех активных соединений. Кнопка ручного re-analyze.
- **Rust API**: Добавлены `analyze_connections(json)` и `analyze_host(host, port, is_proxied, bytes)` — промпты для Qwen 2.5 с [OK]/[WARN]/[ALERT] тегами. Используют `SHARED_AI` lock для доступа к модели.
- **FRB codegen (30 марта 2026)**: Запущен `flutter_rust_bridge_codegen generate`, все FRB API (`getActiveConnections`, `getConnectionStats`, `setConnectionProxy`, `setProxyMode`, `analyzeConnections`, `analyzeHost`) сгенерированы как real bindings вместо стабов. Content hash: `-2077367203`. Исправлен missing `CachedContent` struct в `hydra-content/src/attention/tracker.rs`.

#### Bootstrap-нода (boot.ze1.org) — обновлена 30 марта 2026
- Код синхронизирован через rsync в `~/src/marx/` (без hydra_mobile, .git, target, .gguf).
- Установлены `libclang-dev`, `cmake` для сборки llama-cpp-sys.
- Собран `hydra-core --bootstrap` в release mode.
- Binary заменён в `/opt/hydra/target/release/hydra-core`, systemd service перезапущен.
- PeerID: `12D3KooWBJvpWZ7Mr2xymmx1ZVXBbRFSagUCMJoEUFyoN1JcW46t`, listen: `/ip4/159.69.213.174/tcp/33097`.
- systemd unit: `/etc/systemd/system/hydra-core.service`, `Restart=always`, `WorkingDirectory=/opt/hydra/hydra-core`.

#### OOM crash fix (30 марта 2026)
- **Причина**: Android OOM killer убивал приложение через ~60 секунд после запуска. llama-cpp загружал AI модель (Qwen 2.5 0.5B, 468 MB) при старте ноды, что вместе с VPN + P2P + 250+ соединений превышало лимит памяти на устройстве с 5.6 GB RAM.
- **Исправление**:
  1. Загрузка AI модели сделана **ленивой** (lazy) — не грузится при старте, только по явному запросу пользователя через Settings > AI Models.
  2. Параметры llama.cpp: `mmap=true, mlock=false` для снижения RSS.
  3. Контекст уменьшен с 2048 до 512 токенов (достаточно для коротких security-промптов).
  4. `analyze_connections()` и `analyze_host()` возвращают fallback-сообщение "[OK] AI not loaded" вместо ошибки, если модель не загружена.
- **Результат**: приложение стабильно работает 5+ минут, RSS ~240 MB.

#### Relay Worker fix (30 марта 2026)
- **Проблема**: данные не проходили через `relay.hydra-net.work`. Три причины:
  1. **Worker: Stream cancelled** — TCP->WS pump запускался как IIFE, но CF Workers runtime отменял readable stream после возврата Response. Исправлено: используется `pipeTo()` + `ctx.waitUntil()`.
  2. **Worker: isAllowedTarget** — фильтр пропускал только Telegram IP, блокируя остальные targets с 403. Удалён, т.к. квотирование обеспечивает защиту от злоупотреблений.
  3. **Worker: множественный getWriter()** — каждое WS сообщение создавало новый writer и вызывало race condition. Исправлено: один writer на соединение.
- **Routing fix на клиенте (Rust)**: в режиме "full" VPN все 200+ соединений отправлялись в relay одновременно, перегружая tokio runtime (timeout не срабатывал). Исправлено:
  - Relay (WSS через Cloudflare) используется **только для Telegram** (anti-censorship).
  - В "full" VPN: весь трафик идёт через VPN tunnel -> SOCKS5, но только Telegram через WSS relay. Остальной — direct из SOCKS5.
  - Добавлен `is_relay_infrastructure()` — connections к `relay.hydra-net.work`, `boot.ze1.org` всегда direct (защита от routing loop).
- **Tracing filter**: добавлен `EnvFilter` — libp2p, noise, rustls на уровне WARN, hydra-core/relay на DEBUG. Убрана лавина TRACE логов.
- **Relay semaphore**: `MAX_CONCURRENT_RELAY = 4` — ограничение одновременных WSS handshake, чтобы blocking DNS (getaddrinfo) не съедал все tokio worker threads.
- **VPN autostart**: ConnectScreen запускает VPN автоматически через 3 сек после старта (если VPN consent уже получен).
- **singleTask**: `android:launchMode="singleTask"` в AndroidManifest — предотвращает создание дублирующих экземпляров приложения.
- **Файлы**: `hydra-relay-worker/src/index.ts`, `hydra-core/src/lib.rs`, `hydra-core/src/socks.rs`, `hydra-core/src/relay.rs`, `hydra_mobile/rust/src/api/simple.rs`, `hydra_mobile/lib/screens/connect_screen.dart`, `hydra_mobile/android/app/src/main/AndroidManifest.xml`.

#### Relay TLS handshake fix (30 марта 2026)
- **Проблема**: WSS relay-соединения зависали на этапе TLS handshake. `connect_async` (tokio-tungstenite) внутри использовал `tokio::net::TcpStream::connect`, который вызывает blocking `getaddrinfo` через `spawn_blocking`. При >=4 одновременных relay tasks все worker threads оказывались заняты, и tokio timer futures не могли выполниться (timeout 10s не срабатывал).
- **Корневая причина (2-я)**: rustls v0.23 требует явной инициализации `CryptoProvider`. Ранее `connect_async` от tokio-tungstenite делал это внутренне, но ручной TLS через `tokio-rustls` падал с паникой "Could not automatically determine the process-level CryptoProvider".
- **Исправление**:
  1. **Ручной TCP -> TLS -> WS pipeline** вместо `connect_async`: каждый этап (TCP connect, TLS handshake, WS upgrade) имеет собственный 5-секундный timeout.
  2. **`build_tls_connector()`**: создаёт `tokio_rustls::TlsConnector` с явным `rustls::crypto::ring::default_provider()` и `webpki_roots` CA store.
  3. **`tokio::spawn`** для relay task: гарантирует что timeout futures получают отдельный poll cycle, не блокируясь вызывающим task.
  4. **Новые зависимости** в `hydra-core/Cargo.toml`: `tokio-rustls = "0.26"`, `rustls = { version = "0.23", features = ["ring"] }`, `webpki-roots = "0.26"`.
  5. `connect_to_target` изменён на `self: &Arc<Self>` для совместимости с `tokio::spawn`.
- **Результат**: WSS relay подключается за ~200ms (TCP ~50ms + TLS ~100ms + WS upgrade ~50ms). Все Telegram DC адреса (149.154.x.x, 91.108.x.x) проксируются через `relay.hydra-net.work`. Worker подтверждает передачу данных (1-6 KB per connection).
- **Файлы**: `hydra-core/src/relay.rs`, `hydra-core/Cargo.toml`.

### HRX Phase 4: P2P Fiat Economy (HydraDealBoard) — РЕПОЗИТОРИЙ ОБНОВЛЁН
- **HydraDealBoard.sol**: self-contained escrow contract for P2P fiat-to-USDC deals. Dealer posts offer (currency, rate, min/max, payment methods), buyer accepts and locks USDC into escrow, lifecycle: Funded -> Sent -> Completed/Rejected/Expired. Reputation feedback via ERC-8004 ReputationRegistry on completion/rejection.
- **Foundry tests**: 30 tests covering full deal lifecycle — offer creation/deactivation, deal acceptance, fiat marking, receipt confirmation, rejection, expired claim, edge cases.
- **Deploy script**: `contracts/script/DeployHydraDealBoard.s.sol` for Base Sepolia with pinned addresses.
- **Config**: `deal_board_address` added to `[crypto]` in `hydra.toml` and `CryptoConfig`/`ExchangeConfig`. New `[agent]` section with `auto_spend_limit`, `max_rate_premium`, `preferred_payment_methods`, `min_dealer_reputation`.
- **Alloy bindings**: `HydraDealBoard` bindings in `hydra-exchange/src/bindings.rs` with selector tests.
- **DealBoardClient** (`hydra-exchange/src/deal_client.rs`): query deals by currency, get offer, accept deal (with escrow event decoding), mark fiat sent, check escrow status, claim expired, approve USDC. 9 unit tests pass.
- **DealAgent** (`hydra-ai/src/deal_agent.rs`): AI-powered deal scoring (reputation 40%, rate 40%, payment method 20%), LLM re-ranking of top-5 deals when model loaded, auto-approve vs confirmation decision based on `auto_spend_limit`. 5 unit tests pass (16 total in hydra-ai).
- **Flutter UI**: `deals_screen.dart` with currency filter, deal cards, accept dialog. Wired to Balance screen "Top Up" button. FRB API functions in `exchange.rs` (7 new endpoints). Models, backend, repository layers extended.
- **Live deployment**: `0x0c811902c990c4D330c1269cc955140d975f7035` on Base Sepolia, deploy tx `0xa2a3bce4789177f5337b1421dfa854e1ebe1a1c745cb4a71f89d9e39e2bf37e7`, block `39717554`. `deal_board_address` recorded in `hydra.toml` and `deployments.json`.
- **Operational follow-up (2026-04-03)**: FRB codegen выполнен, `flutter analyze`/`flutter test` зелёные, release APK собран в `hydra_mobile/build/app/outputs/flutter-apk/app-release.apk`, DealBoard `totalOffers()` теперь `1` после live seeding offer `#1`.
- **Physical Android validation (2026-04-03)**: release APK проверен на устройстве `M2101K7BNY` (Android 13). Подтверждены `VpnService`/`tun0`, Balance UX, Share & Earn toggle, live `P2P Deals` offer `#1` и live RouteBook offer `#1` через advanced Marketplace. Runtime blockers тоже закрыты: `hydra.toml` теперь materialize-ится из bundled asset на first launch, `Full VPN` больше не упирается в missing transports, а `ParcelFileDescriptor` double-close исправлен через `detachFd()` в Android `HydraVpnService`. После фикса non-Telegram traffic в `full` режиме реально пошёл через `wss://relay.hydra-net.work`, а 3 stop/start VPN cycles подряд прошли без `fdsan` и без смены PID. Подробности: `reports/2026-04-03-ops-validation.md`.
- **TLS fix (Android)**: `rustls-platform-verifier` паникует на Android без JNI-инициализации. Исправлено: `hydra-exchange/src/config.rs` — синглтон `http_client()` строит `reqwest::Client` с явным `ring` CryptoProvider + `webpki-roots` корневыми сертификатами. Все `ProviderBuilder::new().connect_http(url)` заменены на `connect_reqwest(http_client(), url)` в `client.rs` и `deal_client.rs` (18 call sites). Зависимости `rustls`, `webpki-roots`, `reqwest` добавлены в `hydra-exchange/Cargo.toml`. 13 тестов проходят.
- **Pending**: iOS device validation, real mobile provider session against deployed Worker, и end-to-end user validation именно Telegram censorship path / premium trial UX, уже поверх исправленного Android runtime.

### Этап 2: Attention + персонализация
- **AttentionTracker** — полнота клиентского трекинга и политика событий.
- **Локальное хранение** — SQLite для статистики чтения.
- **Дообучение промптов** по интересам пользователя и опционально **LoRA** для суммаризации.

### Доработки Content (параллельно этапу 2)
- Углубление **иерархического TLDR-фолдинга** (4 уровня) и полировка UX карточек каналов.
- **Локальная обработка** приватных чатов: явные гарантии и настройки приватности в UI.

### Этап 3: Production deployment
- **Custom domain** для CF Worker relay — ВЫПОЛНЕНО: `relay.hydra-net.work` (домен `hydra-net.work`).
- **HRX settlement hardening**: offer creation/staking UI, x402 flow, mainnet economics.
- **Franchise model**: региональные операторы relay-нод.
- **iOS build** и App Store distribution.

### Этап 4: Cloud + Web
- **Серверная суммаризация** публичных каналов, доставка в свёрнутом виде.
- **Chrome MCP / Browser extension** для обработки веб-страниц.
- **Attention-weighted ranking**: каналы по глубине вовлечения.

### Дальняя перспектива
- **Onion routing**: послойное шифрование (DH + AES-256-GCM).
- **Обфускация трафика**: VLESS/XTLS поверх P2P потоков.
- **GPU ускорение**: CUDA/Metal через llama.cpp backend.
- **ZKP Proof-of-Transfer**: криптографические доказательства доставки.
- **Smart contract settlement**: автоматизированные расчёты на L2.
- **Federated Learning**: обмен LoRA-адаптерами между узлами.
- **Mesh-сети**: Bluetooth LE, Wi-Fi Direct, LoRa.
