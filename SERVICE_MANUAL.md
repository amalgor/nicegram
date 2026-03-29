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
6. **hydra-econ** — Экономический слой и репутационная система (на базе `sled`).
7. **hydra_mobile/rust** — Мост Flutter-Rust (flutter_rust_bridge). VPN-интерфейс (tun2proxy), менеджер моделей, телеметрия и API Content для мобильного UI.

## Конфигурация
Все параметры вынесены в `hydra.toml` (TOML-файл в рабочей директории). При отсутствии файла используются значения по умолчанию. Пример конфигурации: `hydra.toml.example`.

Основные секции:
- **[network]** — `socks5_port`, `p2p_listen_port`, `bootstrap_nodes`
- **[ai]** — `model_path`, `max_generation_tokens`, `cache_ttl_seconds`, `cache_max_items`
- **[econ]** — `db_path`, `settlement_threshold_bytes`
- **[telegram]** — `api_id`, `api_hash`, `session_path`
- **[content]** — `db_path`, `summarization_max_tokens`, `cache_ttl_seconds`
- **[relay]** — `endpoints` (WSS relay URLs), `mode` (auto/always/never), `device_id`
- **[crypto]** — `enabled`, `circle_api_key`, `settlement_chain`, `wallet_set_id`, `entity_secret`
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
- **Крипто-расчёты**: Circle API (REST) через `reqwest`, USDC на Arbitrum Sepolia.
- **Форматы данных**: JSON (промпты AI, API), CBOR (бинарные сетевые протоколы).
- **Мобильный мост**: `flutter_rust_bridge` 2.11.1.
- **VPN перехват**: `tun2proxy` (SOCKS5 bridge через TUN-интерфейс Android).
- **Flutter UI**: Material 3, `shared_preferences` для настроек.

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
- **USDC Settlement** (`circle.rs`): Интеграция с Circle developer-controlled wallets для расчётов в USDC. Поддержка Arbitrum Sepolia (testnet). API: создание wallet set, создание кошельков, проверка баланса, перевод USDC, ожидание подтверждения транзакции. Конфигурация через `[crypto]` секцию.
- **Proof of Transfer**: Каркас для верификации доставки данных.

### 5. Ядро (hydra-core)
- **SOCKS5 Server**: Принимает соединения от локальных приложений, порт из `[network].socks5_port`.
- **Multi-hop Relay**: Последовательно устанавливает stream-каналы через промежуточные узлы.
- **WSS Relay** (`relay.rs`): Клиент для Cloudflare Worker WSS relay. При недоступности прямого соединения или P2P пиров, трафик маршрутизируется через WSS-туннель к Cloudflare Worker, который проксирует TCP к целевому серверу. Режимы: `auto` (fallback), `always` (принудительно), `never` (отключено). Конфигурация через `[relay]` секцию.

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
- **Flutter UI**: 5 вкладок — Connect (VPN toggle + quota + connection summary), Connections (live connection list с per-connection proxy toggle), AI Models (скачивание/выбор), Content (Telegram auth + folding message cards), Settings (proxy mode, relay endpoints, crypto).
- **UI Architecture**: Разделён на отдельные файлы: `screens/connect_screen.dart`, `screens/connections_screen.dart`, `screens/models_screen.dart`, `screens/content_screen.dart`, `screens/settings_screen.dart`, `widgets/quota_widget.dart`, `widgets/connection_tile.dart`, `widgets/message_card.dart`.
- **Rust bridge**: `flutter_rust_bridge` для синхронных и асинхронных вызовов. Новые API: `get_active_connections()`, `get_connection_stats()`, `set_connection_proxy()`, `fetch_channel_messages()`.
- **VPN**: Android VpnService → TUN FD → `tun2proxy` → локальный SOCKS5 → hydra-core. **Исправлен баг**: `vpn.rs` теперь читает `socks5_port` из загруженной конфигурации через `SOCKS5_PORT` AtomicU16, а не из `NetworkConfig::default()`.
- **Connection Tracking** (`hydra-core/src/connections.rs`): `ConnectionRegistry` — thread-safe реестр всех соединений с tracking bytes, route type, Telegram detection, AI reasoning, force-proxy override.
- **Selective Routing**: Telegram DC трафик (149.154.0.0/16, 91.108.0.0/16) автоматически маршрутизируется через relay. Остальной трафик — direct. Пользователь может переключить per-connection.
- **Content Intelligence**: Tap on dialog → `fetch_channel_messages()` → `MessageHandler::fetch_and_process()` → `Summarizer::process()` → `FoldableMessageCard` с 4 уровнями (Headline/Summary/KeyPoints/FullText). Attention tracking при expand/collapse.
- **Model Manager**: Скачивание моделей с HuggingFace, горячая замена через `SHARED_AI`.
- **Quota Manager** (`quota.rs`): Локальный трекинг потреблённого трафика с периодической синхронизацией с CF Worker KV.
- **Settings**: Proxy mode (Off/Telegram Only/Full VPN), список relay endpoints, статус USDC кошелька. Убран нефункциональный "Selected Apps".

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
- **Circle USDC settlement** (`hydra-econ/src/circle.rs`): developer-controlled wallets, Arbitrum Sepolia testnet.
- **Gossipsub relay sharing**: топик `hydra/relay-endpoints/1.0` в `hydra-p2p`, publish/subscribe.
- **Quota system**: CF Worker KV tracking + Rust `quota.rs` + Flutter circular progress widget.
- **Settings screen**: proxy mode (Off/Telegram/Selected/Full VPN), relay endpoints, crypto toggle.
- **STRATEGY.md**: продуктовое позиционирование, экономика, целевые рынки.
- **Android APK**: успешная сборка с новыми компонентами.

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

### Этап 2: Attention + персонализация
- **AttentionTracker** — полнота клиентского трекинга и политика событий.
- **Локальное хранение** — SQLite для статистики чтения.
- **Дообучение промптов** по интересам пользователя и опционально **LoRA** для суммаризации.

### Доработки Content (параллельно этапу 2)
- Углубление **иерархического TLDR-фолдинга** (4 уровня) и полировка UX карточек каналов.
- **Локальная обработка** приватных чатов: явные гарантии и настройки приватности в UI.

### Этап 3: Production deployment
- **Custom domain** для CF Worker relay — ВЫПОЛНЕНО: `relay.hydra-net.work` (домен `hydra-net.work`).
- **Production USDC** (mainnet, реальные деньги).
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
