# Operations Validation — 2026-04-03

## Scope
- Fix `hydra-relay-worker/wrangler.toml` Durable Object migration type
- Deploy Cloudflare Worker with provider-session Durable Object
- Regenerate Flutter Rust Bridge bindings for deal API
- Re-run Flutter validation
- Build Android release APK
- Verify live `HydraDealBoard`
- Seed one live test deal offer on Base Sepolia

## Cloudflare Worker

### Config change
- File: `hydra-relay-worker/wrangler.toml`
- Change: `new_sqlite_classes = ["HydraProviderSession"]` -> `new_classes = ["HydraProviderSession"]`

### Auth status
`wrangler whoami`

```text
You are logged in with an OAuth Token, associated with the email alex@alder.ru.
Account ID: 8e418b62669470a077532442d2cf76e1
```

### Deploy dry-run
`cd hydra-relay-worker && wrangler deploy --dry-run`

```text
Binding                                                             Resource
env.HYDRA_PROVIDER_SESSIONS (HydraProviderSession)                  Durable Object
env.HYDRA_QUOTAS (82ed22205d834b94b87f42502823a59e)                 KV Namespace
env.DEFAULT_DAILY_QUOTA ("52428800")                                Environment Variable
```

### Live deploy
`cd hydra-relay-worker && wrangler deploy`

```text
Uploaded hydra-relay
Deployed hydra-relay triggers
  https://hydra-relay.hydra-net.workers.dev
  relay.hydra-net.work (custom domain)
Current Version ID: 93b673f4-6ccc-4e94-9256-72c9bdafc5f4
```

### Live verification
- `curl -i https://relay.hydra-net.work/health` -> `HTTP/2 200` body `ok`
- Legacy direct relay:
  - Target: `httpbin.org:80`
  - Result: HTTP payload returned over WebSocket
  - Quota proof: `/quota?device_id=codex-httpbin-org` -> `{"remaining":52418917,"limit":52428800}`
- Consumer without provider:
  - Result: WebSocket rejected with `HTTP 503`
- Provider registration:
  - Result: WebSocket opened and first control frame was `{"type":"registered"}`

### Cloudflare API verification
- Script settings show `HYDRA_PROVIDER_SESSIONS` Durable Object binding live
- Latest deployment:
  - deployment id: `932c7ac8-cba5-4ce2-b4a8-c27280e9b335`
  - version id: `93b673f4-6ccc-4e94-9256-72c9bdafc5f4`
  - created_on: `2026-04-03T09:07:31.747875Z`

## Flutter / Mobile

### FRB codegen
`cd hydra_mobile && flutter_rust_bridge_codegen generate --config-file flutter_rust_bridge.yaml`

```text
Done!
```

### Analyze
`cd hydra_mobile && flutter analyze`

```text
No issues found!
```

### Tests
`cd hydra_mobile && flutter test`

```text
All tests passed!
```

### Release build
`cd hydra_mobile && flutter build apk --release`

```text
✓ Built build/app/outputs/flutter-apk/app-release.apk (562.3MB)
```

Artifact:
- `hydra_mobile/build/app/outputs/flutter-apk/app-release.apk`

Note:
- Physical Android install / 5-minute runtime validation was not executed in this environment.

## Base Sepolia DealBoard

### Live read
`cast call 0x0c811902c990c4D330c1269cc955140d975f7035 "totalOffers()(uint256)" --rpc-url https://sepolia.base.org`

```text
0
```

### Dealer readiness
- Dealer / deployer wallet: `0x6C69eE6e524F12d20c14c4b8CaAa754012c9dC63`
- Agent ownership check:

```text
ownerOf(3377) -> 0x6C69eE6e524F12d20c14c4b8CaAa754012c9dC63
```

- Balances before seed:
  - USDC: `19000000` (19 USDC)
  - ETH: `9978704669821674` wei

### Seed transaction
Command:

```bash
cast send 0x0c811902c990c4D330c1269cc955140d975f7035 \
  'createDealOffer(uint256,string,uint256,uint256,uint256,string[])' \
  3377 'RUB' 100000000 1000000 100000000 '["bank_transfer"]' \
  --rpc-url https://sepolia.base.org \
  --keystore ~/.foundry/keystores/root \
  --password ''
```

Result:

```text
status               1 (success)
transactionHash      0x4137cc1811329072f3dd206937e34f214b43d7662318cad569341df2d237e46a
blockNumber          39719689
```

Important correction:
- `HydraDealBoard.rate` is **fiat units per 1 USDC with 6 decimals**
- Therefore `100 RUB / 1 USDC` must be encoded as `100_000_000`, not `1_000_000`

### Post-seed verification

`cast call totalOffers()`

```text
1
```

`cast call getOffer(1)`

```text
(0x6C69eE6e524F12d20c14c4b8CaAa754012c9dC63, 3377, "RUB", 100000000, 1000000, 100000000, ["bank_transfer"], true)
```

## Remaining external validation
- iOS device validation
- Real provider session from mobile runtime against deployed Worker

## Android device validation

### Device and app
- Device: `SGDANRAMW8HM4XG6` (`M2101K7BNY`, Android 13, arm64)
- Package: `com.hydra.network.hydra_mobile`
- Activity: `.MainActivity`
- Validation window:
  - Relaunched foreground app on PID `29085`
  - Stayed alive and focused from `12:22` through `12:28` local time without another process restart

