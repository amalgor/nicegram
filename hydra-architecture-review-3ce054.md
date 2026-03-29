# Hydra: Архитектурный обзор и план развития

Комплексный обзор текущей реализации Hydra с оценкой архитектуры, списком проблем, предложениями по улучшению, и интеграцией нового слоя — **Content Intelligence** (суммаризация, TLDR-фолдинг, факт-чекинг, attention-аналитика).

---

## Непрерывность контекста (обязательно к прочтению после сжатия истории)

**Назначение:** этот файл — основной слой памяти репозитория. Чат и сессии IDE сжимаются; решения и текущее состояние фиксируются здесь и в `SERVICE_MANUAL.md`.

**Каноническая копия плана:** корень репозитория (`hydra-architecture-review-3ce054.md`). Копия в `~/.windsurf/plans/` при расхождении считается устаревшей — перенесите изменения из репозитория.

**Снимок на 2026-03-29 (обновлён после MVP Sprint)**

| Этап плана (часть 5) | Статус | Где смотреть в коде |
|----------------------|--------|---------------------|
| **Этап 0** — стабилизация | Выполнен | `hydra-config/`, `hydra-p2p/src/lib.rs`, `hydra-ai/src/lib.rs`, `hydra-core/src/main.rs` + `hydra.toml` |
| **Этап 1** — Content Intelligence (Telegram) | Выполнен | `hydra-content/`, Flutter: `screens/content_screen.dart`, `widgets/message_card.dart` |
| **Sprint: Network + Crypto + Content** | Выполнен | `hydra-relay-worker/`, `hydra-core/src/relay.rs`, `hydra-econ/src/circle.rs`, `STRATEGY.md` |
| **Sprint: Mobile MVP Readiness** | Выполнен | См. ниже |
| **Сборка Android** | Проходит | `hydra_mobile/`: VpnService + `tun2proxy`, bundled `qwen2.5-0.5b.gguf` |

**Mobile MVP Readiness Sprint (завершён 2026-03-29):**
- **64 unit + integration теста**: `hydra-core/src/socks.rs` (SOCKS5 parsing), `hydra-core/src/relay.rs` (relay), `hydra-core/src/connections.rs` (registry), `hydra-ai/src/lib.rs` (routing), `hydra-config/src/lib.rs` (config), `hydra_mobile/rust/src/api/quota.rs` (quota), `hydra-core/tests/integration.rs` (end-to-end SOCKS5, domain connect, auth rejection, registry tracking, AI routing)
- **Connection tracking**: `hydra-core/src/connections.rs` — `ConnectionRegistry` с route type, bytes, Telegram detection, AI reasoning, per-connection proxy override
- **Selective Telegram routing**: Telegram DC IPs → relay, остальное → direct. Настраивается в `[relay].mode`
- **VPN port bug fix**: `vpn.rs` → `SOCKS5_PORT` AtomicU16 из конфига
- **Flutter UI restructure**: `main.dart` разделён на `screens/` + `widgets/`. Новый Connections screen, folding message cards, connection summary на Connect screen
- **Config robustness**: `#[serde(default)]` на всех config structs — partial TOML работает

**Соответствие пунктам аудита §1.2 (кратко):**

- **[P1] Утечка `Box::leak` в P2P** — исправлено: протокол туннеля как `StreamProtocol` (`/hydra/tunnel/1.0.0`), без утечки на каждый `open_stream`.
- **[P7] Промпт vs JSON маршрутизации** — исправлено: промпт явно требует `path`, `transport`, `max_price` в одном формате с `RoutingInstruction`.
- **[P8] Путь к модели в core** — исправлено: путь из `HydraConfig` / `hydra.toml`, дефолты в `hydra-config`.
- **[P5] `set_active_model` пустой** — исправлено: перезагрузка весов через `AiNegotiator::load_model` при поднятом узле (`SHARED_AI`).
- **[P6] Два tokio runtime (VPN)** — осознанный компромисс: в `hydra_mobile/rust/src/api/vpn.rs` отдельный поток и runtime для долгого цикла `tun2proxy` (TUN FD); это не замена глобального runtime FRB, а изоляция блокирующего/долгоживущего цикла. Унификация с одним runtime — отдельная задача, если появятся регрессии или метрики.
- **[P4] econ settlement** — реализовано: `circle.rs` с Circle API (wallet set, wallets, transfer, balance check, tx polling). Testnet Arbitrum Sepolia.
- **Остаётся открытым:** [P2] onion без послойного шифрования, [P3] SOCKS5 без auth, graceful shutdown, миграция `sled`→`redb`/sqlite, тесты — см. §1.2–1.3 как список направлений.

