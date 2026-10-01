import 'dart:async';
import 'dart:io';
import 'dart:ui';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/app/app_log.dart';
import 'package:hydra_mobile/app/app_settings.dart';
import 'package:hydra_mobile/app/models.dart';
import 'package:hydra_mobile/app/native_bridge.dart';
import 'package:hydra_mobile/app/proxy_controller.dart';
import 'package:hydra_mobile/app/proxy_scope.dart';
import 'package:hydra_mobile/logging/log_store.dart';
import 'package:hydra_mobile/rust_init.dart';
import 'package:hydra_mobile/screens/home_screen.dart';
import 'package:hydra_mobile/screens/logs_screen.dart';
import 'package:hydra_mobile/screens/servers_screen.dart';
import 'package:hydra_mobile/screens/settings_screen.dart';
import 'package:hydra_mobile/src/rust/api/shared_state.dart' as shared_state_api;
import 'package:hydra_mobile/src/rust/api/simple.dart' as simple_api;
import 'package:hydra_mobile/src/rust/api/telemetry.dart' as telemetry_api;
import 'package:path_provider/path_provider.dart';

void main() {
  runZonedGuarded(() {
    WidgetsFlutterBinding.ensureInitialized();
    FlutterError.onError = (details) {
      AppLog.error('flutter', details.exceptionAsString(), details.context?.toDescription(), details.stack);
      if (kDebugMode) FlutterError.presentError(details);
    };
    PlatformDispatcher.instance.onError = (error, stack) {
      AppLog.error('uncaught', 'Unhandled platform error', error, stack);
      return true;
    };
    runApp(const HydraApp(home: StartupScreen()));
  }, (error, stack) => AppLog.error('uncaught', 'Unhandled zone error', error, stack));
}

Future<void> _materializeBundledConfigIfNeeded(String baseDir) async {
  final configFile = File('$baseDir/hydra.toml');
  if (await configFile.exists()) return;
  await configFile.writeAsString(await rootBundle.loadString('assets/hydra.toml'));
  AppLog.info('startup', 'Wrote bundled hydra.toml to $baseDir');
}

/// Every step is logged with its duration so a device log shows exactly where
/// a failed launch stopped.
Future<ProxyController> _bootstrap() async {
  final total = Stopwatch()..start();
  Future<T> step<T>(String name, Future<T> Function() body) async {
    final watch = Stopwatch()..start();
    try {
      final result = await body();
      AppLog.info('startup', '$name ok (${watch.elapsedMilliseconds} ms)');
      return result;
    } catch (e, st) {
      AppLog.error('startup', '$name failed after ${watch.elapsedMilliseconds} ms', describeError(e), st);
      rethrow;
    }
  }

  NativeBridge.instance;
  await step('load Rust library', initHydraRustLib);
  LogStore.instance.bind(telemetry_api.createLogStream);

  final support = await getApplicationSupportDirectory();
  final logDir = Directory('${support.path}/logs');
  await logDir.create(recursive: true);
  simple_api.initApp(logDir: logDir.path);
  AppLog.attachRust();
  unawaited(shared_state_api.readLogLines().then(LogStore.instance.seed).catchError((Object _) {}));

  final device = await NativeBridge.instance.deviceInfo();
  AppLog.info('startup', 'Launch: $device');
  await NativeBridge.instance.refreshNetwork();

  final baseDir = (await getApplicationDocumentsDirectory()).path;
  await step('materialize config', () => _materializeBundledConfigIfNeeded(baseDir));
  await step('prepare runtime', () => simple_api.prepareLocalRuntime(baseDir: baseDir));
  final settings = await AppSettings.load(baseDir);
  final controller = ProxyController(baseDir: baseDir, settings: settings);
  await step('init controller', controller.init);
  AppLog.info('startup', 'Startup complete in ${total.elapsedMilliseconds} ms');
  return controller;
}

class HydraApp extends StatelessWidget {
  const HydraApp({super.key, this.home});

  final Widget? home;

