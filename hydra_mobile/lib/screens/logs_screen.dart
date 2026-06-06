import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/logging/log_store.dart';

// ---------------------------------------------------------------------------
// Log view configuration (grouped by concern, see comments per group).
// ---------------------------------------------------------------------------

/// Verbosity presets the user can switch between. `minLevel` hides anything less
/// important than itself. Debug/Trace are available but off by default per spec.
class _Verbosity {
  const _Verbosity(this.label, this.minLevel);
  final String label;
  final LogLevel minLevel;
}

const _verbosities = <_Verbosity>[
  _Verbosity('ERR', LogLevel.error),
  _Verbosity('WARN', LogLevel.warn),
  _Verbosity('INFO', LogLevel.info), // default
  _Verbosity('DEBUG', LogLevel.debug),
  _Verbosity('TRACE', LogLevel.trace),
];

const int _defaultVerbosityIndex = 2; // INFO

/// Visual style for the log text. Small monospace font per spec.
const double _logFontSize = 10;
const double _logLineHeight = 1.35;

Color _levelColor(LogLevel level) {
  switch (level) {
    case LogLevel.error:
      return const Color(0xFFF87171); // red
    case LogLevel.warn:
      return const Color(0xFFFBBF24); // amber
    case LogLevel.info:
      return const Color(0xFF4ADE80); // green
    case LogLevel.debug:
      return const Color(0xFF94A3B8); // slate
    case LogLevel.trace:
      return const Color(0xFF64748B); // dim slate
    case LogLevel.other:
      return const Color(0xFFCBD5E1);
  }
}

class LogsScreen extends StatefulWidget {
  const LogsScreen({super.key});

  @override
  State<LogsScreen> createState() => _LogsScreenState();
}

class _LogsScreenState extends State<LogsScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  final LogStore _store = LogStore.instance;

  // reverse:true => offset 0 is the newest line (visual bottom). "Following" the
  // tail means staying pinned at offset 0; when the user scrolls up we stop
  // following and new lines append off-screen without moving the viewport.
  final ScrollController _scrollController = ScrollController();
  bool _following = true;

  int _verbosityIndex = _defaultVerbosityIndex;
  bool _sshOnly = false;
  String _search = '';

  @override
  void initState() {
    super.initState();
    _store.addListener(_onStoreChanged);
    _scrollController.addListener(_onScroll);
  }

  @override
  void dispose() {
    _store.removeListener(_onStoreChanged);
    _scrollController.removeListener(_onScroll);
    _scrollController.dispose();
    super.dispose();
  }

  void _onStoreChanged() {
    if (mounted) setState(() {});
  }

  void _onScroll() {
    if (!_scrollController.hasClients) return;
    // In a reversed list, the tail (newest) is at pixels == 0.
    final atTail = _scrollController.position.pixels <= 8;
    if (atTail != _following) {
      setState(() => _following = atTail);
    }
  }

  void _jumpToTail() {
    setState(() => _following = true);
    if (_scrollController.hasClients) {
      _scrollController.jumpTo(0);
    }
  }

  List<LogRecord> get _filtered {
    final minRank = _verbosities[_verbosityIndex].minLevel.index;
    final search = _search.toLowerCase();
    // Build newest-first for the reversed ListView.
    final out = <LogRecord>[];
    final records = _store.records;
    for (var i = records.length - 1; i >= 0; i--) {
      final r = records[i];
      // `other` (unparsed) lines always pass the level gate so nothing vanishes.
      if (r.level != LogLevel.other && r.level.index > minRank) continue;
      if (_sshOnly && !r.isSsh) continue;
      if (search.isNotEmpty && !r.raw.toLowerCase().contains(search)) continue;
      out.add(r);
    }
    return out;
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final logs = _filtered;

    return Column(
      children: [
        _buildControls(context, logs.length),
        const Divider(height: 1),
        Expanded(
          child: logs.isEmpty
              ? const Center(
                  child: Text(
                    'No log activity yet',
                    style: TextStyle(color: Color(0xFF64748B)),
                  ),
                )
              : Stack(
                  children: [
                    Scrollbar(
                      controller: _scrollController,
                      thumbVisibility: true,
                      child: ListView.builder(
                        controller: _scrollController,
                        reverse: true,
                        padding: const EdgeInsets.symmetric(
                          horizontal: 8,
                          vertical: 4,
                        ),
                        itemCount: logs.length,
                        itemBuilder: (context, index) => _LogLine(logs[index]),
                      ),
                    ),
                    if (!_following)
                      Positioned(
                        right: 12,
                        bottom: 12,
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
    );
  }

  Widget _buildControls(BuildContext context, int shownCount) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 6, 8, 4),
      child: Column(
        children: [
          Row(
            children: [
              // Verbosity selector
              Expanded(
                child: SizedBox(
                  height: 30,
                  child: ListView.separated(
                    scrollDirection: Axis.horizontal,
                    itemCount: _verbosities.length,
                    separatorBuilder: (_, __) => const SizedBox(width: 4),
                    itemBuilder: (context, i) {
                      final v = _verbosities[i];
                      final selected = i == _verbosityIndex;
                      final color = _levelColor(v.minLevel);
                      return ChoiceChip(
                        label: Text(v.label, style: const TextStyle(fontSize: 10)),
                        selected: selected,
                        labelStyle: TextStyle(
                          color: selected ? Colors.white : color,
                        ),
                        selectedColor: color.withValues(alpha: 0.35),
                        visualDensity: VisualDensity.compact,
                        materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                        showCheckmark: false,
                        onSelected: (_) =>
                            setState(() => _verbosityIndex = i),
                      );
                    },
                  ),
                ),
              ),
              const SizedBox(width: 4),
              FilterChip(
                label: const Text('SSH', style: TextStyle(fontSize: 10)),
                selected: _sshOnly,
                visualDensity: VisualDensity.compact,
                materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                showCheckmark: false,
                onSelected: (v) => setState(() => _sshOnly = v),
              ),
            ],
          ),
          const SizedBox(height: 4),
          Row(
            children: [
              Expanded(
                child: SizedBox(
                  height: 32,
                  child: TextField(
                    decoration: const InputDecoration(
                      hintText: 'Filter…',
                      prefixIcon: Icon(Icons.search, size: 16),
                      isDense: true,
                      contentPadding:
                          EdgeInsets.symmetric(vertical: 4, horizontal: 8),
                    ),
                    style: const TextStyle(fontSize: 12),
                    onChanged: (v) => setState(() => _search = v),
                  ),
                ),
              ),
              const SizedBox(width: 6),
              Text(
                '$shownCount',
                style: const TextStyle(fontSize: 10, color: Color(0xFF64748B)),
              ),
              IconButton(
                icon: const Icon(Icons.copy, size: 18),
                tooltip: 'Copy shown',
                visualDensity: VisualDensity.compact,
                onPressed: () {
                  final text = _filtered.reversed.map((r) => r.raw).join('\n');
                  Clipboard.setData(ClipboardData(text: text));
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                      content: Text('Logs copied'),
                      duration: Duration(seconds: 1),
                    ),
                  );
                },
              ),
              IconButton(
                icon: const Icon(Icons.delete_outline, size: 18),
                tooltip: 'Clear',
                visualDensity: VisualDensity.compact,
                onPressed: _store.clear,
              ),
            ],
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
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 1),
      child: SelectableText(
        record.raw,
        style: TextStyle(
          fontFamily: 'monospace',
          fontSize: _logFontSize,
          height: _logLineHeight,
          color: _levelColor(record.level),
        ),
      ),
    );
  }
}