**Sprint Network + Crypto + Content (2026-03-29) — ВЫПОЛНЕН:**
- Cloudflare Worker WSS relay (`hydra-relay-worker/`): WSS-to-TCP proxy с KV-квотами, whitelist Telegram DC.
- Rust WSS relay client (`hydra-core/src/relay.rs`): `tokio-tungstenite`, интеграция в SOCKS5 handler (auto/always/never).
- Circle USDC settlement (`hydra-econ/src/circle.rs`): developer-controlled wallets, Arbitrum Sepolia testnet.
- Gossipsub relay endpoint sharing: топик `hydra/relay-endpoints/1.0` в `hydra-p2p`.
- Конфигурация: `[relay]` и `[crypto]` секции в `hydra-config`.
- Quota system: CF Worker KV + Rust `quota.rs` + Flutter circular progress widget.
- Settings screen (5-я вкладка): proxy mode radio, relay endpoints, crypto toggle.
- STRATEGY.md: продуктовое позиционирование, экономика, целевые рынки, конкурентный анализ.
- Android APK: успешная сборка с новыми компонентами.

**Cloudflare deployment (2026-03-29):**
- Worker deployed: `https://relay.hydra-net.work` (custom domain, primary) + `https://hydra-relay.hydra-net.workers.dev` (workers.dev, fallback)
- Custom domain: `relay.hydra-net.work` — собственный домен `hydra-net.work`, зарегистрирован через Cloudflare Registrar, zone active. SSL: Let's Encrypt wildcard `*.hydra-net.work` (auto-provisioned via Custom Domains).
- KV namespace: `HYDRA_QUOTAS` (id: `82ed22205d834b94b87f42502823a59e`)
- Health check: `/health` -> `ok`
- Quota API: `/quota?device_id=...` -> `{"remaining": N, "limit": M}`
- WebSocket relay: WSS upgrade with `X-Hydra-Target: host:port` and `X-Hydra-Device: device-id`
- Account: `8e418b62669470a077532442d2cf76e1` (Alex@alder.ru)
- Subdomain: `hydra-net.workers.dev`

**Следующая логическая работа:** real-device тест Android -> CF Worker -> Telegram DC, этап 2 плана — Attention tracking + персонализация.

**Мобильный Content (фактическая зрелость, 2026-03-29):**
- **Суммаризация в приложении:** логика есть в Rust (`hydra-content` → `Summarizer` + `MessageHandler`: для текста >200 символов вызывается LLM или fallback). На устройстве она **не доходит до UI**: не вызывается `listen_for_updates` (никто не забирает `take_updates_receiver` и не запускает цикл), нет экспорта в FRB для `fetch_and_process` / готовых `ProcessedMessage`. Пользователь видит только JSON-список диалогов.
- **Экран каналов со сворачиваемыми карточками:** **нет** — `ContentScreen` рисует плоский `ListView` из `TrackedChat`; дерево `ContentNode` нигде не отображается и `ExpansionTile`/уровни фолдинга не используются.
- **Баг «выход за границы диапазона» при загрузке диалогов:** при пустом `title` у части чатов/каналов в Flutter выполнялось `(title)[0]` → `RangeError`. Исправлено: безопасная буква в аватаре, подпись `Chat {id}`; в Rust для пустого имени подставляется `Chat {chat_id}`.

---

## Часть 1. Оценка текущей архитектуры

*Ниже — материалы архитектурного аудита. Актуальный статус реализации пунктов [P1]–[P8] и этапов 0–1 см. в разделе «Непрерывность контекста» выше.*

### 1.1. Общая структура (что хорошо)

- **Чистое разделение на крейты**: `hydra-core`, `hydra-p2p`, `hydra-ai`, `hydra-econ`, `hydra_mobile/rust` — каждый с ясной зоной ответственности.
- **Правильный стек**: Rust + tokio + libp2p + candle — production-grade выбор для мобильного P2P-агента.
- **Flutter + flutter_rust_bridge** — разумный выбор для кросс-платформенного мобильного UI с нативным Rust-ядром.
- **tun2proxy** для перехвата всего трафика через VPN-интерфейс Android — рабочий подход.
- **Diagnostics protocol** (Co-Agent) — оригинальная идея опроса соседей при сбое.

