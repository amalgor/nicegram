import 'package:flutter/material.dart';
import 'package:hydra_mobile/src/rust/api/model_manager.dart';

class ModelsScreen extends StatefulWidget {
  const ModelsScreen({super.key});

  @override
  State<ModelsScreen> createState() => _ModelsScreenState();
}

class _ModelsScreenState extends State<ModelsScreen> with TickerProviderStateMixin {
  late TabController _tabController;

  @override
  void initState() {
    super.initState();
    _tabController = TabController(length: 2, vsync: this);
  }

  @override
  void dispose() {
    _tabController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        TabBar(
          controller: _tabController,
          tabs: const [
            Tab(icon: Icon(Icons.download), text: 'Models'),
            Tab(icon: Icon(Icons.chat), text: 'Chat'),
          ],
        ),
        Expanded(
          child: TabBarView(
            controller: _tabController,
            children: const [
              _ModelsTab(),
              _ChatTab(),
            ],
          ),
        ),
      ],
    );
  }
}

class _ModelsTab extends StatefulWidget {
  const _ModelsTab();

  @override
  State<_ModelsTab> createState() => _ModelsTabState();
}

class _ModelsTabState extends State<_ModelsTab> with AutomaticKeepAliveClientMixin {
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
      setState(() { _models = models; _isLoading = false; });
    } catch (e) {
      debugPrint("Error loading models: $e");
      setState(() { _isLoading = false; });
    }
  }

  void _downloadModel(String id) async {
    setState(() { _downloadProgress[id] = 0.0; });
    try {
      final stream = downloadModel(id: id);
      await for (final prog in stream) {
        setState(() { _downloadProgress[id] = prog; });
      }
      await _loadModels();
      setState(() { _downloadProgress.remove(id); });
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Model downloaded')));
      }
    } catch (e) {
      setState(() { _downloadProgress.remove(id); });
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Download failed: $e')));
      }
    }
  }

  void _activateModel(String id) async {
    try {
      await setActiveModel(id: id);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Model activated')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Activation failed: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    if (_isLoading) return const Center(child: CircularProgressIndicator());

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
                    Expanded(child: Text(model.name, style: Theme.of(context).textTheme.titleLarge)),
                    if (model.isDownloaded)
                      const Chip(label: Text('OK'), backgroundColor: Colors.green),
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
                  Text('${progress.toStringAsFixed(1)}%'),
                ] else if (model.isDownloaded) ...[
                  ElevatedButton(onPressed: () => _activateModel(model.id), child: const Text('Activate')),
                ] else ...[
                  FilledButton.icon(onPressed: () => _downloadModel(model.id), icon: const Icon(Icons.download), label: const Text('Download')),
                ],
              ],
            ),
          ),
        );
      },
    );
  }
}

class _ChatTab extends StatefulWidget {
  const _ChatTab();

  @override
  State<_ChatTab> createState() => _ChatTabState();
}

class _ChatTabState extends State<_ChatTab> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  final TextEditingController _inputController = TextEditingController();
  final ScrollController _scrollController = ScrollController();
  final List<_ChatMessage> _messages = [];
  bool _isGenerating = false;
  bool _modelLoaded = false;

  @override
  void initState() {
    super.initState();
    _checkModelStatus();
  }

  @override
  void dispose() {
    _inputController.dispose();
    _scrollController.dispose();
    super.dispose();
  }

  Future<void> _checkModelStatus() async {
    try {
      final loaded = await isModelLoaded();
      setState(() { _modelLoaded = loaded; });
    } catch (e) {
      debugPrint("Error checking model status: $e");
    }
  }

  Future<void> _sendMessage() async {
    final text = _inputController.text.trim();
    if (text.isEmpty || _isGenerating) return;

    debugPrint('Chat send requested: len=${text.length}');
    _inputController.clear();
    setState(() {
      _messages.add(_ChatMessage(text: text, isUser: true));
      _isGenerating = true;
    });
    _scrollToBottom();

    try {
      debugPrint('Calling chatWithModel...');
      final response = await chatWithModel(prompt: text, maxTokens: 256);
      debugPrint('chatWithModel returned: len=${response.length}');
      setState(() {
        _messages.add(_ChatMessage(text: response, isUser: false));
        _isGenerating = false;
      });
      _scrollToBottom();
    } catch (e) {
      debugPrint('Chat generation failed: $e');
      setState(() {
        _messages.add(_ChatMessage(text: 'Model error: $e', isUser: false, isError: true));
        _isGenerating = false;
      });
      _scrollToBottom();
    }
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scrollController.hasClients) {
        _scrollController.animateTo(
          _scrollController.position.maxScrollExtent,
          duration: const Duration(milliseconds: 200),
          curve: Curves.easeOut,
        );
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);

    return Column(
      children: [
        Expanded(
          child: _messages.isEmpty
              ? Center(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      Icon(Icons.chat_bubble_outline, size: 64, color: Colors.grey[400]),
                      const SizedBox(height: 16),
                      Text(
                        _modelLoaded ? 'Start a conversation' : 'No model loaded',
                        style: TextStyle(color: Colors.grey[600], fontSize: 16),
                      ),
                      if (!_modelLoaded) ...[
                        const SizedBox(height: 8),
                        TextButton(
                          onPressed: _checkModelStatus,
                          child: const Text('Refresh status'),
                        ),
                      ],
                    ],
                  ),
                )
              : ListView.builder(
                  controller: _scrollController,
                  padding: const EdgeInsets.all(16),
                  itemCount: _messages.length + (_isGenerating ? 1 : 0),
                  itemBuilder: (context, index) {
                    if (index == _messages.length && _isGenerating) {
                      return const Padding(
                        padding: EdgeInsets.symmetric(vertical: 8),
                        child: Row(
                          children: [
                            SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)),
                            SizedBox(width: 8),
                            Text('Generating...'),
                          ],
                        ),
                      );
                    }
                    final msg = _messages[index];
                    return _buildMessageBubble(msg);
                  },
                ),
        ),
        Container(
          padding: const EdgeInsets.all(8),
          decoration: BoxDecoration(
            color: Theme.of(context).colorScheme.surface,
            boxShadow: [BoxShadow(color: Colors.black12, blurRadius: 4)],
          ),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _inputController,
                  decoration: const InputDecoration(
                    hintText: 'Type a message...',
                    border: OutlineInputBorder(),
                    contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                  ),
                  maxLines: 3,
                  minLines: 1,
                  textInputAction: TextInputAction.send,
                  onSubmitted: (_) => _sendMessage(),
                ),
              ),
              const SizedBox(width: 8),
              IconButton.filled(
                onPressed: _isGenerating ? null : _sendMessage,
                icon: const Icon(Icons.send),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _buildMessageBubble(_ChatMessage msg) {
    final isUser = msg.isUser;
    return Align(
      alignment: isUser ? Alignment.centerRight : Alignment.centerLeft,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 4),
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        constraints: BoxConstraints(maxWidth: MediaQuery.of(context).size.width * 0.8),
        decoration: BoxDecoration(
          color: msg.isError
              ? Colors.red[100]
              : isUser
                  ? Theme.of(context).colorScheme.primaryContainer
                  : Theme.of(context).colorScheme.secondaryContainer,
          borderRadius: BorderRadius.circular(12),
        ),
        child: SelectableText(
          msg.text,
          style: TextStyle(
            color: msg.isError ? Colors.red[900] : null,
          ),
        ),
      ),
    );
  }
}

class _ChatMessage {
  final String text;
  final bool isUser;
  final bool isError;

  _ChatMessage({required this.text, required this.isUser, this.isError = false});
}
