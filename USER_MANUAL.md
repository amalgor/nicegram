# Руководство пользователя (Android MVP)

## Что это сейчас
Hydra в текущем shipping состоянии — это **Android network utility** для маршрутизации трафика через:
- встроенный `Cloudflare WSS relay`
- пользовательские `VLESS` credentials, которые вы получили самостоятельно и импортировали в приложение

В APK **не встроена LLM-модель**. Локальный AI остаётся опциональным: модель можно скачать позже из `Settings -> Optional AI`.

## Что нужно
- Android-устройство с поддержкой `VpnService`
- доступ в интернет
- ваши собственные `vless://...` credentials или V2Ray base64 subscription, если хотите использовать VLESS

## Установка APK
Собрать release APK из репозитория:

```bash
cd hydra_mobile
flutter build apk --release
```

Готовый артефакт появится в:

```text
hydra_mobile/build/app/outputs/flutter-apk/app-release.apk
```

## Первый запуск
При первом старте приложение:
1. materialize-ит `hydra.toml` в app documents dir
2. seed-ит built-in профиль `Hydra WSS Relay`
3. поднимает локальный runtime без загрузки AI-модели

Дополнительные runtime-файлы в documents dir:
- `mobile_routes.json` — маршруты и их порядок
- `route_policies.json` — сохранённые правила маршрутизации
- `relay_usage.json` — локальная история WSS relay usage

## Основные экраны

### Connect
- включает и выключает Android VPN
- показывает active/proxied connections, uplink/downlink, краткую сводку relay usage

### Connections
- показывает активные соединения, сгруппированные по приложению, если owner app удалось определить
- если app owner не определён, используется domain group fallback
- для каждой группы можно сохранить policy:
  - `Auto`
  - `Direct`
  - `Block`
  - explicit `WSS`
  - explicit `VLESS`

### Routes
- показывает built-in WSS relay и импортированные VLESS profiles
- поддерживает:
  - import raw `vless://...`
  - import V2Ray base64 subscription
  - enable/disable
  - rename
  - reorder priority
  - mode `All traffic` / `Telegram only`

### Relay Usage
- считает только **WSS relay traffic**
- показывает today / 7d / 30d usage
- считает **estimated cost** по локально заданному `USD/GB` rate
- даёт `Support relay` link/QR для внешних донатов

### Settings
- global proxy mode: `Off`, `Telegram Only`, `Full VPN`
- estimated relay cost rate
- `Optional AI` для ручной загрузки модели

## Как импортировать VLESS

### Вариант 1: raw URI
В `Routes` нажмите `Paste VLESS` и вставьте один или несколько `vless://...` URI, по одному на строку.

Поддерживаются оба распространённых варианта:
- обычный `VLESS` без `flow`
- `Reality/Vision` URI с `flow=xtls-rprx-vision`

### Вариант 2: V2Ray base64 subscription
В `Routes` нажмите `Import Subscription` и вставьте base64 payload. Приложение:
- декодирует подписку
- разбивает её на строки
- импортирует только `vless://...`
- пропускает неподдерживаемые записи

## Как задать маршрут для приложения или домена
1. Откройте `Connections`
2. Найдите нужную group card
3. Нажмите кнопку маршрута
4. Выберите `Auto`, `Direct`, `Block`, конкретный `WSS` или конкретный `VLESS`

Выбор сохраняется в `route_policies.json` и применяется runtime-ом на следующем цикле refresh.

## Что считается в Relay Usage
В `relay_usage.json` пишется только трафик, который реально прошёл через `WSS` transport.

Не попадает в оценку:
- direct traffic
- imported `VLESS` traffic

Это сделано специально, чтобы Cloudflare estimate отражал только relay leg.

## Устранение неполадок
- `Import failed`: проверьте, что raw input начинается с `vless://`, а subscription действительно base64 и после декодирования содержит `vless://` строки.
- `VLESS подключается, но трафика нет`: обновите приложение до сборки не раньше `2026-04-05`. В более раннем APK обычные VLESS URI без `flow` могли ошибочно отправляться как `xtls-rprx-vision`, из-за чего сервер закрывал поток сразу после SOCKS5 connect.
- `VPN не стартует`: проверьте системное разрешение Android VPN и повторите запуск с `Connect`.
- `Relay usage не растёт`: этот экран считает только WSS. Если трафик идёт direct или через imported VLESS, счётчик не изменится.
- `Нет AI анализа`: это ожидаемо, пока вы не скачали модель вручную из `Settings -> Optional AI`.
- `Нет app grouping`: best-effort app attribution пока не универсальна; в таких случаях приложение покажет domain group fallback.

## Что пока не входит в shipping APK
- Cloudflare balance API integration
- in-app donation reconciliation
- встроенный marketplace / payments / providers UI
- bundled GGUF model
