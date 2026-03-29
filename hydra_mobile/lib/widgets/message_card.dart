import 'package:flutter/material.dart';

class FoldableMessageCard extends StatefulWidget {
  final Map<String, dynamic> message;
  final Function(String messageId, int depth, int readTimeMs)? onAttention;

  const FoldableMessageCard({
    super.key,
    required this.message,
    this.onAttention,
  });

  @override
  State<FoldableMessageCard> createState() => _FoldableMessageCardState();
}

class _FoldableMessageCardState extends State<FoldableMessageCard> {
  int _expandedLevel = 0;
  DateTime? _expandStart;

  Map<String, dynamic> get _tree => widget.message['content_tree'] as Map<String, dynamic>;

  String get _headline => _tree['content'] as String? ?? '';

  String? _getContentAtLevel(Map<String, dynamic> node, int targetLevel) {
    final level = node['level'] as String? ?? '';
    final levelNum = _levelToNum(level);
    if (levelNum == targetLevel) return node['content'] as String?;
    final children = node['children'] as List<dynamic>? ?? [];
    for (final child in children) {
      final found = _getContentAtLevel(child as Map<String, dynamic>, targetLevel);
      if (found != null) return found;
    }
    return null;
  }

  int _levelToNum(String level) {
    switch (level) {
      case 'Headline': return 0;
      case 'Summary': return 1;
      case 'KeyPoints': return 2;
      case 'FullText': return 3;
      default: return 0;
    }
  }

  void _expand(int level) {
    if (_expandStart != null && widget.onAttention != null) {
      final readTime = DateTime.now().difference(_expandStart!).inMilliseconds;
      widget.onAttention!(
        widget.message['id'] as String? ?? '',
        _expandedLevel,
        readTime,
      );
    }
    setState(() {
      _expandedLevel = level;
      _expandStart = DateTime.now();
    });
  }

  @override
  Widget build(BuildContext context) {
    final sender = widget.message['sender_name'] as String? ?? 'Unknown';
    final timestamp = widget.message['timestamp'] as String? ?? '';
    final timeShort = timestamp.length >= 16 ? timestamp.substring(11, 16) : timestamp;

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
      child: InkWell(
        onTap: () => _expand((_expandedLevel + 1).clamp(0, 3)),
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      sender,
                      style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 13),
                    ),
                  ),
                  Text(timeShort, style: const TextStyle(fontSize: 11, color: Colors.grey)),
                  const SizedBox(width: 8),
                  _buildLevelIndicator(),
                ],
              ),
              const SizedBox(height: 6),
              Text(
                _headline,
                style: const TextStyle(fontSize: 14),
                maxLines: _expandedLevel == 0 ? 2 : null,
                overflow: _expandedLevel == 0 ? TextOverflow.ellipsis : null,
              ),
              if (_expandedLevel >= 1) ...[
                const Divider(height: 16),
                _buildSection('Summary', _getContentAtLevel(_tree, 1)),
              ],
              if (_expandedLevel >= 2) ...[
                const Divider(height: 16),
                _buildSection('Key Points', _getContentAtLevel(_tree, 2)),
              ],
              if (_expandedLevel >= 3) ...[
                const Divider(height: 16),
                _buildSection('Full Text', _getContentAtLevel(_tree, 3)),
              ],
              if (_expandedLevel > 0)
                Align(
                  alignment: Alignment.centerRight,
                  child: TextButton(
                    onPressed: () => _expand(0),
                    child: const Text('Collapse', style: TextStyle(fontSize: 12)),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildLevelIndicator() {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: List.generate(4, (i) => Container(
        width: 6,
        height: 6,
        margin: const EdgeInsets.only(left: 2),
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          color: i <= _expandedLevel
              ? Theme.of(context).colorScheme.primary
              : Colors.grey.shade700,
        ),
      )),
    );
  }

  Widget _buildSection(String title, String? content) {
    if (content == null || content.isEmpty) return const SizedBox.shrink();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(title, style: TextStyle(
          fontSize: 11,
          fontWeight: FontWeight.bold,
          color: Theme.of(context).colorScheme.primary,
        )),
        const SizedBox(height: 4),
        Text(content, style: const TextStyle(fontSize: 13)),
      ],
    );
  }
}
