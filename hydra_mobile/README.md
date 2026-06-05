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

**Upload Symbols / missing `objective_c.framework` dSYM:** newer `objective_c` (9.2+) can ship without valid DWARF for App Store. This repo pins `objective_c: 9.1.0` via `dependency_overrides` in `pubspec.yaml` (see [dart-lang/native#3004](https://github.com/dart-lang/native/issues/3004)). Xcode also runs `ios/scripts/embed_native_framework_dsyms.sh` after embed frameworks as a backup.

**Release Rust:** cargokit links `librust_lib_hydra_mobile.a` via Pods (`-force_load`). Release also uses `-dead_strip`; `ios/Runner/rust_link_stub.c` anchors `frb_get_rust_content_hash` so Rust is not stripped from `Runner`. Debug loads `Runner.debug.dylib` via `lib/rust_init.dart`.

**Rust bitcode vs Apple `ld` ("Hydra could not start" / `frb_get_rust_content_hash` symbol not found):** Rust defaults to `embed-bitcode=yes`, putting an `.llvmbc` section in the staticlib objects. With `-force_load`, Apple's `ld` must parse every member object and aborts on bitcode produced by a newer LLVM than the Xcode linker (`ld: ... Unknown attribute kind (NNN) (Producer: 'LLVM 22...' Reader: 'LLVM APPLE_1_...')`). When that happens the link silently drops the Rust objects, so `frb_get_rust_content_hash` ends up present in the `.a` but **absent from `Runner`**, and FRB's runtime `dlsym` fails. Fix: `.cargo/config.toml` forces `-C embed-bitcode=no` for the `*-apple-ios*` targets (we never use LTO). Diagnose by checking the symbol in both places on the Mac:

```bash
nm -arch arm64 <path>/librust_lib_hydra_mobile.a | grep frb_get_rust_content_hash   # present
nm -gU <archive>/Runner.app/Runner            | grep frb_get_rust_content_hash      # must also be present
```

If it is in the `.a` but not in `Runner`, the bitcode/`-force_load` parse failure above is the cause (not `-dead_strip` and not the force_load path).

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
