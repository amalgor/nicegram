import 'package:hydra_mobile/src/rust/api/telemetry.dart';
import 'dart:io';
import 'package:path_provider/path_provider.dart';
import 'package:flutter/services.dart';
import 'package:flutter/material.dart';
import 'package:hydra_mobile/src/rust/api/model_manager.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart';
import 'package:hydra_mobile/src/rust/api/vpn.dart';
import 'dart:async';
import 'package:hydra_mobile/src/rust/frb_generated.dart';

// Global log storage so it catches everything before UI mounts
final List<String> gNetworkLogs = [];
// Broadcast stream controller to update UI
final StreamController<List<String>> gLogStreamController = StreamController<List<String>>.broadcast();

void _initGlobalLogStream() async {
  debugPrint("Starting global log stream initialization...");
  try {
    final stream = createLogStream();
    debugPrint("Stream created successfully. Awaiting events...");
    await for (final log in stream) {
      debugPrint("Received log from rust: $log");
      gNetworkLogs.insert(0, log);
      if (gNetworkLogs.length > 500) {
        gNetworkLogs.removeLast();
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
  
  // Initialize model manager with application directory
  _initGlobalLogStream();
  
  try {
    final dir = await getApplicationDocumentsDirectory();
    initModelManager(baseDir: dir.path);
    
    // Extract bundled model if it doesn't exist
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
    const NetworkLogsScreen(),
    const ModelsScreen(),
  ];

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Hydra P2P Node'),
        centerTitle: true,
      ),
      body: _screens[_currentIndex],
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
            icon: Icon(Icons.network_ping),
            label: 'Network',
          ),
          NavigationDestination(
            icon: Icon(Icons.smart_toy),
            label: 'AI Models',
          ),
        ],
      ),
    );
  }
}


class ConnectScreen extends StatefulWidget {
  const ConnectScreen({super.key});

  @override
  State<ConnectScreen> createState() => _ConnectScreenState();
}

// Global state for VPN connection
bool gIsVpnActive = false;

class _ConnectScreenState extends State<ConnectScreen> {
  static const platform = MethodChannel('com.hydra.network/vpn');

  @override
  void initState() {
    super.initState();
    platform.setMethodCallHandler((call) async {
      if (call.method == 'onVpnStarted') {
        final fd = call.arguments as int;
        if (fd != -1) {
          try {
            startVpnTunnel(fd: fd);
            debugPrint("Rust VPN tunnel started on FD: $fd");
          } catch (e) {
            debugPrint("Failed to start Rust VPN tunnel: $e");
          }
        }
      }
    });
  }

  void _toggleVpn() async {
    try {
      if (gIsVpnActive) {
        await platform.invokeMethod('stopVpn');
        stopVpnTunnel();
        setState(() {
          gIsVpnActive = false;
        });
      } else {
        // Start node logic
        final dir = await getApplicationDocumentsDirectory();
        await startHydraNode(baseDir: dir.path);
        final bool? result = await platform.invokeMethod('startVpn');
        if (result == true) {
          setState(() {
            gIsVpnActive = true;
          });
          // FD is handled by the platform channel listener setup in initState
        }
      }
    } on PlatformException catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('VPN Error: ${e.message}')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Error: $e')),
        );
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Container(
            width: 200,
            height: 200,
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: gIsVpnActive 
                  ? Colors.green.withOpacity(0.2)
                  : Theme.of(context).colorScheme.primaryContainer,
            ),
            child: IconButton(
              iconSize: 100,
              icon: Icon(
                gIsVpnActive ? Icons.power_settings_new : Icons.power_settings_new_outlined,
                color: gIsVpnActive 
                    ? Colors.green 
                    : Theme.of(context).colorScheme.onPrimaryContainer,
              ),
              onPressed: _toggleVpn,
            ),
          ),
          const SizedBox(height: 32),
          Text(
            gIsVpnActive ? 'Connected' : 'Disconnected',
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 16),
          Text(
            gIsVpnActive 
              ? 'System-wide VPN interception is active.\nTraffic is routed through Hydra network.'
              : 'System-wide VPN interception is offline.',
            textAlign: TextAlign.center,
          ),
        ],
      ),
    );
  }
}


class NetworkLogsScreen extends StatefulWidget {
  const NetworkLogsScreen({super.key});

  @override
  State<NetworkLogsScreen> createState() => _NetworkLogsScreenState();
}

