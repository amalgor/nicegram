import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/platform/hydra_platform_gateway.dart';
import 'package:hydra_mobile/screens/connect_screen.dart';
import 'package:hydra_mobile/screens/connections_screen.dart';
import 'package:hydra_mobile/screens/relay_usage_screen.dart';
import 'package:hydra_mobile/screens/routes_screen.dart';
import 'package:hydra_mobile/screens/settings_screen.dart';
import 'package:hydra_mobile/src/rust/api/model_manager.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart';
import 'package:hydra_mobile/src/rust/frb_generated.dart';
import 'package:shared_preferences/shared_preferences.dart';

final List<String> gNetworkLogs = [];
final StreamController<List<String>> gLogStreamController =
    StreamController<List<String>>.broadcast();

String _ts() {
  final n = DateTime.now();
  return '${n.hour.toString().padLeft(2, '0')}:'
      '${n.minute.toString().padLeft(2, '0')}:'
      '${n.second.toString().padLeft(2, '0')}.'
      '${n.millisecond.toString().padLeft(3, '0')}';
}

void _initGlobalLogStream() async {
  try {
    await HydraPlatformGateway.instance.bindLogs((log) {
      gNetworkLogs.add('${_ts()} $log');
      if (gNetworkLogs.length > 10000) {
        gNetworkLogs.removeAt(0);
      }
      gLogStreamController.add(gNetworkLogs);
    });
  } catch (e) {
    debugPrint('Log stream init error: $e');
  }
}

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
  await initApp();
  await HydraPlatformGateway.instance.initialize();

  _initGlobalLogStream();

  try {
    final baseDir = await HydraPlatformGateway.instance.resolveBaseDir();
    await _materializeBundledConfigIfNeeded(baseDir);
    await prepareLocalRuntime(baseDir: baseDir);
    initModelManager(baseDir: baseDir);

    unawaited(
      HydraPlatformGateway.instance
          .startNetworkRuntime(baseDir: baseDir)
          .then((_) async {
            final prefs = await SharedPreferences.getInstance();
            final mode = prefs.getString('proxy_mode') ?? 'full';
            await HydraPlatformGateway.instance.setProxyMode(mode: mode);
          })
          .catchError((Object error) {
            debugPrint('Hydra network runtime error: $error');
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
      title: 'Hydra Network',
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
    'Connect',
    'Connections',
    'Routes',
    'Relay Usage',
    'Settings',
  ];

  final List<Widget> _screens = const [
    ConnectScreen(),
    ConnectionsScreen(),
    RoutesScreen(),
    RelayUsageScreen(),
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
            icon: Icon(Icons.power_settings_new),
            label: 'Connect',
          ),
          NavigationDestination(
            icon: Icon(Icons.account_tree_outlined),
            label: 'Connections',
          ),
          NavigationDestination(icon: Icon(Icons.route), label: 'Routes'),
          NavigationDestination(
            icon: Icon(Icons.query_stats),
            label: 'Relay Usage',
          ),
          NavigationDestination(icon: Icon(Icons.settings), label: 'Settings'),
        ],
      ),
    );
  }
}
