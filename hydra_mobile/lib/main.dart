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
    const ContentScreen(),
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
          NavigationDestination(
            icon: Icon(Icons.article),
            label: 'Content',
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


class ContentScreen extends StatefulWidget {
  const ContentScreen({super.key});

  @override
  State<ContentScreen> createState() => _ContentScreenState();
}

class _ContentScreenState extends State<ContentScreen> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  // Auth state: disconnected, need_phone, need_code, need_password:<hint>, authorized:<name>, error:<msg>
  String _authState = 'disconnected';
  final TextEditingController _phoneController = TextEditingController();
  final TextEditingController _codeController = TextEditingController();
  final TextEditingController _passwordController = TextEditingController();
  bool _isLoading = false;
  String? _errorMessage;

  // Dialogs loaded after auth
  List<dynamic> _dialogs = [];

  String get _authStatus {
    if (_authState.startsWith('authorized:')) return _authState.substring(11);
    return _authState;
  }

  bool get _isAuthorized => _authState.startsWith('authorized:');
  bool get _needsPhone => _authState == 'need_phone';
  bool get _needsCode => _authState == 'need_code';
  bool get _needsPassword => _authState.startsWith('need_password:');

  String get _passwordHint {
    if (_authState.startsWith('need_password:')) {
      return _authState.substring(14);
    }
    return '';
  }

  void _setLoading(bool v) => setState(() { _isLoading = v; _errorMessage = null; });
  void _setError(String msg) => setState(() { _isLoading = false; _errorMessage = msg; });

  Future<void> _connectTelegram() async {
    _setLoading(true);
    try {
      // TODO: These calls require flutter_rust_bridge_codegen to generate Dart bindings
      // from hydra_mobile/rust/src/api/content.rs
      // For now, show a placeholder message
      final dir = await getApplicationDocumentsDirectory();
      // await initContentEngine(baseDir: dir.path);
      // final state = await telegramConnect();
      // setState(() { _authState = state; _isLoading = false; });
      setState(() {
        _isLoading = false;
        _errorMessage = 'Bridge codegen required. Run: flutter_rust_bridge_codegen generate';
      });
    } catch (e) {
      _setError('$e');
    }
  }

  Future<void> _sendPhone() async {
    if (_phoneController.text.isEmpty) return;
    _setLoading(true);
    try {
      // final state = await telegramSendPhone(phone: _phoneController.text);
      // setState(() { _authState = state; _isLoading = false; });
      _setError('Bridge codegen required');
    } catch (e) {
      _setError('$e');
    }
  }

  Future<void> _sendCode() async {
    if (_codeController.text.isEmpty) return;
    _setLoading(true);
    try {
      // final state = await telegramSendCode(code: _codeController.text);
      // setState(() { _authState = state; _isLoading = false; });
      _setError('Bridge codegen required');
    } catch (e) {
      _setError('$e');
    }
  }

  Future<void> _sendPassword() async {
    if (_passwordController.text.isEmpty) return;
    _setLoading(true);
    try {
      // final state = await telegramSendPassword(password: _passwordController.text);
      // setState(() { _authState = state; _isLoading = false; });
      _setError('Bridge codegen required');
    } catch (e) {
      _setError('$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);

    if (_isLoading) {
      return const Center(child: CircularProgressIndicator());
    }

    if (_isAuthorized) {
      return _buildContentView();
    }

    return _buildAuthView();
  }

  Widget _buildAuthView() {
    return Padding(
      padding: const EdgeInsets.all(24.0),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Icon(
            Icons.telegram,
            size: 64,
            color: Theme.of(context).colorScheme.primary,
          ),
          const SizedBox(height: 16),
          Text(
            'Telegram Content Intelligence',
            style: Theme.of(context).textTheme.headlineSmall,
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: 8),
          Text(
            'Connect your Telegram account to enable TLDR folding, summarization, and attention tracking.',
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.bodyMedium,
          ),
          const SizedBox(height: 32),

          if (_errorMessage != null) ...[
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: Colors.red.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(8),
              ),
              child: Text(
                _errorMessage!,
                style: const TextStyle(color: Colors.redAccent),
              ),
            ),
            const SizedBox(height: 16),
          ],

          if (_authState == 'disconnected') ...[
            FilledButton.icon(
              onPressed: _connectTelegram,
              icon: const Icon(Icons.link),
              label: const Text('Connect to Telegram'),
            ),
          ],

          if (_needsPhone) ...[
            TextField(
              controller: _phoneController,
              decoration: const InputDecoration(
                labelText: 'Phone Number',
                hintText: '+1 234 567 8900',
                prefixIcon: Icon(Icons.phone),
              ),
              keyboardType: TextInputType.phone,
            ),
            const SizedBox(height: 16),
            FilledButton(
              onPressed: _sendPhone,
              child: const Text('Send Code'),
            ),
          ],

          if (_needsCode) ...[
            TextField(
              controller: _codeController,
              decoration: const InputDecoration(
                labelText: 'Login Code',
                hintText: 'Enter code from Telegram',
                prefixIcon: Icon(Icons.lock_outline),
              ),
              keyboardType: TextInputType.number,
            ),
            const SizedBox(height: 16),
            FilledButton(
              onPressed: _sendCode,
              child: const Text('Verify Code'),
            ),
          ],

          if (_needsPassword) ...[
            Text('2FA Password required (hint: $_passwordHint)'),
            const SizedBox(height: 8),
            TextField(
              controller: _passwordController,
              decoration: const InputDecoration(
                labelText: 'Password',
                prefixIcon: Icon(Icons.key),
              ),
              obscureText: true,
            ),
            const SizedBox(height: 16),
            FilledButton(
              onPressed: _sendPassword,
              child: const Text('Submit Password'),
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildContentView() {
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.all(16.0),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Content Intelligence',
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  Text(
                    'Signed in as $_authStatus',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ],
              ),
              IconButton(
                icon: const Icon(Icons.logout),
                tooltip: 'Disconnect',
                onPressed: () {
                  setState(() {
                    _authState = 'disconnected';
                    _dialogs = [];
                  });
                },
              ),
            ],
          ),
        ),
        Expanded(
          child: _dialogs.isEmpty
              ? Center(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      Icon(Icons.chat_bubble_outline, size: 48, color: Colors.grey),
                      const SizedBox(height: 16),
                      const Text('No dialogs loaded yet.'),
                      const SizedBox(height: 8),
                      FilledButton.icon(
                        onPressed: () {
                          // TODO: await telegramGetDialogs() after codegen
                          ScaffoldMessenger.of(context).showSnackBar(
                            const SnackBar(content: Text('Bridge codegen required for dialog loading')),
                          );
                        },
                        icon: const Icon(Icons.refresh),
                        label: const Text('Load Dialogs'),
                      ),
                    ],
                  ),
                )
              : ListView.builder(
                  itemCount: _dialogs.length,
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  itemBuilder: (context, index) {
                    final dialog = _dialogs[index];
                    return Card(
                      margin: const EdgeInsets.only(bottom: 8),
                      child: ListTile(
                        leading: CircleAvatar(
                          child: Text(
                            (dialog['title'] as String? ?? '?')[0].toUpperCase(),
                          ),
                        ),
                        title: Text(dialog['title'] ?? 'Unknown'),
                        subtitle: Text(
                          dialog['is_private'] == true ? '[PRIVATE]' : '[PUBLIC]',
                        ),
                        trailing: const Icon(Icons.chevron_right),
                      ),
                    );
                  },
                ),
        ),
      ],
    );
  }
}