### Confirmed working on device
- App launches and stays in foreground.
- Android `VpnService` is established on `tun0`.
- Connect/Balance UX is present on the device:
  - starter balance
  - route status
  - identity anchor card
  - Share & Earn surface
  - advanced tools entry
- `Top up` opens `P2P Deals`.
- Live deal board data loads on device:
  - `Deal #1`
  - dealer `0x6c69ee...c9dc63`
  - agent `3377`
  - currency `RUB`
  - payment `bank_transfer`
- Advanced Marketplace opens from Balance.
- Live route book data loads on device after setting marketplace region filter to `US`:
  - `Offer #1`
  - provider `0x6c69ee6e524f12d20c14c4b8caaa754012c9dc63`
  - agent `3377`
  - protocol `vless`
  - price `1 USDC / GB`
  - stake `1 USDC`
  - bandwidth `100 Mbps`

### Active blockers found on device
- Runtime mismatch: app is running with `proxy_mode=full`, but current device runtime has no matching transports loaded.
  - Repeated live log:

```text
[WARN] hydra_core: Connection #...: proxied path required for ... but no matching transports are configured
```

  - Effect: VPN/tunnel is up, but proxied traffic fails instead of using the expected configured transport path.
- Intermittent native crash exists on the device when starting the tunnel path:

```text
fdsan: attempted to close file descriptor 191, expected to be unowned, actually owned by ParcelFileDescriptor
Abort message: 'fdsan: attempted to close file descriptor 191, expected to be unowned, actually owned by ParcelFileDescriptor 0x625ce8b'
```

  - Crash observed on old PID `28046` at `2026-04-03 12:21:40-12:21:41`.
  - Relaunch recovered, but FD ownership in the Android VPN bridge is still a real bug.

### Acceptance status after device pass
- `VPN starts` -> passed
- `Balance screen shows credit status` -> passed
- `Deals screen loads` -> passed with live deal `#1`
- `Marketplace / RouteBook offers load` -> passed with live offer `#1` after region filter `US`
- `Share & Earn toggle appears` -> passed
- `No crash within 5 minutes of usage` -> passed after relaunch on PID `29085`
- `Telegram works through relay` -> blocked by missing runtime transports on device

### Next fix targets
- Inspect how `hydra.toml` is materialized into `{app_documents_dir}/hydra.toml` on Android release builds and why `[[transports]]` are absent or not loaded at runtime.
- Fix Android VPN FD ownership so `ParcelFileDescriptor` and Rust tunnel lifecycle do not both close the same FD.

## Android blocker resolution pass

### hydra.toml materialization
- Added bundled mobile config asset at `hydra_mobile/assets/hydra.toml`.
- `main.dart` now copies that asset to `{app_documents_dir}/hydra.toml` on first launch before `prepareLocalRuntime()`.
- Live release-device proof after `pm clear`:

```text
Materialized bundled hydra.toml to /data/user/0/com.hydra.network.hydra_mobile/app_flutter/hydra.toml
[INFO] hydra_config: Configuration loaded from /data/user/0/com.hydra.network.hydra_mobile/app_flutter/hydra.toml
```

- The bundled transport uses `mode = "all"`, and `hydra-config` fallback default transport was also changed from `telegram` to `all`.

### Full-proxy traffic after fix
- After switching the device UI to `Full VPN`, non-Telegram traffic no longer hits the old fail-closed path.
- Live release-device proof:

```text
[INFO] hydra_core: Connection #16: target=play.googleapis.com:443, telegram=false, proxy_required=true, mode=full
[INFO] hydra_core: Connection #16: trying transport wss to play.googleapis.com:443
[INFO] hydra_core::transport::wss: WSS relay connected to play.googleapis.com:443 via wss://relay.hydra-net.work
```

- The previous warning did not reappear:

```text
proxied path required ... but no matching transports are configured
```

### fdsan / ParcelFileDescriptor
- Kotlin `HydraVpnService` now uses `ParcelFileDescriptor.detachFd()` and no longer retains or closes the Java-side descriptor after ownership moves to Rust.
- Release-device stress check:
  - 3 stop/start VPN cycles
  - same app PID before/after: `31906`
  - no `fdsan`, `Abort message`, or `ParcelFileDescriptor` double-close lines in logcat
- Live release-device proof:

```text
[INFO] rust_lib_hydra_mobile::api::vpn: Stopping VPN tunnel...
[INFO] rust_lib_hydra_mobile::api::vpn: tun2proxy stopped successfully.
I HydraVpnService: VPN established with detached FD: 177
[INFO] rust_lib_hydra_mobile::api::vpn: tun2proxy started. Intercepting all device traffic.
```

### Marketplace default region
- Marketplace region default now starts empty instead of inheriting a blocking locale default such as `RU`.
- Fresh-state release-device proof:
  - the region input was blank
  - `Offer #1` appeared without manually changing the region filter

### Updated acceptance status
- `VPN starts` -> passed
- `Balance screen shows credit status` -> passed
- `Deals screen loads` -> passed with live deal `#1`
- `Marketplace / RouteBook offers load` -> passed with live offer `#1`
- `Share & Earn toggle appears` -> passed
- `No crash within 5 minutes of usage` -> passed
- `Telegram works through relay` -> no longer blocked by missing transport config
- `Non-Telegram traffic in full mode` -> passed through WSS relay
