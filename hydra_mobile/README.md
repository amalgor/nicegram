# hydra_mobile

Flutter client for the Hydra mobile runtime.

## Current Runtime Shape

- Android is the validated MVP surface: VPN mode, routes, connections, relay usage, and settings.
- iOS is currently a proxy-only validation surface. It starts the Rust/FRB runtime and exposes a local SOCKS5 proxy UI, but it does not provide an iOS Network Extension VPN tunnel.
- The bundled first-run config is `assets/hydra.toml`; the app copies it into the platform documents directory as `hydra.toml` if no user config exists yet.

## First Launch

Startup order is intentionally defensive:

1. Flutter renders a startup screen immediately.
2. The app initializes `flutter_rust_bridge` / Rust.
3. The app creates or loads `hydra.toml`.
4. The app prepares route/runtime files.
5. The app starts the local SOCKS5 node in the background.

If iOS cannot load the Rust library, cannot register a Flutter plugin, times out during Rust startup, or fails while preparing local runtime files, the app now shows an on-screen error and retry button instead of staying on a blank white screen.

## Versioning

Single source of truth: `pubspec.yaml`:

```yaml
version: 1.4.1+10401   # 1.4.1 = Version (App Store), 10401 = Build
```

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

## Logging (in-memory live log)

The app streams Rust runtime events to a live, in-memory log view. **No log files are written** — logs exist only for the app session.

- **Source:** `tracing` events in Rust → `FlutterLogLayer` (`rust/src/api/telemetry.rs`) → FRB `Stream<String>` (`createLogStream`). Each line is `"[LEVEL] target: message"`.
- **In-memory only:** `rust/src/api/shared_state.rs` keeps a bounded ring (`MAX_LOG_LINES = 5000`) for backfill; the per-line `logs.json` write was removed. The Flutter UI keeps its own session-length view.
- **Wiring:** `lib/main.dart` `_bootstrapHydraRuntime` calls `LogStore.instance.bind(createLogStream)` before `initApp()`, then backfills via `readLogLines()`.
- **Store:** `lib/logging/log_store.dart` — singleton `LogStore` (`ChangeNotifier`); parses each line once into `LogRecord { level, target, message, raw }`.
- **UI:** `lib/screens/logs_screen.dart` — Logs tab. 10pt monospace, color-coded by level (error red, warn amber, info green, debug/trace slate). Verbosity selector (ERR/WARN/INFO/DEBUG/TRACE; **INFO default**, debug/trace available but off), SSH-only filter, text filter, copy, clear.
- **Stable scroll:** the list is `reverse: true` (newest at offset 0). It sticks to the tail while the user is at the bottom; once scrolled up it **freezes** and new lines append off-screen without moving the viewport. A "jump to latest" FAB appears when not following.

### SSH event logging

`hydra-core/src/transport/ssh.rs` emits structured `tracing` events with an `ssh_event` field at every `russh` call site: `created`, `connecting`, `connected`, `authenticated`, `channel_open`, `channel_closed`, `reconnect` (INFO/WARN), and failures `connect_timeout`, `connect_failed`, `auth_error`, `auth_failed`, `channel_failed` (WARN). The mobile `EnvFilter` (`rust/src/api/simple.rs`) keeps `hydra_core::transport=debug` so byte-bridge details are available; lifecycle events are INFO and visible by default.

### Connection activity indicators

`lib/widgets/activity_indicators.dart` (shown on the Proxy tab status card) derives simple indicators from the log stream: an activity dot that pulses on new lines, and an SSH up/down dot driven by SSH lifecycle events. This is Phase 1; the full connection-classification UI (ad/telemetry/analytics, app attribution — `hydra-core/src/connections.rs` + the orphaned `ConnectionsScreen`) is a later phase that requires re-exposing `ConnectionRegistry` through FRB.

### Rebuilding after these changes (Mac)

The log stream uses the existing `createLogStream` FRB binding (no codegen needed). Rust changed, so the static lib must rebuild:

```bash
cd hydra_mobile
cargo clean            # force cargokit to rebuild librust_lib_hydra_mobile.a
flutter pub get
flutter run -d <ios-device-id>     # or build/archive as usual
```

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

**Physical device** is the supported iOS test path (Developer Mode on, USB trust). Intel Mac simulators may build `Runner` as x86_64 while Flutter ships arm64-only plugin frameworks (`objective_c`); use a real device or an Apple Silicon Mac for simulator runs.

## Validation

Useful local checks:

```bash
cd hydra_mobile
flutter analyze lib/main.dart
flutter test test/widget_test.dart
```

Full-project `flutter analyze` is currently blocked by older hidden/latent screens and stale generated FRB imports that are not part of the proxy-only navigation surface.