class _NetworkLogsScreenState extends State<NetworkLogsScreen> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  @override
  Widget build(BuildContext context) {
    super.build(context);
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.all(16.0),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                'Live Node Activity',
                style: Theme.of(context).textTheme.titleLarge,
              ),
              IconButton(
                icon: const Icon(Icons.delete_outline),
                tooltip: 'Clear Logs',
                onPressed: () {
                  gNetworkLogs.clear();
                  gLogStreamController.add(gNetworkLogs);
                },
              ),
            ],
          ),
        ),
        Expanded(
          child: Container(
            color: Colors.black87,
            child: StreamBuilder<List<String>>(
              stream: gLogStreamController.stream,
              initialData: gNetworkLogs,
              builder: (context, snapshot) {
                final logs = snapshot.data ?? [];
                return ListView.builder(
                  reverse: true, // Newest at the bottom
                  padding: const EdgeInsets.all(8.0),
                  itemCount: logs.length,
                  itemBuilder: (context, index) {
                    final log = logs[index];
                    Color textColor = Colors.white70;
                    if (log.contains("[INFO]")) textColor = Colors.lightBlueAccent;
                    if (log.contains("[WARN]")) textColor = Colors.orangeAccent;
                    if (log.contains("[ERROR]")) textColor = Colors.redAccent;
                    
                    return Padding(
                      padding: const EdgeInsets.symmetric(vertical: 2.0),
                      child: Text(
                        log,
                        style: TextStyle(
                          fontFamily: 'monospace',
                          fontSize: 12,
                          color: textColor,
                        ),
                      ),
                    );
                  },
                );
              },
            ),
          ),
        ),
      ],
    );
  }
}

class ModelsScreen extends StatefulWidget {
  const ModelsScreen({super.key});

  @override
  State<ModelsScreen> createState() => _ModelsScreenState();
}

class _ModelsScreenState extends State<ModelsScreen> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  List<ModelInfo> _models = [];
  bool _isLoading = true;
  final Map<String, double> _downloadProgress = {};

  @override
  void initState() {
    super.initState();
    _loadModels();
  }

  Future<void> _loadModels() async {
    try {
      final models = await getAvailableModels();
      setState(() {
        _models = models;
        _isLoading = false;
      });
    } catch (e) {
      debugPrint("Error loading models: $e");
      setState(() {
        _isLoading = false;
      });
    }
  }

  void _downloadModel(String id) async {
    setState(() {
      _downloadProgress[id] = 0.0;
    });

    try {
      final stream = downloadModel(id: id);
      await for (final prog in stream) {
        setState(() {
          _downloadProgress[id] = prog;
        });
      }
      
      // Refresh models to update UI status
      await _loadModels();
      
      setState(() {
        _downloadProgress.remove(id);
      });
      
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Model downloaded successfully')),
        );
      }
    } catch (e) {
      setState(() {
        _downloadProgress.remove(id);
      });
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Download failed: $e')),
        );
      }
    }
  }

  void _activateModel(String id) async {
    try {
      await setActiveModel(id: id);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Active model set')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to set active model: $e')),
        );
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    if (_isLoading) {
      return const Center(child: CircularProgressIndicator());
    }

    return ListView.builder(
      itemCount: _models.length,
      padding: const EdgeInsets.all(16),
      itemBuilder: (context, index) {
        final model = _models[index];
        final progress = _downloadProgress[model.id];
        final isDownloading = progress != null;

        return Card(
          margin: const EdgeInsets.only(bottom: 16),
          child: Padding(
            padding: const EdgeInsets.all(16.0),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Text(
                      model.name,
                      style: Theme.of(context).textTheme.titleLarge,
                    ),
                    if (model.isDownloaded)
                      const Chip(
                        label: Text('Downloaded'),
                        backgroundColor: Colors.green,
                      ),
                  ],
                ),
                const SizedBox(height: 8),
                Text(model.description),
                const SizedBox(height: 8),
                Text('Size: ${model.sizeMb} MB'),
                const SizedBox(height: 16),
                if (isDownloading) ...[
                  LinearProgressIndicator(value: progress / 100),
                  const SizedBox(height: 8),
                  Text('${progress.toStringAsFixed(1)}% downloaded'),
                ] else if (model.isDownloaded) ...[
                  ElevatedButton(
                    onPressed: () => _activateModel(model.id),
                    child: const Text('Set as Active'),
                  ),
                ] else ...[
                  FilledButton.icon(
                    onPressed: () => _downloadModel(model.id),
                    icon: const Icon(Icons.download),
                    label: const Text('Download'),
                  ),
                ],
              ],
            ),
          ),
        );
      },
    );
  }
}
