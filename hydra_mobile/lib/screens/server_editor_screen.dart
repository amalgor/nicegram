import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/app/app_log.dart';
import 'package:hydra_mobile/app/models.dart';
import 'package:hydra_mobile/app/proxy_scope.dart';

/// Create or edit one SSH server (the `user@host -p N` part of `ssh -D`).
class ServerEditorScreen extends StatefulWidget {
  const ServerEditorScreen({super.key, this.server});

  final ServerInfo? server;

  @override
  State<ServerEditorScreen> createState() => _ServerEditorScreenState();
}

class _ServerEditorScreenState extends State<ServerEditorScreen> {
  final _form = GlobalKey<FormState>();
  late final _label = TextEditingController(text: widget.server?.label ?? '');
  late final _host = TextEditingController(text: widget.server?.host ?? '');
  late final _port = TextEditingController(text: '${widget.server?.port ?? 22}');
  late final _user = TextEditingController(text: widget.server?.username ?? '');
  final _password = TextEditingController();
  late String _authType = widget.server?.authType == 'password' ? 'password' : 'key';
  bool _showPassword = false;

  /// A key generated or pasted in this session; saved with the server.
  String? _newPrivateKey;
  SshKeyInfo? _newKeyInfo;

  bool _saving = false;
  bool _testing = false;
  SshTestResult? _testResult;

  ServerInfo? get _server => widget.server;
  bool get _isNew => _server == null;
  bool get _authChanged => _server != null && _server!.authType != _authType;

  String? get _publicKey => _newKeyInfo?.publicKey ?? (_authChanged ? null : _server?.publicKey);
  String? get _fingerprint => _newKeyInfo?.fingerprint ?? (_authChanged ? null : _server?.keyFingerprint);

  @override
  void initState() {
    super.initState();
    if (_isNew) WidgetsBinding.instance.addPostFrameCallback((_) => _generateKey(silent: true));
  }

  @override
  void dispose() {
    for (final c in [_label, _host, _port, _user, _password]) {
      c.dispose();
    }
    super.dispose();
  }

  // ---------------------------------------------------------------- actions

  void _generateKey({bool silent = false}) {
    try {
      final key = ProxyScope.read(context).generateKey('hydra-ios');
      setState(() {
        _newPrivateKey = key.privateKey;
        _newKeyInfo = key;
        _testResult = null;
      });
      if (!silent) _snack('New key generated. Add the public key to the server before saving.');
    } catch (e, st) {
      AppLog.error('editor', 'Key generation failed', e, st);
      _snack(describeError(e));
    }
  }

