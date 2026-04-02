# Техническая документация и архитектура (Service Manual)

**Непрерывность между сессиями:** дорожная карта этапов, статус P1–P8 и решения по компромиссам зафиксированы в [`hydra-architecture-review-3ce054.md`](hydra-architecture-review-3ce054.md) (корень репозитория). После сжатия контекста чата начинайте с того файла, затем с этого Service Manual.

## Обзор архитектуры
Hydra — это мульти-агентная P2P сеть, предназначенная для интеллектуальной маршрутизации трафика, обхода сетевых ограничений (DPI/цензуры) и проактивной обработки контента (суммаризация, TLDR-фолдинг, факт-чекинг). Главная инновация — локальная Small Language Model (SLM) на каждом узле для динамического принятия решений и обработки контента.

Проект написан на **Rust** и разделен на следующие крейты:
1. **hydra-config** — Централизованная конфигурация всего проекта (TOML). Все настраиваемые параметры сети, AI, экономики, Telegram и content в одном месте.
2. **hydra-core** — Ядро приложения. SOCKS5-сервер, координатор компонентов, multi-hop relay.
3. **hydra-p2p** — Сетевой слой на базе `libp2p`. Управляет соединениями, телеметрией и протоколами связи между узлами.
4. **hydra-ai** — Модуль искусственного интеллекта. Инференс локальной LLM (llama.cpp через llama-cpp-2 + GGUF) для маршрутизации и обработки контента.
5. **hydra-content** — Content Intelligence. Telegram-клиент (grammers MTProto), TLDR-фолдинг, суммаризация через LLM, attention tracking (хранилище по конфигу `[content]`).
6. **hydra-econ** — Локальный экономический слой и репутационная система (на базе `sled`) для trust/debt без ончейн settlement.
7. **hydra-exchange** — Base Sepolia HRX client. Локальный EOA wallet (BIP-39), `alloy` bindings для `HydraRouteBook`, ERC-8004 identity/reputation и чтения USDC balance.
8. **hydra_mobile/rust** — Мост Flutter-Rust (flutter_rust_bridge). VPN-интерфейс (tun2proxy), менеджер моделей, телеметрия, API Content и FRB API Marketplace для мобильного UI.

## Конфигурация
Все параметры вынесены в `hydra.toml` (TOML-файл в рабочей директории). При отсутствии файла используются значения по умолчанию. Пример конфигурации: `hydra.toml.example`.

Основные секции:
- **[network]** — `socks5_port`, `p2p_listen_port`, `bootstrap_nodes`
- **[ai]** — `model_path`, `max_generation_tokens`, `cache_ttl_seconds`, `cache_max_items`
- **[econ]** — `db_path`, `settlement_threshold_bytes`
- **[telegram]** — `api_id`, `api_hash`, `session_path`
- **[content]** — `db_path`, `summarization_max_tokens`, `cache_ttl_seconds`
- **[[transports]]** — transport list в порядке failover/приоритета: `type = "wss" | "vless"`, `mode = "telegram" | "all"`, transport-specific поля (`endpoints`, `device_id`, `url`)
- **[crypto]** — `enabled`, `chain`, `rpc_url`, `route_book_address`, `identity_registry_address`, `reputation_registry_address`, `usdc_address`
- **[bootstrap]** — `listen_port` (для bootstrap-нод)

На мобильном устройстве конфигурация загружается из `{app_documents_dir}/hydra.toml`, относительные пути автоматически разрешаются относительно `app_documents_dir`.

## Применяемые технологии и библиотеки
- **Сеть**: `libp2p` (TCP, Noise, Yamux, Kademlia DHT, Gossipsub, mDNS).
- **Gossipsub**: Топик `hydra/relay-endpoints/1.0` для обмена WSS relay endpoints между узлами.
- **Асинхронность**: `tokio` (полный асинхронный рантайм).
- **WSS Relay**: `tokio-tungstenite` (клиент), Cloudflare Workers (сервер).
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
- **Operational quirk**: live ERC-8004 proxy calls `register()` / `ownerOf()` return spurious `NotActivated` errors under Foundry script simulation. Actual on-chain `cast call/send` works. Current acceptance seed was completed with direct `cast send` instead of `seed-base-sepolia.sh`.
- **Bindings**: `HydraRouteBook`, official ERC-8004 ABI JSON for `IdentityRegistry.register()` / `ReputationRegistry.giveFeedback(...)`, ERC-20 `balanceOf`.
- **Read path**: `query_offers(region, protocol)` нормализует offers в мобильную view model и при наличии registry подмешивает reputation summary.
- **Write path**: agent registration и manual feedback submission подписываются локально через mnemonic, передаваемый из Flutter только на время операции.

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
- **WSS-to-TCP relay**: Принимает WebSocket-соединения с заголовком `X-Hydra-Target: host:port`, открывает TCP-соединение к цели через `connect()` API.
- **Quota tracking**: KV namespace `HYDRA_QUOTAS` для учёта трафика по device_id с дневным лимитом (50 MB free tier).
- **Безопасность**: Whitelist Telegram DC IP-диапазонов (149.154.*, 91.108.*).
- **Endpoints**: `/health` (healthcheck), `/quota?device_id=...` (проверка квоты), WebSocket upgrade (relay).

### 7. Мобильный слой (hydra_mobile)
- **Flutter UI**: 7 вкладок — Connect, Network, Marketplace, AI, Content, Logs, Settings.
- **UI Architecture**: Добавлены `screens/marketplace_screen.dart` и `exchange/` слой (`HydraExchangeRepository`, backend, models, secure mnemonic store).
- **Rust bridge**: `flutter_rust_bridge` используется и для network/runtime API, и для Marketplace API: `get_marketplace_config_status()`, `create_wallet()`, `import_wallet()`, `get_wallet_balances()`, `list_route_offers()`, `register_agent()`, `submit_feedback()`.
- **VPN**: Android VpnService → TUN FD → `tun2proxy` → локальный SOCKS5 → hydra-core. **Исправлен баг**: `vpn.rs` теперь читает `socks5_port` из загруженной конфигурации через `SOCKS5_PORT` AtomicU16, а не из `NetworkConfig::default()`.
- **Connection Tracking** (`hydra-core/src/connections.rs`): `ConnectionRegistry` — thread-safe реестр всех соединений с tracking bytes, route type, Telegram detection, AI reasoning, force-proxy override.
- **Selective Routing**: Telegram DC трафик (149.154.0.0/16, 91.108.0.0/16) автоматически маршрутизируется через relay. Остальной трафик — direct. Пользователь может переключить per-connection.
- **Content Intelligence**: Tap on dialog → `fetch_channel_messages()` → `MessageHandler::fetch_and_process()` → `Summarizer::process()` → `FoldableMessageCard` с 4 уровнями (Headline/Summary/KeyPoints/FullText). Attention tracking при expand/collapse.
- **Model Manager**: Скачивание моделей с HuggingFace, горячая замена через `SHARED_AI`.
- **Quota Manager** (`quota.rs`): Локальный трекинг потреблённого трафика с периодической синхронизацией с CF Worker KV.
- **Marketplace**: локальный wallet onboarding, balances (ETH + USDC), ERC-8004 agent registration, filters `region/protocol` с `SharedPreferences`, offers list и manual feedback.
- **Marketplace runtime states**: экран явно различает `disabled`, `incomplete config`, `no offers` и `load failed`; пустой `route_book_address` в `hydra.toml` больше не вываливает сырой backend exception в UI.
- **Settings**: Proxy mode и краткая on-chain status card. Legacy settlement UI удалён.

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
