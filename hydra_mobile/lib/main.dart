import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/screens/proxy_screen.dart';
import 'package:hydra_mobile/screens/routes_screen.dart';
import 'package:hydra_mobile/screens/settings_screen.dart';
import 'package:hydra_mobile/screens/terminal_screen.dart';
import 'package:hydra_mobile/rust_init.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart' as simple_api;
import 'package:path_provider/path_provider.dart';

Future<void> _materializeBundledConfigIfNeeded(String baseDir) async {
  final configFile = File('$baseDir/hydra.toml');
  if (await configFile.exists()) {
    return;
  }
  final content = await rootBundle.loadString('assets/hydra.toml');
  await configFile.writeAsString(content);
}

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const HydraApp(home: StartupScreen()));
}

Future<void> _bootstrapHydraRuntime() async {
  await initHydraRustLib();
  simple_api.initApp();

  final dir = await getApplicationDocumentsDirectory();
  final baseDir = dir.path;
  await _materializeBundledConfigIfNeeded(baseDir);
  await simple_api.prepareLocalRuntime(baseDir: baseDir);

  unawaited(
    simple_api.startHydraNode(baseDir: baseDir).catchError((Object error) {
      debugPrint('Hydra proxy start error: $error');
    }),
  );
}

class HydraApp extends StatelessWidget {
  const HydraApp({super.key, this.home});

  final Widget? home;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Hydra Proxy',
      theme: ThemeData.dark(useMaterial3: true).copyWith(
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xFF0EA5E9),
          brightness: Brightness.dark,
        ),
        scaffoldBackgroundColor: const Color(0xFF08111F),
      ),
      home: home ?? const MainScreen(),
    );
  }
}

class StartupScreen extends StatefulWidget {
  const StartupScreen({super.key});

  @override
  State<StartupScreen> createState() => _StartupScreenState();
}

class _StartupScreenState extends State<StartupScreen> {
  Object? _error;
  bool _ready = false;

  @override
  void initState() {
    super.initState();
    _start();
  }

  Future<void> _start() async {
    try {
      await _bootstrapHydraRuntime().timeout(
        const Duration(seconds: 20),
        onTimeout: () => throw TimeoutException(
          'Hydra startup timed out while initializing the Rust runtime.',
        ),
      );
      if (!mounted) return;
      setState(() {
        _ready = true;
        _error = null;
      });
    } catch (error, stackTrace) {
      debugPrint('Hydra startup error: $error\n$stackTrace');
      if (!mounted) return;
      setState(() {
        _error = error;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_ready) {
      return const MainScreen();
    }

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
                        const SizedBox(
                          width: 36,
                          height: 36,
                          child: CircularProgressIndicator(strokeWidth: 3),
                        ),
                        const SizedBox(height: 20),
                        Text(
                          'Starting Hydra Proxy',
                          style: theme.textTheme.titleLarge,
                          textAlign: TextAlign.center,
                        ),
                      ],
                    )
                  : Card(
                      child: Padding(
                        padding: const EdgeInsets.all(20),
                        child: SingleChildScrollView(
                          child: Column(
                            mainAxisSize: MainAxisSize.min,
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                            Icon(
                              Icons.error_outline,
                              size: 40,
                              color: theme.colorScheme.error,
                            ),
                            const SizedBox(height: 12),
                            Text(
                              'Hydra could not start',
                              style: theme.textTheme.titleLarge,
                              textAlign: TextAlign.center,
                            ),
                            const SizedBox(height: 12),
                            SelectableText(
                              error.toString(),
                              style: theme.textTheme.bodySmall,
                              textAlign: TextAlign.center,
                            ),
                            const SizedBox(height: 16),
                            FilledButton.icon(
                              onPressed: () {
                                setState(() {
                                  _error = null;
                                });
                                _start();
                              },
                              icon: const Icon(Icons.refresh),
                              label: const Text('Retry'),
                            ),
                          ],
                          ),
                        ),
                      ),
                    ),
            ),
          ),
        ),
      ),
    );
  }
}

class MainScreen extends StatefulWidget {
  const MainScreen({super.key});

  @override
  State<MainScreen> createState() => _MainScreenState();
}

class _MainScreenState extends State<MainScreen> {
  int _currentIndex = 0;

  static const _titles = <String>[
    'Proxy',
    'Routes',
    'Terminal',
    'Settings',
  ];

  final List<Widget> _screens = const [
    ProxyScreen(),
    RoutesScreen(),
    TerminalScreen(),
    SettingsScreen(),
  ];

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(_titles[_currentIndex]), centerTitle: true),
      body: IndexedStack(index: _currentIndex, children: _screens),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _currentIndex,
        onDestinationSelected: (index) {
          setState(() {
            _currentIndex = index;
          });
        },
        destinations: const [
          NavigationDestination(
            icon: Icon(Icons.shield_outlined),
            selectedIcon: Icon(Icons.shield),
            label: 'Proxy',
          ),
          NavigationDestination(icon: Icon(Icons.route), label: 'Routes'),
          NavigationDestination(
            icon: Icon(Icons.terminal),
            label: 'Terminal',
          ),
          NavigationDestination(icon: Icon(Icons.settings), label: 'Settings'),
        ],
      ),
    );
  }
}