### 1.2. Критические проблемы

#### [P1] `hydra-p2p/src/lib.rs:341` — утечка памяти через `Box::leak`
```rust
let protocol_static: &'static str = Box::leak(protocol.into_boxed_str());
```
Каждый вызов `open_stream` утекает строку навсегда. При активном трафике — неограниченный рост памяти. **Решение**: использовать `StreamProtocol` с `Cow<'static, str>` или статическую константу протокола.

#### [P2] Onion routing без шифрования
`hydra-core/src/onion.rs` — данные между хопами передаются **в открытом виде**. Промежуточный узел видит адрес конечной цели и весь трафик. Это НЕ onion routing, а multi-hop relay. **Решение**: послойное шифрование (ключ каждого хопа), как в Tor — каждый узел снимает свой слой.

#### [P3] SOCKS5 без аутентификации
`hydra-core/src/lib.rs:74` — только `NO AUTHENTICATION` (0x00). Любой процесс на устройстве может использовать прокси. **Решение**: добавить как минимум username/password auth для контроля доступа.

#### [P4] `hydra-econ` — `settle()` — симуляция без реальной логики
`hydra-econ/src/lib.rs:76-106` — settlement просто обнуляет долг. `verify_proof_of_transfer()` тоже не содержит реального proof. Эти функции вызываются из core так, как будто они работают. **Решение**: пометить как не реализованные и не вызывать из production-путей, либо реализовать.

#### [P5] `hydra_mobile/rust/src/api/model_manager.rs:126` — TODO без реализации
```rust
// TODO: integrate with hydra-core / hydra-ai
```
`set_active_model` ничего не делает. Пользователь думает, что модель переключена, но на деле ничего не происходит.

#### [P6] `hydra_mobile/rust/src/api/vpn.rs` — создаёт новый tokio Runtime в std::thread
```rust
std::thread::spawn(move || {
    let rt = tokio::runtime::Runtime::new().unwrap();
```
И `start_vpn_tunnel`, и `stop_vpn_tunnel` делают это. Есть глобальный tokio runtime от flutter_rust_bridge — нужно использовать его. Два рантайма = двойное потребление ресурсов + проблемы с синхронизацией.

#### [P7] Промпт AI не соответствует ожидаемому формату ответа
`hydra-ai/src/lib.rs:86-94` — промпт просит `{"next_hop": ...}`, но парсер ожидает `RoutingInstruction` с полями `path`, `transport`, `max_price`. Модель никогда не вернёт корректный JSON для парсинга. **Решение**: привести промпт и структуру в соответствие.

#### [P8] `hydra-core/src/main.rs:39` — hardcoded путь модели не соответствует файлу на диске
```rust
let model_path = PathBuf::from("models/qwen2.5-0.5b.gguf");
```
На диске: `models/qwen2.5-1.5b-instruct-q4_k_m.gguf`. Модель никогда не загрузится.

### 1.3. Проблемы средней критичности

- **Нет graceful shutdown** — ни P2P node, ни SOCKS5 server не поддерживают корректное завершение. `CancellationToken` только для tun2proxy.
- **`sled` deprecated** — автор рекомендует переход на другие embedded DB. Для мобильного устройства лучше `redb` или `sqlite` (через `rusqlite`).
- **Hardcoded bootstrap node** `boot.ze1.org:33097` — должен быть в конфиге.
- **`hydra_mobile/rust/src/api/test_tun.rs`** — мёртвый код, не используется, не компилируется как тест. Удалить.
- **Нет конфигурационного файла** — порты, пути, адреса bootstrap-нод, параметры AI — всё hardcoded. Нарушает правило 10 (всё что меняется — в config).
- **`init_model_manager` принимает `base_dir` в Dart, но не в Rust** — сигнатура Rust не принимает `base_dir`, использует `ProjectDirs`. Dart-код передаёт аргумент, который игнорируется.
- **Нет тестов** — ни юнит, ни интеграционных.

### 1.4. Что выбросить

| Файл/компонент | Причина |
|---|---|
| `hydra_mobile/rust/src/api/test_tun.rs` | Мёртвый код, не тест |
| `hydra-econ::settle()` body | Симуляция, создаёт ложное впечатление работающего settlement |
| `hydra-econ::verify_proof_of_transfer()` body | Нет никакого proof — просто ±trust_score |
| `check_tun2proxy` (бинарник в корне) | Отладочный артефакт, 3.8MB |
| `SERVICE_MANUAL.pdf`, `USER_MANUAL.pdf` | Дубликаты .md файлов, не обновляются синхронно |
| `hydra_db/` в корне | Runtime-данные в репозитории |
| `models/` в репозитории | 1.1GB модель в git — должна скачиваться отдельно |

