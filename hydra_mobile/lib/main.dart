import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/screens/proxy_screen.dart';
import 'package:hydra_mobile/screens/routes_screen.dart';
import 'package:hydra_mobile/screens/settings_screen.dart';
import 'package:hydra_mobile/screens/terminal_screen.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart' as simple_api;
import 'package:hydra_mobile/src/rust/frb_generated.dart';
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
  await RustLib.init();
  simple_api.initApp();

  try {
    final dir = await getApplicationDocumentsDirectory();
    final baseDir = dir.path;
    await _materializeBundledConfigIfNeeded(baseDir);
    await simple_api.prepareLocalRuntime(baseDir: baseDir);

    unawaited(
      simple_api
          .startHydraNode(baseDir: baseDir)
          .catchError((Object error) {
            debugPrint('Hydra proxy start error: $error');
          }),
    );
  } catch (e) {
    debugPrint('Init error: $e');
  }

  runApp(const HydraApp());
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
