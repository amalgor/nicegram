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

## Android relay deep dive — 2026-04-04

### Scope
- Fix live byte accounting in the connections list
- Re-test Telegram-over-Worker on a physical Android device under real carrier blocking / DPI
- Add enough client/server telemetry to decide whether the remaining Telegram failure is a code bug or a transport limitation

### Live byte accounting
- `hydra-core/src/lib.rs`
  - `copy_with_rate_limit(...)` now reports progress per copied chunk instead of updating `ConnectionRegistry` only after the connection closes.
  - Both proxied and direct paths now call `registry.update_bytes(...)` live during the copy loop.
- Release-device proof:
  - Network screen no longer stays at `0 / 0`
  - Example live state observed on device:
    - `166.121 / RELAY / TG / 50 conn / 144.0 KB`
    - `195.2 / RELAY / 37 conn / 45.7 KB`
    - top counters showed non-zero `Up` and `Down`

### Telegram target classification hardening
- `hydra-core/src/socks.rs`
  - Telegram detection now uses the official Telegram CIDR snapshot from `core.telegram.org/resources/cidr.txt`, including IPv6 ranges.
  - IPv6 target parsing was normalized for `[addr]:port` form.
- `hydra-relay-worker/src/index.ts`
  - Target parsing now correctly handles bracketed IPv6 targets instead of naive `split(":")`.

### Direct relay telemetry added
- `hydra-relay-worker/src/index.ts`
  - Added per-connection trace ids and logs for:
    - relay start
    - TCP `opened`
    - first client frame
    - first TCP response frame
    - readable close
    - socket close/error
    - final byte/frame totals
  - Socket mode changed to `allowHalfOpen: true`.
  - Eager WebSocket close on `tcp.readable.close()` was removed; close now follows `socket.closed`.
- `hydra-core/src/transport/wss.rs`
  - Added warnings for worker close frames, websocket receive errors, bridge read errors, and clearer end-of-task logging.

### Device result after deep dive
- Telegram app still shows `Connecting...` after repeated cold starts in both `telegram` and `full` modes.
- This is no longer explained by:
  - missing `hydra.toml`
  - wrong `proxy_mode`
  - zero live counters
  - Android `ParcelFileDescriptor` crash
  - incomplete Telegram CIDR matching
  - IPv6 target parse bugs

### Key traces
- Client-side:
  - Hydra repeatedly routes Telegram DC traffic through Worker, for example:
    - `149.154.166.121:443`
    - `149.154.166.121:5222`
    - `149.154.165.111:5222`
    - `149.154.167.151:443`
    - `149.154.175.52:443`
  - `WSS relay connected ... via wss://relay.hydra-net.work` is repeatedly observed.
- Worker-side:
  - One failed trace showed the remote side closing immediately after the first client bytes and before any response:

```text
[relay:cfd49a23] connect start target=149.154.165.111:443 device=anonymous
[relay:cfd49a23] tcp opened target=149.154.165.111:443 remote=149.154.165.111:443 local=unknown
[relay:cfd49a23] first client frame target=149.154.165.111:443 bytes=146
[relay:cfd49a23] tcp readable closed target=149.154.165.111:443 frames=0
[relay:cfd49a23] finalize target=149.154.165.111:443 bytes=146 c2t_frames=1 t2c_frames=0
[relay:cfd49a23] websocket closed target=149.154.165.111:443 bytes=146 duration_ms=187
```

  - Another trace showed partial bidirectional exchange, but Telegram still abandoned the session:

```text
[relay:de21c2b8] connect start target=149.154.166.121:443 device=anonymous
[relay:de21c2b8] tcp opened target=149.154.166.121:443 remote=149.154.166.121:443 local=unknown
[relay:de21c2b8] first client frame target=149.154.166.121:443 bytes=418
[relay:de21c2b8] first tcp frame target=149.154.166.121:443 bytes=178
[relay:de21c2b8] websocket closed target=149.154.166.121:443 bytes=596 duration_ms=9464
[relay:de21c2b8] tcp readable closed target=149.154.166.121:443 frames=1
[relay:de21c2b8] finalize target=149.154.166.121:443 bytes=596 c2t_frames=1 t2c_frames=1
```

