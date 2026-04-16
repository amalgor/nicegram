import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:hydra_mobile/src/rust/api/simple.dart' as simple_api;

class TerminalScreen extends StatefulWidget {
  const TerminalScreen({super.key});

  @override
  State<TerminalScreen> createState() => _TerminalScreenState();
}

class _TerminalScreenState extends State<TerminalScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  final TextEditingController _inputCtrl = TextEditingController();
  final ScrollController _scrollCtrl = ScrollController();
  final FocusNode _inputFocus = FocusNode();
  final List<_TerminalEntry> _history = [];
  bool _running = false;

  @override
  void dispose() {
    _inputCtrl.dispose();
    _scrollCtrl.dispose();
    _inputFocus.dispose();
    super.dispose();
  }

  Future<void> _executeCommand(String command) async {
    if (command.trim().isEmpty) return;

    setState(() {
      _history.add(_TerminalEntry(type: _EntryType.command, text: command));
      _running = true;
    });
    _inputCtrl.clear();
    _scrollToBottom();

    try {
      final json = await simple_api.shellExec(command: command);
      final result = jsonDecode(json) as Map<String, dynamic>;
      final stdout = result['stdout'] as String? ?? '';
      final stderr = result['stderr'] as String? ?? '';
      final exitCode = result['exit_code'];
      final timedOut = result['timed_out'] as bool? ?? false;

      if (stdout.isNotEmpty) {
        setState(() {
          _history.add(_TerminalEntry(type: _EntryType.stdout, text: stdout));
        });
      }
      if (stderr.isNotEmpty) {
        setState(() {
          _history.add(_TerminalEntry(type: _EntryType.stderr, text: stderr));
        });
      }
      if (exitCode != null && exitCode != 0 && !timedOut) {
        setState(() {
          _history.add(_TerminalEntry(
            type: _EntryType.info,
            text: '[EXIT $exitCode]',
          ));
        });
      }
    } catch (e) {
      setState(() {
        _history.add(_TerminalEntry(
          type: _EntryType.stderr,
          text: '[ERROR] $e',
        ));
      });
    }

    setState(() {
      _running = false;
    });
    _scrollToBottom();
  }

  Future<void> _runInspection() async {
    setState(() {
      _history.add(_TerminalEntry(
        type: _EntryType.info,
        text: '--- Network Inspection ---',
      ));
      _running = true;
    });
    _scrollToBottom();

    try {
      final summary = await simple_api.quickNetworkSummary();
      setState(() {
        _history.add(_TerminalEntry(type: _EntryType.stdout, text: summary));
      });
    } catch (e) {
      setState(() {
        _history.add(_TerminalEntry(
          type: _EntryType.stderr,
          text: '[ERROR] $e',
        ));
      });
    }

    setState(() {
      _running = false;
    });
    _scrollToBottom();
  }

  void _clearHistory() {
    setState(() {
      _history.clear();
    });
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scrollCtrl.hasClients) {
        _scrollCtrl.animateTo(
          _scrollCtrl.position.maxScrollExtent,
          duration: const Duration(milliseconds: 150),
          curve: Curves.easeOut,
        );
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final theme = Theme.of(context);

    return Column(
      children: [
        // Toolbar
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
          child: Row(
            children: [
              Tooltip(
                message: 'Quick network summary',
                child: IconButton(
                  icon: const Icon(Icons.wifi_find),
                  onPressed: _running ? null : _runInspection,
                ),
              ),
              const Spacer(),
              Tooltip(
                message: 'Clear',
                child: IconButton(
                  icon: const Icon(Icons.clear_all),
                  onPressed: _clearHistory,
                ),
              ),
            ],
          ),
        ),
        const Divider(height: 1),
        // Output area
        Expanded(
          child: _history.isEmpty
              ? Center(
                  child: Text(
                    'Type a command below or tap network inspect',
                    style: theme.textTheme.bodyMedium?.copyWith(
                      color: theme.colorScheme.onSurface.withValues(alpha: 0.5),
                    ),
                  ),
                )
              : ListView.builder(
                  controller: _scrollCtrl,
                  padding: const EdgeInsets.all(8),
                  itemCount: _history.length,
                  itemBuilder: (context, index) {
                    final entry = _history[index];
                    return _buildEntry(entry, theme);
                  },
                ),
        ),
        if (_running) const LinearProgressIndicator(minHeight: 2),
        const Divider(height: 1),
        // Input bar
        Padding(
          padding: EdgeInsets.fromLTRB(
            8,
            4,
            8,
            MediaQuery.of(context).viewPadding.bottom + 4,
          ),
          child: Row(
            children: [
              Text(
                '\$ ',
                style: theme.textTheme.bodyLarge?.copyWith(
                  fontFamily: 'monospace',
                  color: const Color(0xFF22C55E),
                  fontWeight: FontWeight.bold,
                ),
              ),
              Expanded(
                child: TextField(
                  controller: _inputCtrl,
                  focusNode: _inputFocus,
                  style: const TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 14,
                  ),
                  decoration: const InputDecoration(
                    hintText: 'ls, ps, cat /proc/net/tcp ...',
                    border: InputBorder.none,
                    isDense: true,
                    contentPadding: EdgeInsets.zero,
                  ),
                  onSubmitted: (value) {
                    _executeCommand(value);
                    _inputFocus.requestFocus();
                  },
                  enabled: !_running,
                ),
              ),
              IconButton(
                icon: const Icon(Icons.send),
                onPressed: _running
                    ? null
                    : () {
                        _executeCommand(_inputCtrl.text);
                        _inputFocus.requestFocus();
                      },
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _buildEntry(_TerminalEntry entry, ThemeData theme) {
    switch (entry.type) {
      case _EntryType.command:
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 2),
          child: SelectableText(
            '\$ ${entry.text}',
            style: const TextStyle(
              fontFamily: 'monospace',
              fontSize: 13,
              color: Color(0xFF22C55E),
              fontWeight: FontWeight.bold,
            ),
          ),
        );
      case _EntryType.stdout:
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 1),
          child: SelectableText(
            entry.text,
            style: TextStyle(
              fontFamily: 'monospace',
              fontSize: 12,
              color: theme.colorScheme.onSurface.withValues(alpha: 0.9),
            ),
          ),
        );
      case _EntryType.stderr:
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 1),
          child: SelectableText(
            entry.text,
            style: const TextStyle(
              fontFamily: 'monospace',
              fontSize: 12,
              color: Color(0xFFEF4444),
            ),
          ),
        );
      case _EntryType.info:
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 2),
          child: SelectableText(
            entry.text,
            style: TextStyle(
              fontFamily: 'monospace',
              fontSize: 12,
              color: theme.colorScheme.onSurface.withValues(alpha: 0.5),
              fontStyle: FontStyle.italic,
            ),
          ),
        );
    }
  }
}

enum _EntryType { command, stdout, stderr, info }

class _TerminalEntry {
  final _EntryType type;
  final String text;

  const _TerminalEntry({required this.type, required this.text});
}
