# hydra_mobile

Flutter client for the Hydra mobile runtime.

## Current Runtime Shape

- Android is the validated MVP surface: VPN mode, routes, connections, relay usage, and settings.
- iOS is currently a proxy-only validation surface. It starts the Rust/FRB runtime and exposes a local SOCKS5 proxy UI, but it does not provide an iOS Network Extension VPN tunnel.
- The bundled first-run config is `assets/hydra.toml`; the app copies it into the platform documents directory as `hydra.toml` if no user config exists yet.

## SSH `-D` proxy (iOS)

Goal: the equivalent of `ssh -D 1080 user@host` on the phone. Apps on the device (Telegram, browsers with proxy support) use SOCKS5 at `127.0.0.1:1080`; every connection becomes a `direct-tcpip` channel on one shared SSH session.

- **Rust core** (`hydra-core/src/transport/ssh.rs`): one `russh` session per server, shared through `Arc<Handle>` so channels open concurrently. Timeouts are DNS 10 s, TCP 10 s, handshake 20 s, auth 20 s, channel 15 s. Keepalive runs every 15 s and gives up after 3 misses. After a failure there is a 3 s hold-down; a channel that hits a dead session reconnects once. Host keys are pinned on first use (TOFU) in `ssh_known_hosts.json` in the documents dir. The core also exposes live status (`status_snapshot()`: state, last error, host key, channels, bytes) and ed25519 key generation.
- **SOCKS5 server** (`hydra-core/src/lib.rs`): `bind()` happens before `serve()`, so a port conflict reaches the UI as an error. Accept errors are logged and retried, never fatal. On iOS the node is built with `new_proxy_only` (no classification, enrichment or LLM, so no direct DNS/whois leaks around the tunnel). Each connection logs one `socks_event=closed` summary.
- **Mobile Rust API** (`rust/src/api/simple.rs`, `routes.rs`, `diagnostics.rs`):
  - `startHydraNode` / `stopHydraNode` / `reconnectTransports` / `getProxyStatus` (JSON).
  - Server CRUD: `saveSshServer`, `setActiveServer`, `deleteServer`, `forgetServerHostKey`. Exactly one SSH server is active; the built-in relay profiles are disabled when one is selected.
  - Keys and checks: `generateSshKey`, `describeSshKey`, and `testSshServer` (connects, authenticates, opens a test channel to 1.1.1.1:443).
- **Dart app layer** (`lib/app/`): `ProxyController` (lifecycle, 1 s status polling in the foreground and 10 s in the background, auto-restart with backoff, reconnect on network change and resume, a heartbeat log line every 60 s, start-on-launch), `NativeBridge` (channel `hydra/native`), `AppSettings` (`app_settings.json`, no secrets), `models.dart` (typed JSON views and an `ssh user@host -p N` parser).
- **UI** (Material 3, light and dark): tabs are **Proxy** (status, Start/Stop, copyable address, "Use in Telegram" `tg://socks` link, traffic stats, last error), **Servers** (list, active radio, editor), **Logs** and **Settings**. Settings has the keep-alive and start-on-launch switches, the log folder, and an Advanced section with the legacy Routes and Network shell screens. The server editor accepts a pasted `ssh …` command, generates a key and shows the public key to copy into `authorized_keys`, accepts a pasted private key, runs "Test connection" and can reset the pinned host key.
- **Background** (`ios/Runner/AppDelegate.swift`): iOS suspends apps a few seconds after they leave the foreground, which kills a local proxy. While the proxy runs and "Keep running in background" is on, `BackgroundKeepAlive` plays a silent looping buffer (`UIBackgroundModes: audio`, `.mixWithOthers`) and restarts after audio interruptions. This is acceptable for TestFlight, but **App Store review may reject it**; the long-term path is a Network Extension. `NWPathMonitor` reports network changes to Dart, which reconnects SSH. Memory warnings, Low Power Mode and app lifecycle changes are logged.
- **Secrets** (passwords, private keys) are stored in the route profiles file in the app's documents dir, not in the Keychain yet. Diagnostics reports redact them.

## First Launch

`lib/main.dart` runs inside `runZonedGuarded`, with `FlutterError.onError` and `PlatformDispatcher.onError` wired to the log. Startup steps are logged with their duration:

1. Flutter renders the startup screen.
2. The Rust library loads (`initHydraRustLib`), the live log stream is bound, and `initApp(logDir: <Application Support>/logs)` installs tracing with file logging. Dart lines buffered before this point are replayed.
3. Device info is logged, `hydra.toml` is materialized, and `prepareLocalRuntime` runs.
4. `ProxyController.init()` loads servers and auto-starts the proxy if an SSH server is active and "Start on launch" is on.

