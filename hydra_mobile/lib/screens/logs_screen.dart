import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/app/app_log.dart';
import 'package:hydra_mobile/app/models.dart';
import 'package:hydra_mobile/app/native_bridge.dart';
import 'package:hydra_mobile/logging/log_store.dart';
import 'package:hydra_mobile/src/rust/api/diagnostics.dart' as diagnostics_api;

class _Verbosity {
  const _Verbosity(this.label, this.minLevel);
  final String label;
  final LogLevel minLevel;
}

const _verbosities = <_Verbosity>[
  _Verbosity('Errors', LogLevel.error),
  _Verbosity('Warnings', LogLevel.warn),
  _Verbosity('Info', LogLevel.info),
  _Verbosity('Debug', LogLevel.debug),
  _Verbosity('Trace', LogLevel.trace),
];

const int _defaultVerbosityIndex = 2;
const int _maxShown = 5000;

Color _levelColor(LogLevel level, ColorScheme scheme) {
  final dark = scheme.brightness == Brightness.dark;
  return switch (level) {
    LogLevel.error => dark ? const Color(0xFFF87171) : const Color(0xFFB91C1C),
    LogLevel.warn => dark ? const Color(0xFFFBBF24) : const Color(0xFFB45309),
    LogLevel.info => scheme.onSurface,
    LogLevel.debug || LogLevel.trace => scheme.onSurfaceVariant,
    LogLevel.other => scheme.onSurfaceVariant,
  };
}

class LogsScreen extends StatefulWidget {
  const LogsScreen({super.key});

  @override
  State<LogsScreen> createState() => _LogsScreenState();
}

class _LogsScreenState extends State<LogsScreen> {
  final LogStore _store = LogStore.instance;

  // reverse:true => offset 0 is the newest line. Following the tail means
  // staying pinned at offset 0.
  final ScrollController _scrollController = ScrollController();
  bool _following = true;

  int _verbosityIndex = _defaultVerbosityIndex;
  bool _sshOnly = false;
  String _search = '';
  bool _sharing = false;

  @override
  void initState() {
    super.initState();
    _store.addListener(_onStoreChanged);
    _scrollController.addListener(_onScroll);
  }

  @override
  void dispose() {
    _store.removeListener(_onStoreChanged);
    _scrollController.dispose();
    super.dispose();
  }

  void _onStoreChanged() {
    if (mounted) setState(() {});
  }

  void _onScroll() {
    final atTail = _scrollController.position.pixels <= 8;
    if (atTail != _following) setState(() => _following = atTail);
  }

  void _jumpToTail() {
    setState(() => _following = true);
    if (_scrollController.hasClients) _scrollController.jumpTo(0);
  }

  /// Newest first, capped at [_maxShown] so filtering stays cheap.
  List<LogRecord> get _filtered {
    final minRank = _verbosities[_verbosityIndex].minLevel.index;
    final search = _search.toLowerCase();
    final out = <LogRecord>[];
    for (var i = _store.length - 1; i >= 0 && out.length < _maxShown; i--) {
      final r = _store.at(i);
      if (r.level != LogLevel.other && r.level.index > minRank) continue;
      if (_sshOnly && !r.isSsh) continue;
      if (search.isNotEmpty && !r.raw.toLowerCase().contains(search)) continue;
      out.add(r);
    }
    return out;
  }

  Future<void> _share() async {
    setState(() => _sharing = true);
    try {
      final device = await NativeBridge.instance.deviceInfo();
      final report = await diagnostics_api.writeDiagnosticsReport(appInfo: jsonEncode(device));
      final files = [report, ...diagnostics_api.listLogFiles().take(3)];
      AppLog.info('logs', 'Sharing ${files.length} diagnostics files');
      final shared = await NativeBridge.instance.shareFiles(files, text: 'Hydra diagnostics');
      if (!shared && mounted) {
        await Clipboard.setData(ClipboardData(text: files.join('\n')));
        _snack('Sharing is unavailable here. File paths copied to the clipboard.');
      }
    } catch (e, st) {
      AppLog.error('logs', 'Could not export diagnostics', describeError(e), st);
      _snack('Export failed: ${describeError(e)}');
    } finally {
      if (mounted) setState(() => _sharing = false);
    }
  }