---

## Часть 2. Рекомендуемые улучшения существующего ядра

### 2.1. Конфигурационная система
Создать `hydra-config` крейт с TOML-конфигом:
```toml
# Сеть
[network]
socks5_port = 1080
p2p_listen_port = 0  # 0 = random
bootstrap_nodes = ["boot.ze1.org:33097"]

# AI модель
[ai]
model_path = "models/qwen2.5-0.5b.gguf"
tokenizer_path = "models/tokenizer.json"
max_generation_tokens = 128
cache_ttl_seconds = 300
cache_max_items = 1000

# Экономика
[econ]
db_path = "hydra_db"
settlement_threshold_bytes = 10000000
```

### 2.2. Исправить AI pipeline
- Привести промпт в соответствие с `RoutingInstruction`
- Добавить structured output validation
- Добавить temperature/top_p в конфиг inference
- Реализовать `set_active_model` — горячая замена модели

### 2.3. Безопасность onion routing
Минимальная реализация: Diffie-Hellman key exchange с каждым хопом, AES-256-GCM послойное шифрование.

### 2.4. Graceful shutdown
`tokio_util::sync::CancellationToken` для всех компонентов. Один токен на весь узел, дочерние токены для подсистем.

---

## Часть 3. Content Intelligence Layer (новый крейт `hydra-content`)

### 3.1. Архитектурная концепция

Два режима обработки контента:

**[LOCAL] Клиентская обработка (on-device)**
- Личные чаты, закрытые группы/каналы
- Данные никогда не покидают устройство
- Обработка локальной LLM (та же, что для маршрутизации, или отдельная, более мощная)

**[CLOUD] Централизованная обработка публичных каналов**
- Публичные каналы обрабатываются на серверной инфраструктуре
- Клиент получает уже свёрнутые (TLDR) версии
- Подгрузка полного контента по мере "разворачивания" пользователем
- Attention-трекинг: что развернул, сколько читал, до какого уровня дошёл

### 3.2. Telegram интеграция через `grammers`

```
[grammers-client] → MTProto → Telegram Servers
       ↓ (расшифрованные сообщения)
[hydra-content]
       ├── ContentClassifier  → определяет тип контента
       ├── Summarizer         → TLDR + иерархический фолдинг
       ├── FactChecker        → перекрёстная проверка фактов
       └── AttentionTracker   → трекинг внимания пользователя
```

- **grammers** — чистый Rust MTProto клиент (github.com/Lonami/grammers, 800+ stars, активно поддерживается)
- Работает как userbot/клиент: подписывается на каналы, получает сообщения
- Все данные расшифровываются на устройстве стандартным MTProto flow

### 3.3. Иерархический TLDR-фолдинг

Модель данных:
```
ContentNode {
    id: UUID,
    level: u8,           // 0 = headline, 1 = summary, 2 = key points, 3 = full text
    content: String,
    children: Vec<ContentNode>,
    is_expanded: bool,    // трекинг на клиенте
    read_duration_ms: u64, // сколько времени пользователь смотрел
}
```

Уровни раскрытия:
- **Level 0**: Заголовок + 1 предложение (всегда видно)
- **Level 1**: Суммаризация (3-5 предложений)
- **Level 2**: Ключевые тезисы + факты
- **Level 3**: Полный оригинальный текст

### 3.4. Attention Analytics — "Точка сборки"

Собираемая статистика (анонимизированная для CLOUD):
```
AttentionEvent {
    channel_id: u64,
    message_id: u64,
    max_depth_reached: u8,    // до какого уровня развернул
    total_read_time_ms: u64,  // общее время чтения
    interaction_type: enum { Skim, Read, DeepDive, Share },
}
```

**Премиумные возможности** (CLOUD):
- **Trend Heatmap** — какие темы/каналы сейчас привлекают глубокое внимание аудитории (не просмотры, а реальное чтение)
- **Attention-weighted ranking** — каналы, отсортированные по глубине вовлечения, а не по подписчикам
- **Cross-channel fact graph** — один факт, как он подаётся в разных каналах, с оценкой согласованности
- **Personalized digest** — дообучение на интересах пользователя (LoRA-адаптер или prompt-tuning)