On failure the startup screen shows the error with **Retry** and **Show logs** buttons.

## Versioning

Single source of truth: `pubspec.yaml`:

```yaml
version: 1.5.4+10504   # 1.5.4 = Version (App Store), 10504 = Build
```

Build number convention: `MAJOR*10000 + MINOR*100 + PATCH`. Upload history:

| Version | Date | Notes |
|---------|------|-------|
| 1.5.2+10502 | 2026-06-06 | Proxy-only + in-memory log view. TestFlight build expired after 90 days (early Sep 2026). |
| 1.5.3+10503 | 2026-10-01 | Rebuild of the same code for a fresh TestFlight build. Clean `flutter build ipa --release` succeeded with no fixes needed (Xcode 26.5, rustc 1.95.0); `_frb_get_rust_content_hash` is present in `Runner`. No App Store Connect API key on the Mac, so the archive was opened in Organizer for a manual upload. |
| 1.5.4+10504 | 2026-10-01 | SSH `-D` proxy rework: reliable SSH transport, persistent logs and diagnostics export, background keep-alive, new UI. Archive built (`_frb_get_rust_content_hash` present, `UIBackgroundModes: audio`). `flutter build ipa` export failed with "No Accounts / No signing certificate iOS Distribution" (only an Apple Development identity in the keychain and no Xcode account session in the shell); the API-key export (`xcodebuild -exportArchive -authenticationKey*`) failed the same way. Upload from Organizer, or sign in to Xcode → Settings → Accounts and rerun. |

After changing the version, refresh iOS/Xcode glue (do **not** edit `ios/Flutter/Generated.xcconfig` by hand):

```bash
cd hydra_mobile
flutter pub get
flutter build ios --config-only
```

Android picks up `versionName` / `versionCode` from `pubspec.yaml` automatically. iOS uses `$(FLUTTER_BUILD_NAME)` and `$(FLUTTER_BUILD_NUMBER)` in `Info.plist` via `Generated.xcconfig`.

## TestFlight (Xcode)

1. Bump `version:` in `pubspec.yaml` (build number must increase for every upload).
2. Run `flutter build ios --config-only` (or a full `flutter build ios --release` once).
3. Open **`ios/Runner.xcworkspace`** (not `.xcodeproj`).
4. Select target **Runner**, scheme **Runner**, destination **Any iOS Device**.
5. **Product → Archive** (Release configuration).
6. In Organizer: **Distribute App → App Store Connect → Upload**.
7. In App Store Connect, assign the build to TestFlight testers.

TestFlight builds expire 90 days after upload; re-uploading the same code still needs a higher build number.

Clean rebuild on the Mac (from the `proxy-only` line, e.g. after a long gap or a Xcode/Flutter/Rust update):

```bash
git fetch origin && git checkout proxy-only && git pull   # or the release branch being shipped
cd hydra_mobile
flutter clean && (cd rust && cargo clean)
flutter pub get
(cd ios && pod install)
flutter build ios --release          # sanity build; then Archive in Xcode as above
# alternatively: flutter build ipa --release  → build/ios/ipa/*.ipa, upload via Transporter
nm -gU build/ios/archive/Runner.xcarchive/Products/Applications/Runner.app/Runner | grep frb_get_rust_content_hash   # must print a symbol
```

Uploading the `.ipa`: with an App Store Connect API key (`AuthKey_<KEY_ID>.p8` in `~/.appstoreconnect/private_keys/`) run `xcrun altool --upload-app --type ios -f build/ios/ipa/*.ipa --apiKey <KEY_ID> --apiIssuer <ISSUER_ID>`. Without a key, `open build/ios/archive/Runner.xcarchive` and use **Distribute App → App Store Connect → Upload** in Organizer. `flutter clean` + `pod install` normally leave `Podfile.lock` and `project.pbxproj` unchanged; commit them if they do change.

After installing from TestFlight, check that the app gets past the startup screen (no "Hydra could not start"), and that the Logs tab shows SSH `connected`/`authenticated` events. If startup fails, check `frb_get_rust_content_hash` with the `nm` commands below.

Note: the Linux dev VM cannot build the iOS target (`ring` needs Apple clang / the iOS SDK). `cargo +stable check -p rust_lib_hydra_mobile` works there as a host-side compile check; the workspace needs Rust ≥ 1.85 (edition 2024).

Use bundle ID `work.hydra-net.nicegram` (must match the App Store Connect app record). Signing team is already set in the Xcode project (`DEVELOPMENT_TEAM`).

