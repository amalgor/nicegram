import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:hydra_mobile/src/rust/api/content.dart';
import 'package:hydra_mobile/widgets/message_card.dart';

class ContentScreen extends StatefulWidget {
  const ContentScreen({super.key});

  @override
  State<ContentScreen> createState() => _ContentScreenState();
}

class _ContentScreenState extends State<ContentScreen> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  String _authState = 'disconnected';
  final TextEditingController _phoneController = TextEditingController();
  final TextEditingController _codeController = TextEditingController();
  final TextEditingController _passwordController = TextEditingController();
  bool _isLoading = false;
  String? _errorMessage;
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
    if (_authState.startsWith('need_password:')) return _authState.substring(14);
    return '';
  }

  void _setLoading(bool v) => setState(() { _isLoading = v; _errorMessage = null; });
  void _setError(String msg) => setState(() { _isLoading = false; _errorMessage = msg; });

  Future<void> _connectTelegram() async {
    _setLoading(true);
    try {
      final dir = await getApplicationDocumentsDirectory();
      await initContentEngine(baseDir: dir.path);
      final state = await telegramConnect();
      setState(() { _authState = state; _isLoading = false; });
    } catch (e) {
      _setError('$e');
    }
  }

  Future<void> _sendPhone() async {
    if (_phoneController.text.isEmpty) return;
    _setLoading(true);
    try {
      final state = await telegramSendPhone(phone: _phoneController.text);
      setState(() { _authState = state; _isLoading = false; });
    } catch (e) {
      _setError('$e');
    }
  }

  Future<void> _sendCode() async {
    if (_codeController.text.isEmpty) return;
    _setLoading(true);
    try {
      final state = await telegramSendCode(code: _codeController.text);
      setState(() { _authState = state; _isLoading = false; });
    } catch (e) {
      _setError('$e');
    }
  }

  Future<void> _sendPassword() async {
    if (_passwordController.text.isEmpty) return;
    _setLoading(true);
    try {
      final state = await telegramSendPassword(password: _passwordController.text);
      setState(() { _authState = state; _isLoading = false; });
    } catch (e) {
      _setError('$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    if (_isLoading) return const Center(child: CircularProgressIndicator());
    if (_isAuthorized) return _buildContentView();
    return _buildAuthView();
  }

  Widget _buildAuthView() {
    return Padding(
      padding: const EdgeInsets.all(24.0),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Icon(Icons.telegram, size: 64, color: Theme.of(context).colorScheme.primary),
          const SizedBox(height: 16),
          Text('Telegram Content Intelligence',
            style: Theme.of(context).textTheme.headlineSmall, textAlign: TextAlign.center),
          const SizedBox(height: 8),
          Text('Connect your Telegram account to enable TLDR folding, summarization, and attention tracking.',
            textAlign: TextAlign.center, style: Theme.of(context).textTheme.bodyMedium),
          const SizedBox(height: 32),
          if (_errorMessage != null) ...[
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: Colors.red.withValues(alpha: 0.1),
                borderRadius: BorderRadius.circular(8),
              ),
              child: Text(_errorMessage!, style: const TextStyle(color: Colors.redAccent)),
            ),
            const SizedBox(height: 16),
          ],
          if (_authState == 'disconnected')
            FilledButton.icon(onPressed: _connectTelegram, icon: const Icon(Icons.link), label: const Text('Connect to Telegram')),
          if (_needsPhone) ...[
            TextField(controller: _phoneController, decoration: const InputDecoration(labelText: 'Phone Number', hintText: '+1 234 567 8900', prefixIcon: Icon(Icons.phone)), keyboardType: TextInputType.phone),
            const SizedBox(height: 16),
            FilledButton(onPressed: _sendPhone, child: const Text('Send Code')),
          ],
          if (_needsCode) ...[
            TextField(controller: _codeController, decoration: const InputDecoration(labelText: 'Login Code', hintText: 'Enter code from Telegram', prefixIcon: Icon(Icons.lock_outline)), keyboardType: TextInputType.number),
            const SizedBox(height: 16),
            FilledButton(onPressed: _sendCode, child: const Text('Verify Code')),
          ],
          if (_needsPassword) ...[
            Text('2FA Password required (hint: $_passwordHint)'),
            const SizedBox(height: 8),
            TextField(controller: _passwordController, decoration: const InputDecoration(labelText: 'Password', prefixIcon: Icon(Icons.key)), obscureText: true),
            const SizedBox(height: 16),
            FilledButton(onPressed: _sendPassword, child: const Text('Submit Password')),
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
              Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                Text('Content Intelligence', style: Theme.of(context).textTheme.titleLarge),
                Text('Signed in as $_authStatus', style: Theme.of(context).textTheme.bodySmall),
              ]),
              IconButton(
                icon: const Icon(Icons.logout),
                tooltip: 'Disconnect',
                onPressed: () async {
                  try { await telegramDisconnect(); } catch (_) {}
                  setState(() { _authState = 'disconnected'; _dialogs = []; });
                },
              ),
            ],
          ),
        ),
        Expanded(
          child: _dialogs.isEmpty
              ? Center(child: Column(mainAxisAlignment: MainAxisAlignment.center, children: [
                  Icon(Icons.chat_bubble_outline, size: 48, color: Colors.grey),
                  const SizedBox(height: 16),
                  const Text('No dialogs loaded yet.'),
                  const SizedBox(height: 8),
                  FilledButton.icon(
                    onPressed: () async {
                      try {
                        final json = await telegramGetDialogs();
                        final list = jsonDecode(json) as List<dynamic>;
                        setState(() { _dialogs = list; });
                      } catch (e) {
                        if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Error: $e')));
                      }
                    },
                    icon: const Icon(Icons.refresh),
                    label: const Text('Load Dialogs'),
                  ),
                ]))
              : ListView.builder(
                  itemCount: _dialogs.length,
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  itemBuilder: (context, index) {
                    final dialog = _dialogs[index];
                    final rawTitle = dialog['title'] as String?;
                    final displayTitle = (rawTitle == null || rawTitle.trim().isEmpty)
                        ? 'Chat ${dialog['chat_id']}'
                        : rawTitle.trim();
                    final initial = displayTitle.isNotEmpty
                        ? displayTitle.characters.first.toUpperCase()
                        : '?';
                    return Card(
                      margin: const EdgeInsets.only(bottom: 8),
                      child: ListTile(
                        leading: CircleAvatar(child: Text(initial)),
                        title: Text(displayTitle),
                        subtitle: Text(dialog['is_private'] == true ? '[PRIVATE]' : '[PUBLIC]'),
                        trailing: const Icon(Icons.chevron_right),
                        onTap: () => _openChat(dialog),
                      ),
                    );
                  },
                ),
        ),
      ],
    );
  }

  void _openChat(Map<String, dynamic> dialog) {
    final chatId = dialog['chat_id'] as int;
    final title = dialog['title'] as String? ?? 'Chat $chatId';
    Navigator.of(context).push(MaterialPageRoute(
      builder: (_) => ChatMessagesScreen(chatId: chatId, title: title),
    ));
  }
}