  Future<void> _pastePrivateKey() async {
    final clip = (await Clipboard.getData(Clipboard.kTextPlain))?.text ?? '';
    if (!mounted) return;
    final controller = TextEditingController(text: clip.contains('PRIVATE KEY') ? clip : '');
    final pem = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Private key'),
        content: TextField(
          controller: controller,
          maxLines: 8,
          autocorrect: false,
          enableSuggestions: false,
          style: const TextStyle(fontFamily: 'Menlo', fontSize: 11),
          decoration: const InputDecoration(
            hintText: '-----BEGIN OPENSSH PRIVATE KEY-----',
            border: OutlineInputBorder(),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(context, controller.text), child: const Text('Use key')),
        ],
      ),
    );
    controller.dispose();
    if (pem == null || pem.trim().isEmpty || !mounted) return;
    try {
      final info = ProxyScope.read(context).describeKey(pem);
      setState(() {
        _newPrivateKey = pem;
        _newKeyInfo = info;
        _testResult = null;
      });
    } catch (e) {
      _snack('Not a usable private key: ${describeError(e)}');
    }
  }

  Future<void> _pasteSshCommand() async {
    final clip = (await Clipboard.getData(Clipboard.kTextPlain))?.text ?? '';
    final parsed = parseSshTarget(clip);
    if (parsed == null) {
      _snack('Clipboard has no `ssh user@host` command');
      return;
    }
    setState(() => _applyTarget(parsed));
  }

  void _applyTarget(({String? user, String host, int? port}) target) {
    _host.text = target.host;
    if (target.user != null) _user.text = target.user!;
    if (target.port != null) _port.text = '${target.port}';
  }

  /// Accepts `user@host:port` typed straight into the host field.
  void _normalizeHost() {
    final text = _host.text.trim();
    if (!text.contains('@') && !text.contains(' ') && ':'.allMatches(text).length != 1) return;
    final parsed = parseSshTarget(text);
    if (parsed != null) _applyTarget(parsed);
  }

  String? get _credential {
    if (_authType == 'password') return _password.text.isEmpty ? null : _password.text;
    return _newPrivateKey;
  }

  bool _validate() {
    _normalizeHost();
    if (!(_form.currentState?.validate() ?? false)) return false;
    final needsCredential = _isNew || _authChanged || !(_server?.hasCredential ?? false);
    if (needsCredential && _credential == null) {
      _snack(_authType == 'password' ? 'Enter the password' : 'Generate or paste a private key');
      return false;
    }
    return true;
  }

  Future<void> _test() async {
    if (!_validate()) return;
    setState(() {
      _testing = true;
      _testResult = null;
    });
    try {
      final result = await ProxyScope.read(context).testServer(
        id: _authChanged ? null : _server?.id,
        host: _host.text.trim(),
        port: int.parse(_port.text),
        username: _user.text.trim(),
        authType: _authType,
        credential: _credential,
      );
      if (mounted) setState(() => _testResult = result);
    } catch (e) {
      if (mounted) setState(() => _testResult = SshTestResult(ok: false, error: describeError(e), elapsedMs: 0));
    } finally {
      if (mounted) setState(() => _testing = false);
    }
  }

  Future<void> _save() async {
    if (!_validate()) return;
    setState(() => _saving = true);
    final navigator = Navigator.of(context);
    final messenger = ScaffoldMessenger.of(context);
    try {
      final saved = await ProxyScope.read(context).saveServer(
        id: _server?.id,
        label: _label.text,
        host: _host.text.trim(),
        port: int.parse(_port.text),
        username: _user.text.trim(),
        authType: _authType,
        credential: _credential,
        activate: _isNew || (_server?.active ?? false),
      );
      messenger.showSnackBar(SnackBar(content: Text('Saved ${saved.label}')));
      navigator.pop();
    } catch (e, st) {
      AppLog.error('editor', 'Save failed', describeError(e), st);
      _snack(describeError(e));
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _delete() async {
    final server = _server!;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Delete ${server.label}?'),
        content: const Text('The stored password or private key is removed from this phone.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(context, true), child: const Text('Delete')),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    final navigator = Navigator.of(context);
    try {
      await ProxyScope.read(context).deleteServer(server);
      navigator.pop();
    } catch (e) {
      _snack(describeError(e));
    }
  }

  Future<void> _forgetHostKey() async {
    try {
      await ProxyScope.read(context).forgetHostKey(_server!);
      _snack('Host key forgotten. The next connection pins the new key.');
      setState(() {});
    } catch (e) {
      _snack(describeError(e));
    }
  }

  void _snack(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(message)));
  }

  // -------------------------------------------------------------------- UI

  @override
  Widget build(BuildContext context) {
    final current = _isNew
        ? null
        : ProxyScope.of(context).servers.where((s) => s.id == _server!.id).firstOrNull;
    return Scaffold(
      appBar: AppBar(
        title: Text(_isNew ? 'New server' : 'Edit server'),
        actions: [
          if (!_isNew)
            IconButton(tooltip: 'Delete server', icon: const Icon(Icons.delete_outline), onPressed: _delete),
          TextButton(onPressed: _saving ? null : _save, child: const Text('Save')),
        ],
      ),
      body: Form(
        key: _form,
        child: ListView(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 32),
          children: [
            _section(context, 'Connection'),
            TextFormField(
              controller: _host,
              decoration: InputDecoration(
                labelText: 'Server',
                hintText: 'example.com or user@example.com:22',
                border: const OutlineInputBorder(),
                suffixIcon: IconButton(
                  tooltip: 'Paste ssh command',
                  icon: const Icon(Icons.content_paste_go),
                  onPressed: _pasteSshCommand,
                ),
              ),
              keyboardType: TextInputType.url,
              autocorrect: false,
              textInputAction: TextInputAction.next,
              onEditingComplete: () {
                setState(_normalizeHost);
                FocusScope.of(context).nextFocus();
              },
              validator: (v) => (v ?? '').trim().isEmpty ? 'Required' : null,
            ),
            const SizedBox(height: 12),
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  flex: 3,
                  child: TextFormField(
                    controller: _user,
                    decoration: const InputDecoration(labelText: 'Username', border: OutlineInputBorder()),
                    autocorrect: false,
                    enableSuggestions: false,
                    textInputAction: TextInputAction.next,
                    validator: (v) => (v ?? '').trim().isEmpty ? 'Required' : null,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  flex: 2,
                  child: TextFormField(
                    controller: _port,
                    decoration: const InputDecoration(labelText: 'Port', border: OutlineInputBorder()),
                    keyboardType: TextInputType.number,
                    inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                    validator: (v) {
                      final port = int.tryParse(v ?? '');
                      return port == null || port < 1 || port > 65535 ? '1–65535' : null;
                    },
                  ),
                ),
              ],
            ),
            const SizedBox(height: 12),
            TextFormField(
              controller: _label,
              decoration: const InputDecoration(
                labelText: 'Name (optional)',
                hintText: 'Defaults to user@host',
                border: OutlineInputBorder(),
              ),
            ),
            _section(context, 'Authentication'),
            SegmentedButton<String>(
              segments: const [
                ButtonSegment(value: 'key', label: Text('SSH key'), icon: Icon(Icons.key)),
                ButtonSegment(value: 'password', label: Text('Password'), icon: Icon(Icons.password)),
              ],
              selected: {_authType},
              onSelectionChanged: (s) => setState(() {
                _authType = s.first;
                _testResult = null;
              }),
            ),
            const SizedBox(height: 12),
            if (_authType == 'password') _passwordField() else _keySection(context),
            if (current?.pinnedHostKey != null) ...[
              _section(context, 'Server identity'),
              Card(
                child: ListTile(
                  leading: const Icon(Icons.fingerprint),
                  title: const Text('Pinned host key'),
                  subtitle: SelectableText(current!.pinnedHostKey!, style: const TextStyle(fontFamily: 'Menlo', fontSize: 11)),
                  trailing: TextButton(onPressed: _forgetHostKey, child: const Text('Reset')),
                ),
              ),
            ],
            _section(context, 'Check'),
            OutlinedButton.icon(
              onPressed: _testing ? null : _test,
              icon: _testing
                  ? const SizedBox.square(dimension: 18, child: CircularProgressIndicator(strokeWidth: 2))
                  : const Icon(Icons.network_check),
              label: Text(_testing ? 'Testing…' : 'Test connection'),
            ),
            if (_testResult != null) ...[
              const SizedBox(height: 12),
              _TestResultCard(result: _testResult!),
            ],
          ],
        ),
      ),
    );
  }

  Widget _passwordField() {
    final keepsStored = !_isNew && !_authChanged && (_server?.hasCredential ?? false);
    return TextFormField(
      controller: _password,
      obscureText: !_showPassword,
      autocorrect: false,
      enableSuggestions: false,
      decoration: InputDecoration(
        labelText: 'Password',
        helperText: keepsStored ? 'Leave empty to keep the saved password' : null,
        border: const OutlineInputBorder(),
        suffixIcon: IconButton(
          tooltip: _showPassword ? 'Hide password' : 'Show password',
          icon: Icon(_showPassword ? Icons.visibility_off : Icons.visibility),
          onPressed: () => setState(() => _showPassword = !_showPassword),
        ),
      ),
    );
  }

  Widget _keySection(BuildContext context) {
    final text = Theme.of(context).textTheme;
    final publicKey = _publicKey;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (publicKey != null)
          Card(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(16, 12, 8, 12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Expanded(child: Text('Public key', style: text.labelLarge)),
                      IconButton(
                        tooltip: 'Copy public key',
                        icon: const Icon(Icons.copy),
                        onPressed: () {
                          Clipboard.setData(ClipboardData(text: publicKey));
                          _snack('Public key copied');
                        },
                      ),
                    ],
                  ),
                  SelectableText(publicKey, style: const TextStyle(fontFamily: 'Menlo', fontSize: 11)),
                  if (_fingerprint != null) ...[
                    const SizedBox(height: 6),
                    Text(_fingerprint!, style: text.bodySmall),
                  ],
                  const SizedBox(height: 8),
                  Text(
                    _newPrivateKey != null
                        ? 'Append this line to ~/.ssh/authorized_keys on the server, then tap Test connection.'
                        : 'This key is saved on the phone.',
                    style: text.bodySmall,
                  ),
                ],
              ),
            ),
          )
        else
          Text(
            _server?.authType == 'key_file'
                ? 'This server uses a key file from the config. Generate or paste a key to replace it.'
                : 'No key yet. Generate a new one or paste an existing private key.',
            style: text.bodyMedium,
          ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            OutlinedButton.icon(onPressed: () => _generateKey(), icon: const Icon(Icons.autorenew), label: const Text('Generate new key')),
            OutlinedButton.icon(onPressed: _pastePrivateKey, icon: const Icon(Icons.content_paste), label: const Text('Paste private key')),
          ],
        ),
      ],
    );
  }

  Widget _section(BuildContext context, String title) => Padding(
        padding: const EdgeInsets.fromLTRB(4, 24, 4, 8),
        child: Text(title, style: Theme.of(context).textTheme.titleSmall?.copyWith(color: Theme.of(context).colorScheme.primary)),
      );
}

class _TestResultCard extends StatelessWidget {
  const _TestResultCard({required this.result});

  final SshTestResult result;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final ok = result.ok;
    final fg = ok ? scheme.onPrimaryContainer : scheme.onErrorContainer;
    return Card(
      color: ok ? scheme.primaryContainer : scheme.errorContainer,
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(ok ? Icons.check_circle : Icons.error_outline, color: fg),
                const SizedBox(width: 8),
                Text(
                  ok ? 'Connected in ${result.elapsedMs} ms' : 'Connection failed',
                  style: TextStyle(color: fg, fontWeight: FontWeight.w600),
                ),
              ],
            ),
            if (result.error != null) ...[
              const SizedBox(height: 8),
              SelectableText(result.error!, style: TextStyle(color: fg)),
            ],
            if (result.tunnelCheck != null) ...[
              const SizedBox(height: 8),
              Text('Tunnel: ${result.tunnelCheck}', style: TextStyle(color: fg)),
            ],
            if (result.hostKeyFingerprint != null) ...[
              const SizedBox(height: 8),
              SelectableText('Host key ${result.hostKeyFingerprint}', style: TextStyle(color: fg, fontFamily: 'Menlo', fontSize: 11)),
            ],
          ],
        ),
      ),
    );
  }
}
