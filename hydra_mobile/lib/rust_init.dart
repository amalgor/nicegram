import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:hydra_mobile/src/rust/frb_generated.dart';

/// Initializes flutter_rust_bridge / cargokit on all platforms.
Future<void> initHydraRustLib() async {
  if (Platform.isIOS || Platform.isMacOS) {
    await RustLib.init(externalLibrary: _appleRustExternalLibrary());
    return;
  }
  await RustLib.init();
}

/// Cargokit links `librust_lib_hydra_mobile.a` with `-force_load`.
///
/// On Flutter 3.41+ debug iOS/macOS builds, that static archive is linked into
/// `Runner.debug.dylib`, not the small `Runner` stub executable. `DynamicLibrary.process()`
/// only searches the main executable, so we open the debug dylib when present.
ExternalLibrary _appleRustExternalLibrary() {
  final bundleDir = File(Platform.resolvedExecutable).parent.path;
  for (final name in ['Runner.debug.dylib', 'App.debug.dylib']) {
    final path = '$bundleDir/$name';
    if (File(path).existsSync()) {
      return ExternalLibrary.open(
        path,
        debugInfo: ' (cargokit static .a in $name)',
      );
    }
  }
  return ExternalLibrary.process(
    iKnowHowToUseIt: true,
    debugInfo: ' (cargokit static .a in main executable)',
  );
}