  void _copyShown(List<LogRecord> logs) {
    Clipboard.setData(ClipboardData(text: logs.reversed.map((r) => r.raw).join('\n')));
    _snack('${logs.length} lines copied');
  }

  void _snack(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(message), duration: const Duration(seconds: 2)));
  }

  @override
  Widget build(BuildContext context) {
    final logs = _filtered;
    final scheme = Theme.of(context).colorScheme;

    return Scaffold(
      appBar: AppBar(
        title: const Text('Logs'),
        actions: [
          IconButton(
            tooltip: 'Share diagnostics',
            onPressed: _sharing ? null : _share,
            icon: _sharing
                ? const SizedBox.square(dimension: 20, child: CircularProgressIndicator(strokeWidth: 2))
                : const Icon(Icons.ios_share),
          ),
          PopupMenuButton<String>(
            onSelected: (value) {
              if (value == 'copy') _copyShown(logs);
              if (value == 'clear') _store.clear();
            },
            itemBuilder: (_) => const [
              PopupMenuItem(value: 'copy', child: ListTile(leading: Icon(Icons.copy), title: Text('Copy shown lines'))),
              PopupMenuItem(value: 'clear', child: ListTile(leading: Icon(Icons.delete_sweep_outlined), title: Text('Clear view'))),
            ],
          ),
        ],
      ),
      body: Column(
        children: [
          _controls(context),
          const Divider(height: 1),
          Expanded(
            child: logs.isEmpty
                ? Center(child: Text('No matching log lines', style: TextStyle(color: scheme.onSurfaceVariant)))
                : Stack(
                    children: [
                      SelectionArea(
                        child: ListView.builder(
                          controller: _scrollController,
                          reverse: true,
                          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
                          itemCount: logs.length,
                          itemBuilder: (context, index) => _LogLine(logs[index]),
                        ),
                      ),
                      if (!_following)
                        Positioned(
                          right: 16,
                          bottom: 16,
                          child: FloatingActionButton.small(
                            onPressed: _jumpToTail,
                            tooltip: 'Jump to latest',
                            child: const Icon(Icons.arrow_downward),
                          ),
                        ),
                    ],
                  ),
          ),
        ],
      ),
    );
  }

  Widget _controls(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 4, 12, 8),
      child: Column(
        children: [
          SearchBar(
            hintText: 'Filter',
            leading: const Icon(Icons.search),
            elevation: const WidgetStatePropertyAll(0),
            constraints: const BoxConstraints(minHeight: 44),
            onChanged: (v) => setState(() => _search = v),
          ),
          const SizedBox(height: 8),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(
              children: [
                for (var i = 0; i < _verbosities.length; i++)
                  Padding(
                    padding: const EdgeInsets.only(right: 6),
                    child: ChoiceChip(
                      label: Text(_verbosities[i].label),
                      selected: i == _verbosityIndex,
                      onSelected: (_) => setState(() => _verbosityIndex = i),
                    ),
                  ),
                const SizedBox(width: 6),
                FilterChip(
                  label: const Text('SSH only'),
                  selected: _sshOnly,
                  onSelected: (v) => setState(() => _sshOnly = v),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _LogLine extends StatelessWidget {
  const _LogLine(this.record);
  final LogRecord record;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 1.5),
      child: Text.rich(
        TextSpan(children: [
          if (record.time != null)
            TextSpan(text: '${record.time} ', style: TextStyle(color: scheme.outline)),
          TextSpan(text: record.raw.substring(record.time == null ? 0 : record.time!.length + 1)),
        ]),
        style: TextStyle(
          fontFamily: 'Menlo',
          fontSize: 11,
          height: 1.35,
          color: _levelColor(record.level, scheme),
        ),
      ),
    );
  }
}