Before archiving after dependency changes:

```bash
cd hydra_mobile
flutter pub get
cd ios && pod install && cd ..
```

If `pod install` warns that CocoaPods could not set the base configuration for **Profile**, ensure `ios/Flutter/Profile.xcconfig` exists and the Runner **Profile** configuration points to it (not only `Release.xcconfig`).

**Upload Symbols / missing `objective_c.framework` dSYM:** keep `objective_c` on the current native-assets implementation (currently `9.4.1`). Older `9.1.0` avoids the upload warning but breaks runtime native-asset lookup on current Flutter. Xcode runs `ios/scripts/embed_native_framework_dsyms.sh` after embedding frameworks; it generates `objective_c.framework.dSYM` with `dsymutil` so App Store Connect receives the matching UUID.

**Release Rust:** cargokit links `librust_lib_hydra_mobile.a` via Pods (`-force_load`). Release also uses `-dead_strip`; `ios/Runner/rust_link_stub.c` anchors `frb_get_rust_content_hash` so Rust is not stripped from `Runner`. Debug loads `Runner.debug.dylib` via `lib/rust_init.dart`.

**Rust bitcode vs Apple `ld` ("Hydra could not start" / `frb_get_rust_content_hash` symbol not found):** Rust defaults to `embed-bitcode=yes`, putting an `.llvmbc` section in the staticlib objects. With `-force_load`, Apple's `ld` must parse every member object and aborts on bitcode produced by a newer LLVM than the Xcode linker (`ld: ... Unknown attribute kind (NNN) (Producer: 'LLVM 22...' Reader: 'LLVM APPLE_1_...')`). When that happens the link silently drops the Rust objects, so `frb_get_rust_content_hash` ends up present in the `.a` but **absent from `Runner`**, and FRB's runtime `dlsym` fails. Fix: `.cargo/config.toml` forces `-C embed-bitcode=no` for the `*-apple-ios*` targets (we never use LTO). Diagnose by checking the symbol in both places on the Mac:

```bash
nm -arch arm64 <path>/librust_lib_hydra_mobile.a | grep frb_get_rust_content_hash   # present
nm -gU <archive>/Runner.app/Runner            | grep frb_get_rust_content_hash      # must also be present
```

If it is in the `.a` but not in `Runner`, the bitcode/`-force_load` parse failure above is the cause (not `-dead_strip` and not the force_load path).

## Logging and diagnostics

Logs are designed so that one export from the device explains a failure, without a debugger attached.

- **Line format:** `HH:MM:SS.mmm [LEVEL] target: message key=value…`. All tracing fields are kept. Dart lines use target `dart::<area>` (`AppLog` in `lib/app/app_log.dart`), and iOS native lines arrive as `dart::ios`.
- **Pipeline** (`rust/src/logging.rs`): each line goes to (1) stderr in debug builds and on iOS (visible in Xcode / Console.app), (2) a non-blocking file sink, and (3) the live FRB stream (`createLogStream`) plus the backfill ring (`readLogLines`).
- **Files:** `<Application Support>/logs/hydra-YYYYMMDD-HHMMSS.mmm.log`, rotated at 8 MB, keeping at most 10 files. A Rust panic is written synchronously to `panic.log`; on the next launch it is reported in the log and renamed to `panic.reported.log`.
- **Verbosity:** `debug` by default, with `russh` and other noisy crates at `info`. Override with the `HYDRA_LOG` env filter.
- **Key events:**
  - `ssh_event` (connecting, connected, authenticated, channel_open, failures with a reason);
  - `socks_event=closed` (target, route, bytes, duration, error);
  - `dart::startup` step timings;
  - `dart::controller` (start/stop, SSH state transitions, restarts);
  - `dart::heartbeat` every 60 s (running, foreground, keep-alive, connections, bytes, network);
  - `dart::lifecycle`, `dart::network`, and `dart::flutter`/`dart::uncaught` errors.
- **Export:** on the Logs tab, the share button writes `diagnostics-<time>.txt` (proxy status, servers with secrets removed, `hydra.toml`, file list, last 1500 lines, device info) and opens the iOS share sheet with the report plus the three newest log files (AirDrop to the Mac works). The Settings tab shows the log folder.
- **Live view** (`lib/logging/log_store.dart`, `lib/screens/logs_screen.dart`): keeps the newest 20k records, coalesces UI updates (150 ms), and offers level chips (Info by default), an SSH-only filter, a text filter, copy and clear. The list sticks to the tail until you scroll up.

## Debugging without long device builds

Most of the proxy can be tested on the Mac, without iOS:

```bash
cd hydra_mobile
tool/e2e_local_ssh.sh                 # throwaway sshd on 127.0.0.1:2222 + 12 SOCKS scenarios via the host harness
cargo run --manifest-path rust/Cargo.toml --example proxy_harness -- \
  --base-dir /tmp/hydra-harness --ssh user@host:22 --key ~/.ssh/id_ed25519 --status-every 5
```

- `rust/examples/proxy_harness.rs` drives the same mobile Rust API as the app (`init_app`, `prepare_local_runtime`, `test_ssh_server`, `save_ssh_server`, `start_hydra_node`, periodic status). Use `--ssh user@host:port` with `--key <file>` or `--password <pw>` to point it at a real server, then `curl --socks5-hostname 127.0.0.1:1080 https://example.com`.
- The e2e script only kills its own sshd and harness processes.
- **Real app in the Simulator** (about 1 minute after a build, no taps needed): `tool/sim_local_ssh.sh` (or `SKIP_BUILD=1 KEEP=1 …`). It starts an sshd on 127.0.0.1:2223, seeds `mobile_routes.json` / `ssh_known_hosts.json` with the harness, copies them into the app's Documents dir, launches the app with its console in `/tmp/hydra-sim/app.log`, then checks auto-start, SOCKS fetches, 20 parallel requests, the SSH state transition, log files, the network path report and a fetch after 20 s with Safari in front. Last run: 10/10. The Simulator suspends background apps less aggressively than a device, so verify the keep-alive on a phone.
- Device run: `flutter run --release -d <udid>` and watch the console, or install from TestFlight and use **Logs → Share**.
- If `xcrun devicectl list devices` stays at `connecting` ("tunnel connection failed"), a Mac VPN (Sota Connect, WireGuard, v2RayTun) is usually capturing the CoreDevice tunnel. Disconnect it while running from Xcode/Flutter.

## Application IDs

Keep these aligned with App Store Connect / Google Play Console:

| Platform | ID | Notes |
|----------|-----|--------|
| iOS | `work.hydra-net.nicegram` | Set in `ios/Runner.xcodeproj` (Debug / Profile / Release) |
| Android | `work.hydra_net.nicegram` | `android/app/build.gradle.kts` — underscore instead of hyphen (Gradle rule) |

## iOS Notes

For a first physical-device run:

```bash
cd hydra_mobile
flutter run -d <ios-device-id> -v
```

If the app shows the startup error screen, inspect the device log for:

- `Hydra startup error`
- `Invalid argument(s): Failed to load dynamic library`
- `MissingPluginException`
- CocoaPods or code signing errors around `rust_lib_hydra_mobile`

The iOS Pod builds the Rust static library through `rust_builder/cargokit`. A successful iOS build must link `librust_lib_hydra_mobile.a` into `Runner.app` (`-force_load` via `rust_lib_hydra_mobile.podspec` `user_target_xcconfig`, plus `-framework SystemConfiguration` for Rust networking deps). There is no `rust_lib_hydra_mobile.framework` at runtime. On Flutter 3.41+ debug builds, `-force_load` lands in `Runner.debug.dylib`; `lib/rust_init.dart` opens that dylib when present, otherwise falls back to `ExternalLibrary.process()` for release-style single-binary links.

**Simulator** works on Intel Macs too. The Podfile used to force `ARCHS[sdk=iphonesimulator*] = arm64` (an Intel Mac cannot run that: "Failed to find matching arch"). The override was removed because current `objective_c` native assets build for x86_64, so `flutter build ios --simulator` now produces a universal x86_64+arm64 `Runner.app`. Do not re-add the override. Background audio, the share sheet and the network monitor behave as on a device; the Simulator shares the Mac's loopback.

## Validation

Useful local checks:

```bash
cd hydra_mobile
flutter analyze lib/main.dart lib/app lib/logging lib/screens/{home,servers,server_editor,logs,settings}_screen.dart
flutter test test/widget_test.dart test/proxy_models_test.dart
(cd rust && cargo test) && (cd ../hydra-core && cargo test transport::ssh)
tool/e2e_local_ssh.sh
```

Full-project `flutter analyze` is currently blocked by older hidden/latent screens (wallet, marketplace, `share_plus`/`url_launcher` widgets) that are not part of the proxy-only navigation surface.

## Next steps

- Store SSH secrets in the Keychain instead of the profiles file.
- Move the proxy into a Network Extension (packet tunnel or app proxy) so it survives without background audio and passes App Store review.
- Optional: a dynamic-port `-L` forward and per-server SOCKS port.
