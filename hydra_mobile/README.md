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

The iOS Pod builds the Rust static library through `rust_builder/cargokit`. A successful iOS build must link `librust_lib_hydra_mobile.a` into `Runner.app`.

## Validation

Useful local checks:

```bash
cd hydra_mobile
flutter analyze lib/main.dart
flutter test test/widget_test.dart
```

Full-project `flutter analyze` is currently blocked by older hidden/latent screens and stale generated FRB imports that are not part of the proxy-only navigation surface.