  static ThemeData _theme(Brightness brightness) {
    final scheme = ColorScheme.fromSeed(seedColor: const Color(0xFF0EA5E9), brightness: brightness);
    return ThemeData(
      colorScheme: scheme,
      useMaterial3: true,
      cardTheme: const CardThemeData(margin: EdgeInsets.zero, clipBehavior: Clip.antiAlias),
      snackBarTheme: const SnackBarThemeData(behavior: SnackBarBehavior.floating),
    );
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Hydra Proxy',
      debugShowCheckedModeBanner: false,
      theme: _theme(Brightness.light),
      darkTheme: _theme(Brightness.dark),
      home: home ?? const StartupScreen(),
    );
  }
}

class StartupScreen extends StatefulWidget {
  const StartupScreen({super.key});

  @override
  State<StartupScreen> createState() => _StartupScreenState();
}

class _StartupScreenState extends State<StartupScreen> {
  ProxyController? _controller;
  Object? _error;

  @override
  void initState() {
    super.initState();
    _start();
  }

  Future<void> _start() async {
    try {
      final controller = await _bootstrap().timeout(
        const Duration(seconds: 30),
        onTimeout: () => throw TimeoutException('Startup took longer than 30 s'),
      );
      if (mounted) setState(() => _controller = controller);
    } catch (error) {
      if (mounted) setState(() => _error = error);
    }
  }

  @override
  Widget build(BuildContext context) {
    final controller = _controller;
    if (controller != null) return ProxyScope(controller: controller, child: const MainShell());

    final error = _error;
    final theme = Theme.of(context);
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 520),
            child: Padding(
              padding: const EdgeInsets.all(24),
              child: error == null
                  ? Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        const CircularProgressIndicator(),
                        const SizedBox(height: 20),
                        Text('Starting Hydra Proxy', style: theme.textTheme.titleMedium),
                      ],
                    )
                  : Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Icon(Icons.error_outline, size: 48, color: theme.colorScheme.error),
                        const SizedBox(height: 12),
                        Text('Hydra could not start', style: theme.textTheme.titleLarge, textAlign: TextAlign.center),
                        const SizedBox(height: 12),
                        SelectableText(describeError(error), textAlign: TextAlign.center),
                        const SizedBox(height: 20),
                        FilledButton.icon(
                          onPressed: () {
                            setState(() => _error = null);
                            _start();
                          },
                          icon: const Icon(Icons.refresh),
                          label: const Text('Retry'),
                        ),
                        const SizedBox(height: 8),
                        OutlinedButton.icon(
                          onPressed: () => Navigator.of(context).push(
                            MaterialPageRoute(builder: (_) => const LogsScreen()),
                          ),
                          icon: const Icon(Icons.article_outlined),
                          label: const Text('Show logs'),
                        ),
                      ],
                    ),
            ),
          ),
        ),
      ),
    );
  }
}

class MainShell extends StatefulWidget {
  const MainShell({super.key});

  @override
  State<MainShell> createState() => _MainShellState();
}

class _MainShellState extends State<MainShell> {
  int _index = 0;

  void _select(int index) {
    if (index == _index) return;
    AppLog.debug('ui', 'Tab $index');
    setState(() => _index = index);
  }

  @override
  Widget build(BuildContext context) {
    final phase = ProxyScope.of(context).phase;
    final connected = phase == ProxyPhase.connected;
    return Scaffold(
      body: IndexedStack(
        index: _index,
        children: [
          HomeScreen(onOpenServers: () => _select(1)),
          const ServersScreen(),
          const LogsScreen(),
          const SettingsScreen(),
        ],
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        onDestinationSelected: _select,
        destinations: [
          NavigationDestination(
            icon: Badge(isLabelVisible: connected, smallSize: 8, backgroundColor: Colors.green, child: const Icon(Icons.shield_outlined)),
            selectedIcon: const Icon(Icons.shield),
            label: 'Proxy',
          ),
          const NavigationDestination(icon: Icon(Icons.dns_outlined), selectedIcon: Icon(Icons.dns), label: 'Servers'),
          const NavigationDestination(icon: Icon(Icons.article_outlined), selectedIcon: Icon(Icons.article), label: 'Logs'),
          const NavigationDestination(icon: Icon(Icons.settings_outlined), selectedIcon: Icon(Icons.settings), label: 'Settings'),
        ],
      ),
    );
  }
}
