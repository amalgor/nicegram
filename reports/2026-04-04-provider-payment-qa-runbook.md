# Hydra Provider & Payments QA Runbook

**Date:** 2026-04-04  
**Scope:** embedded-wallet mobile app manual QA for provider and payment flows on one device

## Goal
Проверить все ручные пользовательские пути без CLI и без внешнего web dashboard:
- consumer receive/top-up path
- buyer escrow path
- dealer publish/manage path
- route provider Share & Earn path
- multi-profile switching on одном устройстве

## Prerequisites
- Android device with current release APK
- Base Sepolia ETH on каждом тестовом profile
- Base Sepolia USDC хотя бы на dealer/provider profile
- Live contracts already configured in `hydra.toml`
- Live worker `relay.hydra-net.work` доступен

## Profiles
Рекомендуемый минимум:
- `Buyer`
- `Dealer`
- `Provider`

Все три профиля можно создать или импортировать на одном устройстве через `Balance -> header -> Profiles`.

## Core Checks
### 1. Profile migration and switching
1. Открыть `Balance`.
2. Проверить, что legacy wallet migrated to a profile or that empty state offers create/import.
3. Создать минимум два профиля и переключиться между ними.
4. Проверить, что address в header меняется и `Receive` sheet показывает correct QR/copy/share value.

### 2. Payments > Buy
1. Переключиться на buyer profile.
2. Открыть `Balance -> Payments -> Buy`.
3. Проверить, что live deal offers загружаются.
4. Выбрать offer, указать amount в пределах min/max.
5. Убедиться, что открывается escrow detail sheet с:
   - timeline/status
   - expiry
   - dealer contact
   - instructions by payment method
   - copy buttons
6. Нажать `Mark fiat sent` и проверить обновление escrow state.

### 3. Payments > Sell
1. Переключиться на dealer profile.
2. Открыть `Balance -> Payments -> Sell`.
3. Проверить readiness card:
   - wallet address
   - ETH balance
   - USDC balance
   - agent registration
   - DealBoard allowance
   - payment profile completeness
4. Если allowance = 0, выполнить `Approve`.
5. Заполнить dealer payment profile и сохранить.
6. Создать deal offer.
7. Проверить, что offer появляется в `My Deals`.

### 4. Payments > Escrows
1. Для buyer profile проверить `Outgoing`.
2. Для dealer profile проверить `Incoming`.
3. На dealer side выполнить `Confirm receipt` или `Reject`.
4. Для expired escrow проверить `Claim expired`, если applicable.

### 5. Providers > Share & Earn
1. Переключиться на provider profile.
2. Открыть `Balance -> Providers -> Share & Earn`.
3. Включить toggle.
4. Проверить:
   - active/enabled state
   - last error
   - last announcement time
   - relay-backed endpoint
   - copy/share/QR actions
5. Открыть settings sheet и проверить manual knobs.

### 6. Providers > Route Offers
1. Проверить lifecycle card.
2. Если on-chain offer ещё не опубликован, выполнить `Publish route offer`.
3. Проверить `offerId`, `agentId`, `tx hash`.
4. Выполнить `Deactivate`.
5. После expiry delay выполнить `Withdraw`.

### 7. Providers > Metrics
1. Проверить local metrics rendering:
   - session count
   - bytes relayed
   - average latency
   - throughput
   - uptime ratio
   - recent failures
   - local routing score
2. При наличии pending reputation delta выполнить `Sync reputation now`.

### 8. Advanced
1. В `Overview` включить `Show advanced tools`.
2. Перейти в `Advanced`.
3. Проверить, что raw Marketplace surface открывается без crash.
4. Проверить profile recovery phrase reveal flow.

## Expected Failure Messages
- no wallet: instructs to create/import a wallet profile
- no ETH for gas: suggests funding Base Sepolia ETH
- missing allowance: suggests DealBoard approve
- missing dealer profile: asks to fill dealer payment profile
- withdrawal not ready: explains delay window
- sharing active under another profile: points to active profile id

## Artifact Paths
- Release APK: `hydra_mobile/build/app/outputs/flutter-apk/app-release.apk`
- Worker ops baseline: `reports/2026-04-03-ops-validation.md`
- Architecture memory: `SERVICE_MANUAL.md`, `hydra-architecture-review-3ce054.md`