class ChatMessagesScreen extends StatefulWidget {
  final int chatId;
  final String title;

  const ChatMessagesScreen({super.key, required this.chatId, required this.title});

  @override
  State<ChatMessagesScreen> createState() => _ChatMessagesScreenState();
}

class _ChatMessagesScreenState extends State<ChatMessagesScreen> {
  List<dynamic> _messages = [];
  bool _isLoading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _loadMessages();
  }

  Future<void> _loadMessages() async {
    setState(() { _isLoading = true; _error = null; });
    try {
      final json = await fetchChannelMessages(chatId: widget.chatId, limit: 20);
      final list = jsonDecode(json) as List<dynamic>;
      setState(() { _messages = list; _isLoading = false; });
    } catch (e) {
      setState(() { _error = '$e'; _isLoading = false; });
    }
  }

  void _onAttention(String messageId, int depth, int readTimeMs) async {
    try {
      final interaction = depth >= 2 ? 'deep_dive' : (depth >= 1 ? 'read' : 'skim');
      await recordAttention(
        messageId: messageId,
        chatId: widget.chatId,
        maxDepth: depth,
        readTimeMs: BigInt.from(readTimeMs),
        interaction: interaction,
      );
    } catch (_) {}
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(widget.title)),
      body: _isLoading
          ? const Center(child: CircularProgressIndicator())
          : _error != null
              ? Center(child: Column(mainAxisAlignment: MainAxisAlignment.center, children: [
                  const Icon(Icons.error_outline, size: 48, color: Colors.red),
                  const SizedBox(height: 16),
                  Text(_error!, style: const TextStyle(color: Colors.redAccent), textAlign: TextAlign.center),
                  const SizedBox(height: 16),
                  FilledButton(onPressed: _loadMessages, child: const Text('Retry')),
                ]))
              : _messages.isEmpty
                  ? const Center(child: Text('No messages found'))
                  : RefreshIndicator(
                      onRefresh: _loadMessages,
                      child: ListView.builder(
                        itemCount: _messages.length,
                        itemBuilder: (context, index) {
                          final msg = _messages[index] as Map<String, dynamic>;
                          return FoldableMessageCard(
                            message: msg,
                            onAttention: _onAttention,
                          );
                        },
                      ),
                    ),
    );
  }
}
