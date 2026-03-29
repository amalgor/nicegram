import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/main.dart';

const _levels = ['ALL', 'ERROR', 'WARN', 'INFO', 'DEBUG', 'TRACE'];

class LogsScreen extends StatefulWidget {
  const LogsScreen({super.key});

  @override
  State<LogsScreen> createState() => _LogsScreenState();
}

class _LogsScreenState extends State<LogsScreen> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  late StreamSubscription<List<String>> _sub;
  List<String> _logs = [];
  final ScrollController _scrollController = ScrollController();
  String _filter = '';
  String _levelFilter = 'ALL';
  bool _userScrolledAway = false;

  @override
  void initState() {
    super.initState();
    _logs = List.from(gNetworkLogs);
    _scrollController.addListener(_onScroll);
    _sub = gLogStreamController.stream.listen((logs) {
      if (!mounted) return;
      setState(() { _logs = List.from(logs); });
      _maybeAutoScroll();
    });
  }

  void _onScroll() {
    if (!_scrollController.hasClients) return;
    final pos = _scrollController.position;
    // "at bottom" = within 50px of maxScrollExtent
    _userScrolledAway = pos.pixels < pos.maxScrollExtent - 50;
  }

  void _maybeAutoScroll() {
    if (_userScrolledAway || !_scrollController.hasClients) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scrollController.hasClients) {
        _scrollController.jumpTo(_scrollController.position.maxScrollExtent);
      }
    });
  }

  @override
  void dispose() {
    _sub.cancel();
    _scrollController.removeListener(_onScroll);
    _scrollController.dispose();
    super.dispose();
  }

  int _levelPriority(String level) {
    switch (level) {
      case 'ERROR': return 0;
      case 'WARN': return 1;
      case 'INFO': return 2;
      case 'DEBUG': return 3;
      case 'TRACE': return 4;
      default: return 5;
    }
  }

  String _extractLevel(String log) {
    if (log.contains('[ERROR]') || log.contains('[PANIC]')) return 'ERROR';
    if (log.contains('[WARN]')) return 'WARN';
    if (log.contains('[INFO]')) return 'INFO';
    if (log.contains('[DEBUG]')) return 'DEBUG';
    if (log.contains('[TRACE]')) return 'TRACE';
    return 'OTHER';
  }

  List<String> get _filteredLogs {
    final minPriority = _levelFilter == 'ALL' ? 99 : _levelPriority(_levelFilter);
    return _logs.where((log) {
      if (_levelFilter != 'ALL' && _levelPriority(_extractLevel(log)) > minPriority) {
        return false;
      }
      if (_filter.isNotEmpty && !log.toLowerCase().contains(_filter.toLowerCase())) {
        return false;
      }
      return true;
    }).toList();
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final logs = _filteredLogs;
    return Column(
      children: [
        // Level filter chips
        SizedBox(
          height: 38,
          child: ListView(
            scrollDirection: Axis.horizontal,
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
            children: _levels.map((level) {
              final selected = _levelFilter == level;
              return Padding(
                padding: const EdgeInsets.only(right: 6),
                child: FilterChip(
                  label: Text(level, style: TextStyle(
                    fontSize: 11,
                    color: selected ? Colors.white : _chipColor(level),
                  )),
                  selected: selected,
                  selectedColor: _chipColor(level).withValues(alpha: 0.3),
                  onSelected: (_) => setState(() { _levelFilter = level; }),
                  visualDensity: VisualDensity.compact,
                  padding: EdgeInsets.zero,
                  materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                ),
              );
            }).toList(),
          ),
        ),
        // Search + actions
        Padding(
          padding: const EdgeInsets.fromLTRB(8, 0, 8, 4),
          child: Row(
            children: [
              Expanded(
                child: SizedBox(
                  height: 34,
                  child: TextField(
                    decoration: const InputDecoration(
                      hintText: 'Filter...',
                      prefixIcon: Icon(Icons.search, size: 18),
                      isDense: true,
                      contentPadding: EdgeInsets.symmetric(vertical: 6, horizontal: 8),
                    ),
                    style: const TextStyle(fontSize: 12),
                    onChanged: (v) => setState(() { _filter = v; }),
                  ),
                ),
              ),
              const SizedBox(width: 4),
              Text('${logs.length}', style: const TextStyle(fontSize: 10, color: Colors.grey)),
              IconButton(
                icon: const Icon(Icons.copy, size: 18),
                tooltip: 'Copy all',
                visualDensity: VisualDensity.compact,
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: logs.join('\n')));
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(content: Text('Logs copied')),
                  );
                },
              ),
              IconButton(
                icon: const Icon(Icons.delete_outline, size: 18),
                tooltip: 'Clear',
                visualDensity: VisualDensity.compact,
                onPressed: () {
                  gNetworkLogs.clear();
                  setState(() { _logs = []; });
                },
              ),
              IconButton(
                icon: const Icon(Icons.vertical_align_bottom, size: 18),
                tooltip: 'Scroll to bottom',
                visualDensity: VisualDensity.compact,
                onPressed: () {
                  _userScrolledAway = false;
                  if (_scrollController.hasClients) {
                    _scrollController.jumpTo(_scrollController.position.maxScrollExtent);
                  }
                },
              ),
            ],
          ),
        ),
        const Divider(height: 1),
        Expanded(
          child: logs.isEmpty
              ? const Center(child: Text('No logs yet', style: TextStyle(color: Colors.grey)))
              : Scrollbar(
                  controller: _scrollController,
                  thumbVisibility: true,
                  child: ListView.builder(
                    controller: _scrollController,
                    itemCount: logs.length,
                    itemBuilder: (context, index) {
                      final log = logs[index];
                      final color = _logColor(log);
                      return Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 0),
                        child: Text(
                          log,
                          style: TextStyle(fontFamily: 'monospace', fontSize: 10, color: color, height: 1.3),
                          maxLines: 4,
                          overflow: TextOverflow.ellipsis,
                        ),
                      );
                    },
                  ),
                ),
        ),
      ],
    );
  }

  Color _chipColor(String level) {
    switch (level) {
      case 'ERROR': return Colors.red;
      case 'WARN': return Colors.orange;
      case 'INFO': return Colors.green;
      case 'DEBUG': return Colors.grey;
      case 'TRACE': return Colors.blueGrey;
      default: return Colors.white70;
    }
  }

  Color _logColor(String log) {
    if (log.contains('[ERROR]') || log.contains('[PANIC]')) return Colors.red;
    if (log.contains('[WARN]')) return Colors.orange;
    if (log.contains('[INFO]')) return Colors.green;
    if (log.contains('[DEBUG]')) return Colors.grey;
    if (log.contains('[TRACE]')) return Colors.grey.shade700;
    return Colors.white70;
  }
}