### 3.5. Веб-контент (не через прокси)

Как установлено — HTTPS трафик в прокси не читается. Альтернативные подходы:

1. **Chrome MCP (Model Context Protocol)** — Chrome DevTools Protocol + Lighthouse для получения рендеренного контента страницы. Hydra-агент подключается к браузеру как MCP-сервер.
2. **Browser extension** — расширение отправляет текстовый контент страницы локальному агенту для обработки.
3. **Content API cooperation** — по согласованию с авторами: сервер/CDN отдаёт структурированный контент напрямую агенту.

---

## Часть 4. Новая структура крейтов

```
hydra/
├── hydra-config/     # [NEW] Конфигурация всего проекта
├── hydra-core/       # Ядро: SOCKS5, координатор, onion routing
├── hydra-p2p/        # P2P сеть (libp2p)
├── hydra-ai/         # LLM inference (candle)
├── hydra-econ/       # Экономика/репутация
├── hydra-content/    # [NEW] Content Intelligence
│   ├── src/
│   │   ├── lib.rs
│   │   ├── telegram/       # grammers интеграция
│   │   │   ├── client.rs   # Telegram клиент
│   │   │   └── handler.rs  # обработчик сообщений
│   │   ├── processing/     # обработка контента
│   │   │   ├── summarizer.rs
│   │   │   ├── fact_checker.rs
│   │   │   ├── classifier.rs
│   │   │   └── folder.rs   # TLDR фолдинг
│   │   ├── attention/      # трекинг внимания
│   │   │   ├── tracker.rs
│   │   │   └── analytics.rs
│   │   └── models.rs       # общие модели данных
│   └── Cargo.toml
├── hydra-cloud/      # [FUTURE] Серверная часть для публичных каналов
├── hydra_mobile/     # Flutter + Rust bridge
└── Cargo.toml
```

---

## Часть 5. План реализации (этапы)

### Этап 0: Стабилизация (fix critical bugs)
1. Исправить `Box::leak` утечку памяти в `hydra-p2p`
2. Исправить несоответствие промпта и `RoutingInstruction` в `hydra-ai`
3. Исправить hardcoded путь модели в `hydra-core/src/main.rs`
4. Удалить мёртвый код (`test_tun.rs`, бинарники, PDF-дубликаты)
5. Создать `hydra-config` крейт, вынести все hardcoded значения
6. Реализовать `set_active_model` до конца
7. Исправить двойной tokio runtime в VPN

### Этап 1: Content Intelligence — Telegram клиент (grammers)
1. Добавить `hydra-content` крейт
2. Интеграция `grammers-client` — авторизация, подписка на каналы
3. Получение сообщений в реальном времени
4. Базовая суммаризация через локальную LLM
5. Модель данных ContentNode с иерархическим фолдингом
6. Flutter UI: экран каналов со сворачиваемыми карточками

### Этап 2: Attention tracking + персонализация
1. AttentionTracker — клиент-сайд трекинг взаимодействий
2. Локальное хранение (SQLite) статистики чтения
3. Дообучение промптов по интересам пользователя
4. LoRA-адаптер для персонализированной суммаризации

### Этап 3: Cloud обработка публичных каналов
1. Серверная инфраструктура для обработки публичных каналов
2. API для доставки TLDR-контента на клиент
3. Attention analytics aggregation (анонимизированная)
4. Премиум API: тренды, heatmap, fact-graph

### Этап 4: Веб-контент
1. Chrome MCP интеграция / Browser extension
2. Обработка веб-страниц тем же pipeline (summarizer → folder → tracker)

---

## Часть 6. Ключевые технические решения

| Решение | Выбор | Обоснование |
|---|---|---|
| Telegram клиент | `grammers` (Rust) | Чистый Rust, нет C/C++ зависимостей, MTProto 2.0, активно поддерживается |
| Local DB для attention | `rusqlite` | Зрелая, маленькая, SQL для аналитических запросов |
| Peer reputation DB | `redb` (замена sled) | Современная embedded KV, не deprecated |
| Конфигурация | `toml` + `config` crate | Стандарт Rust-экосистемы |
| LLM для суммаризации | Та же candle + GGUF модель | Переиспользуем инфраструктуру, разные промпты |
| Cloud API (future) | gRPC + tonic | Rust-native, streaming, эффективно для мобильных клиентов |
