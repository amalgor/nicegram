import 'package:flutter/material.dart';
import 'package:hydra_mobile/src/rust/api/model_manager.dart';

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
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Model downloaded successfully')));
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
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('Active model set')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Failed to set active model: $e')));
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
                    Text(model.name, style: Theme.of(context).textTheme.titleLarge),
                    if (model.isDownloaded)
                      const Chip(label: Text('Downloaded'), backgroundColor: Colors.green),
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
                  ElevatedButton(onPressed: () => _activateModel(model.id), child: const Text('Set as Active')),
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
