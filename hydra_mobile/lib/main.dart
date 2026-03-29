import 'dart:async';
import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:hydra_mobile/src/rust/api/telemetry.dart';
import 'package:hydra_mobile/src/rust/api/model_manager.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart';
import 'package:hydra_mobile/src/rust/frb_generated.dart';
import 'package:hydra_mobile/screens/connect_screen.dart';
import 'package:hydra_mobile/screens/connections_screen.dart';
import 'package:hydra_mobile/screens/models_screen.dart';
import 'package:hydra_mobile/screens/content_screen.dart';
import 'package:hydra_mobile/screens/settings_screen.dart';
import 'package:hydra_mobile/screens/logs_screen.dart';

final List<String> gNetworkLogs = [];
final StreamController<List<String>> gLogStreamController = StreamController<List<String>>.broadcast();

String _ts() {
  final n = DateTime.now();
  return '${n.hour.toString().padLeft(2, '0')}:'
      '${n.minute.toString().padLeft(2, '0')}:'
      '${n.second.toString().padLeft(2, '0')}.'
      '${n.millisecond.toString().padLeft(3, '0')}';
}

void _initGlobalLogStream() async {
  debugPrint("Starting global log stream initialization...");
  try {
    final stream = createLogStream();
    debugPrint("Stream created successfully. Awaiting events...");
    await for (final log in stream) {
      debugPrint("Received log from rust: $log");
      gNetworkLogs.add('${_ts()} $log');
      if (gNetworkLogs.length > 10000) {
        gNetworkLogs.removeAt(0);
      }
      gLogStreamController.add(gNetworkLogs);
    }
  } catch (e) {
    debugPrint("Log stream init error: $e");
  }
}

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  await initApp();

  _initGlobalLogStream();

  try {
    final dir = await getApplicationDocumentsDirectory();
    initModelManager(baseDir: dir.path);

    final modelsDir = Directory('${dir.path}/models');
    if (!await modelsDir.exists()) {
      await modelsDir.create(recursive: true);
    }
    final modelPath = '${modelsDir.path}/qwen2.5-0.5b.gguf';
    final modelFile = File(modelPath);

    if (!await modelFile.exists() || await modelFile.length() < 1024) {
      debugPrint("Extracting bundled Qwen 2.5 0.5B model from assets...");
      final byteData = await rootBundle.load('assets/models/qwen2.5-0.5b.gguf');
      await modelFile.writeAsBytes(byteData.buffer.asUint8List(byteData.offsetInBytes, byteData.lengthInBytes));
      debugPrint("Model extracted successfully.");
    }

    // Start Hydra node early so SOCKS5 proxy is ready before VPN toggle
    startHydraNode(baseDir: dir.path).then((_) async {
      debugPrint("Hydra node started successfully.");
      // Apply saved proxy mode from SharedPreferences
      try {
        final prefs = await SharedPreferences.getInstance();
        final mode = prefs.getString('proxy_mode') ?? 'telegram';
        await setProxyMode(mode: mode);
        debugPrint("Applied saved proxy mode: $mode");
      } catch (e) {
        debugPrint("Failed to apply saved proxy mode: $e");
      }
    }).catchError((e) {
      debugPrint("Hydra node start error: $e");
    });
  } catch (e) {
    debugPrint("Init error: $e");
  }

  runApp(const HydraApp());
}

class HydraApp extends StatelessWidget {
  const HydraApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Hydra Network',
      theme: ThemeData.dark(useMaterial3: true).copyWith(
        colorScheme: ColorScheme.fromSeed(
          seedColor: Colors.deepPurple,
          brightness: Brightness.dark,
        ),
      ),
      home: const MainScreen(),
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

  final List<Widget> _screens = [
    const ConnectScreen(),
    const ConnectionsScreen(),
    const ModelsScreen(),
    const ContentScreen(),
    const LogsScreen(),
    const SettingsScreen(),
  ];

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Hydra P2P Node'),
        centerTitle: true,
      ),
      body: IndexedStack(
        index: _currentIndex,
        children: _screens,
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _currentIndex,
        onDestinationSelected: (index) {
          setState(() { _currentIndex = index; });
        },
        destinations: const [
          NavigationDestination(icon: Icon(Icons.power_settings_new), label: 'Connect'),
          NavigationDestination(icon: Icon(Icons.swap_vert), label: 'Network'),
          NavigationDestination(icon: Icon(Icons.smart_toy), label: 'AI'),
          NavigationDestination(icon: Icon(Icons.article), label: 'Content'),
          NavigationDestination(icon: Icon(Icons.terminal), label: 'Logs'),
          NavigationDestination(icon: Icon(Icons.settings), label: 'Settings'),
        ],
      ),
    );
  }
}