### Conclusion
- **Live byte accounting is fixed.**
- **Telegram-over-Worker is still not operational in this real blocked-network scenario.**
- The remaining blocker is now classified as a **transport / interoperability problem on the Telegram path itself**, not an Android runtime misconfiguration:
  - some Telegram DC connections are accepted and then closed without a response
  - others exchange some bytes but still do not complete into a usable Telegram session
- With the current architecture, Cloudflare Worker WSS relay is good enough for general TCP relay testing and non-Telegram traffic, but it is **not yet sufficient as a reliable Telegram MTProto censorship bypass** on this carrier path.

### Diagnostic redeploy follow-up
- Worker redeployed again after the long-lived socket diagnostics cleanup.
- Current production worker version: `4cc786c7-3d70-456f-9a47-b3410add395c`
- `/health` remained green after the redeploy and `wrangler tail` resumed showing production relay traces.
- Fresh device proof after this redeploy:
  - Hydra returned to `Connected`
  - Connect screen showed non-zero live counters (`17.0 KB` relayed, `6.8 KB` total at the sampled moment)
  - Telegram UI still rendered `Connecting...`
- Fresh Android-side trace after reconnect:
  - `WSS relay connected to 149.154.166.121:5222`
  - `WSS relay connected to 149.154.167.151:5222`
  - `WSS relay connected to 149.154.166.121:443`
  - `WSS relay connected to 149.154.167.151:443`
  - `WSS relay connected to 5.28.195.2:443`
- Fresh worker-side trace after redeploy again showed live direct-relay telemetry, including explicit upstream refusal on `mozilla.cloudflare-dns.com:443` and normal lifecycle logs for other targets. This confirms the diagnostic logging path is live in production; it did not change the Telegram outcome.

### Next logical step
- Move Telegram censorship bypass off the plain Cloudflare Worker TCP relay path and onto a transport that is designed for this threat model:
  - premium/provider routes (`vless://` / Reality)
  - relay-backed provider sessions once real mobile providers are validated
  - later DPI hardening / camouflage work

## Android relay ECH follow-up — 2026-04-04 (late)

### Context
- Relay client was upgraded to use `rustls` ECH with Cloudflare DoH against `relay.hydra-net.work`.
- Physical Android device `M2101K7BNY` remained connected on the same carrier path for live validation.

### What the logs showed
- TCP connect to Cloudflare edge succeeds immediately through DoH-resolved IPs such as `104.21.95.158:443`.
- No live `ECH accepted` was observed on this carrier path.
- ECH-enabled TLS handshake repeatedly timed out after 5s for relay targets such as:
  - `mtalk.google.com:5228`
  - `149.154.167.51:443`
  - `149.154.167.51:5222`
- Standard TLS fallback to the same `relay.hydra-net.work` endpoint then succeeded and completed WSS upgrade for those same targets.

### Representative log lines
```text
[WARN] Relay: [mtalk.google.com:5228] ECH-enabled TLS handshake failed for relay.hydra-net.work:443: TLS handshake timeout. Retrying with standard TLS.
[DEBUG] Relay: [mtalk.google.com:5228] TLS handshake completed for relay.hydra-net.work with standard TLS
[INFO] WSS relay connected to mtalk.google.com:5228 via wss://relay.hydra-net.work using standard TLS
```

### Follow-up fix
- Added an ECH cooldown/circuit breaker in `hydra-core/src/transport/wss.rs`.
- After the first ECH handshake failure, the client disables ECH for 10 minutes and logs:
```text
Relay: ECH temporarily disabled for relay.hydra-net.work:443 after recent handshake failures; using standard TLS.
```
- Fresh Android logs confirmed that after the first failures, subsequent relay sockets no longer paid the repeated 5-second ECH timeout penalty and connected directly with standard TLS.

### Outcome
- `Hydra -> relay.hydra-net.work` now behaves correctly on this carrier path:
  - tries ECH once
  - falls back safely to standard TLS
  - avoids repeated per-socket latency with a temporary cooldown
- This improves relay usability, but it does **not** by itself fix Telegram session establishment under DPI. The remaining censorship-bypass gap is now downstream of the client->relay TLS leg.
