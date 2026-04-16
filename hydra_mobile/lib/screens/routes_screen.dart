import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:hydra_mobile/mvp/mobile_state_repository.dart';
import 'package:hydra_mobile/src/rust/api/routes.dart' as routes_api;

class RoutesScreen extends StatefulWidget {
  const RoutesScreen({super.key});

  @override
  State<RoutesScreen> createState() => _RoutesScreenState();
}

class _RoutesScreenState extends State<RoutesScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  static const _repository = MobileStateRepository();

  bool _loading = true;
  List<RouteProfile> _profiles = const [];

  @override
  void initState() {
    super.initState();
    _loadProfiles();
  }

  Future<void> _loadProfiles() async {
    setState(() {
      _loading = true;
    });
    try {
      final profiles = await _repository.loadRouteProfiles();
      if (!mounted) {
        return;
      }
      setState(() {
        _profiles = profiles;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) {
        return;
      }
      setState(() {
        _loading = false;
      });
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Failed to load routes: $e')));
    }
  }

  Future<void> _saveProfiles(List<RouteProfile> profiles) async {
    await _repository.saveRouteProfiles(profiles);
    await _loadProfiles();
  }

  Future<void> _toggleProfile(RouteProfile profile, bool enabled) async {
    await _repository.updateRouteProfile(profile.copyWith(enabled: enabled));
    await _loadProfiles();
  }

  Future<void> _updateMode(RouteProfile profile, String mode) async {
    await _repository.updateRouteProfile(profile.copyWith(mode: mode));
    await _loadProfiles();
  }

  Future<void> _renameProfile(RouteProfile profile) async {
    final controller = TextEditingController(text: profile.label);
    final updated = await showDialog<String>(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('Rename route'),
          content: TextField(
            controller: controller,
            autofocus: true,
            decoration: const InputDecoration(labelText: 'Label'),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () =>
                  Navigator.of(context).pop(controller.text.trim()),
              child: const Text('Save'),
            ),
          ],
        );
      },
    );

    if (updated == null || updated.isEmpty) {
      return;
    }
    await _repository.updateRouteProfile(profile.copyWith(label: updated));
    await _loadProfiles();
  }

  Future<void> _deleteProfile(RouteProfile profile) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('Delete imported route'),
          content: Text(
            'Remove "${profile.label}" from the device? Existing policies that reference it will fall back to Auto until you change them.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.of(context).pop(true),
              child: const Text('Delete'),
            ),
          ],
        );
      },
    );

    if (confirmed != true) {
      return;
    }
    await _repository.deleteRouteProfile(profile.id);
    await _loadProfiles();
  }

  Future<void> _moveProfile(RouteProfile profile, int direction) async {
    final profiles = [..._profiles];
    final index = profiles.indexWhere((entry) => entry.id == profile.id);
    if (index == -1) {
      return;
    }
    final nextIndex = index + direction;
    if (nextIndex < 0 || nextIndex >= profiles.length) {
      return;
    }
    final current = profiles.removeAt(index);
    profiles.insert(nextIndex, current);
    await _saveProfiles(profiles);
  }

  Future<void> _showImportSheet(String format) async {
    final controller = TextEditingController();
    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      showDragHandle: true,
      builder: (context) {
        var importFormat = format;
        return StatefulBuilder(
          builder: (context, setModalState) {
            return Padding(
              padding: EdgeInsets.fromLTRB(
                16,
                8,
                16,
                MediaQuery.of(context).viewInsets.bottom + 16,
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Import Route',
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const SizedBox(height: 8),
                  SegmentedButton<String>(
                    segments: const [
                      ButtonSegment<String>(
                        value: 'raw',
                        label: Text('Raw VLESS'),
                        icon: Icon(Icons.vpn_key),
                      ),
                      ButtonSegment<String>(
                        value: 'subscription',
                        label: Text('V2Ray Base64'),
                        icon: Icon(Icons.article_outlined),
                      ),
                    ],
                    selected: {importFormat},
                    onSelectionChanged: (selection) {
                      setModalState(() {
                        importFormat = selection.first;
                      });
                    },
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: controller,
                    minLines: 6,
                    maxLines: 12,
                    autofocus: true,
                    decoration: InputDecoration(
                      labelText: importFormat == 'subscription'
                          ? 'Paste V2Ray base64 payload'
                          : 'Paste one or more vless:// URIs',
                      border: const OutlineInputBorder(),
                    ),
                  ),
                  const SizedBox(height: 12),
                  Text(
                    importFormat == 'subscription'
                        ? 'Only vless:// entries will be imported. Unsupported lines are skipped.'
                        : 'You can paste multiple raw VLESS links, one per line.',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                  const SizedBox(height: 16),
                  SizedBox(
                    width: double.infinity,
                    child: FilledButton.icon(
                      onPressed: () async {
                        Navigator.of(context).pop();
                        await _importProfiles(
                          payload: controller.text,
                          format: importFormat,
                        );
                      },
                      icon: const Icon(Icons.download_done),
                      label: const Text('Import'),
                    ),
                  ),
                ],
              ),
            );
          },
        );
      },
    );
  }

  Future<void> _importProfiles({
    required String payload,
    required String format,
  }) async {
    if (payload.trim().isEmpty) {
      return;
    }

    try {
      final report = await _repository.importProfiles(
        payload: payload,
        format: format,
      );
      await _loadProfiles();
      if (!mounted) {
        return;
      }
      final summary =
          'Imported ${report.imported.length}/${report.totalCandidates} route(s).';
      final details = report.skipped.isEmpty ? '' : '\n${report.skipped.first}';
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('$summary$details')));
    } catch (e) {
      if (!mounted) {
        return;
      }
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Import failed: $e')));
    }
  }

  Future<void> _showSshCreateSheet() async {
    final hostCtrl = TextEditingController();
    final portCtrl = TextEditingController(text: '22');
    final userCtrl = TextEditingController();
    final credCtrl = TextEditingController();
    var authType = 'password';
    String? keyFilePath;

    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      showDragHandle: true,
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setModalState) {
            return Padding(
              padding: EdgeInsets.fromLTRB(
                16,
                8,
                16,
                MediaQuery.of(context).viewInsets.bottom + 16,
              ),
              child: SingleChildScrollView(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Add SSH Tunnel',
                      style: Theme.of(context).textTheme.titleLarge,
                    ),
                    const SizedBox(height: 12),
                    TextField(
                      controller: hostCtrl,
                      decoration: const InputDecoration(
                        labelText: 'Host',
                        border: OutlineInputBorder(),
                        hintText: '192.168.1.100',
                      ),
                    ),
                    const SizedBox(height: 10),
                    TextField(
                      controller: portCtrl,
                      keyboardType: TextInputType.number,
                      decoration: const InputDecoration(
                        labelText: 'Port',
                        border: OutlineInputBorder(),
                      ),
                    ),
                    const SizedBox(height: 10),
                    TextField(
                      controller: userCtrl,
                      decoration: const InputDecoration(
                        labelText: 'Username',
                        border: OutlineInputBorder(),
                      ),
                    ),
                    const SizedBox(height: 12),
                    SegmentedButton<String>(
                      segments: const [
                        ButtonSegment<String>(
                          value: 'password',
                          label: Text('Password'),
                          icon: Icon(Icons.lock),
                        ),
                        ButtonSegment<String>(
                          value: 'key_pem',
                          label: Text('Paste PEM'),
                          icon: Icon(Icons.key),
                        ),
                        ButtonSegment<String>(
                          value: 'key_file',
                          label: Text('Key File'),
                          icon: Icon(Icons.file_open),
                        ),
                      ],
                      selected: {authType},
                      onSelectionChanged: (selection) {
                        setModalState(() {
                          authType = selection.first;
                          credCtrl.clear();
                          keyFilePath = null;
                        });
                      },
                    ),
                    const SizedBox(height: 10),
                    if (authType == 'password')
                      TextField(
                        controller: credCtrl,
                        obscureText: true,
                        decoration: const InputDecoration(
                          labelText: 'Password',
                          border: OutlineInputBorder(),
                        ),
                      )
                    else if (authType == 'key_pem')
                      TextField(
                        controller: credCtrl,
                        minLines: 4,
                        maxLines: 8,
                        decoration: const InputDecoration(
                          labelText: 'Paste PEM private key',
                          border: OutlineInputBorder(),
                          hintText: '-----BEGIN OPENSSH PRIVATE KEY-----',
                        ),
                      )
                    else ...[
                      Row(
                        children: [
                          Expanded(
                            child: Text(
                              keyFilePath ?? 'No file selected',
                              style: Theme.of(context).textTheme.bodySmall,
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                          const SizedBox(width: 8),
                          FilledButton.icon(
                            onPressed: () async {
                              final result = await FilePicker.platform
                                  .pickFiles(type: FileType.any);
                              if (result != null &&
                                  result.files.single.path != null) {
                                setModalState(() {
                                  keyFilePath = result.files.single.path!;
                                  credCtrl.text = keyFilePath!;
                                });
                              }
                            },
                            icon: const Icon(Icons.folder_open),
                            label: const Text('Browse'),
                          ),
                        ],
                      ),
                    ],
                    const SizedBox(height: 16),
                    SizedBox(
                      width: double.infinity,
                      child: FilledButton.icon(
                        onPressed: () async {
                          Navigator.of(context).pop();
                          await _createSshProfile(
                            host: hostCtrl.text.trim(),
                            port: int.tryParse(portCtrl.text.trim()) ?? 22,
                            username: userCtrl.text.trim(),
                            authType: authType,
                            credential: credCtrl.text,
                          );
                        },
                        icon: const Icon(Icons.add),
                        label: const Text('Create SSH Profile'),
                      ),
                    ),
                  ],
                ),
              ),
            );
          },
        );
      },
    );
  }

  Future<void> _createSshProfile({
    required String host,
    required int port,
    required String username,
    required String authType,
    required String credential,
  }) async {
    if (host.isEmpty || username.isEmpty || credential.isEmpty) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('All fields are required')),
      );
      return;
    }

    try {
      await routes_api.createSshRouteProfile(
        host: host,
        port: port,
        username: username,
        authType: authType,
        credential: credential,
      );
      await _loadProfiles();
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('SSH profile $username@$host:$port created')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to create SSH profile: $e')),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }

    return RefreshIndicator(
      onRefresh: _loadProfiles,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Route Profiles',
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const SizedBox(height: 8),
                  const Text(
                    'Built-in Hydra WSS relay is seeded automatically. Import your own VLESS credentials as raw links or V2Ray base64 subscriptions.',
                  ),
                  const SizedBox(height: 14),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      FilledButton.icon(
                        onPressed: () => _showImportSheet('raw'),
                        icon: const Icon(Icons.vpn_key),
                        label: const Text('Paste VLESS'),
                      ),
                      OutlinedButton.icon(
                        onPressed: () => _showImportSheet('subscription'),
                        icon: const Icon(Icons.article_outlined),
                        label: const Text('Import Subscription'),
                      ),
                      OutlinedButton.icon(
                        onPressed: _showSshCreateSheet,
                        icon: const Icon(Icons.terminal),
                        label: const Text('Add SSH'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: 12),
          if (_profiles.isEmpty)
            const Card(
              child: Padding(
                padding: EdgeInsets.all(16),
                child: Text('No route profiles are available yet.'),
              ),
            )
          else
            ..._profiles.map(_buildProfileCard),
        ],
      ),
    );
  }

  Widget _buildProfileCard(RouteProfile profile) {
    final routeColor = routeTypeColor(profile.kind);
    final profileIndex = _profiles.indexWhere(
      (entry) => entry.id == profile.id,
    );

    return Card(
      margin: const EdgeInsets.only(bottom: 12),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Container(
                  width: 40,
                  height: 40,
                  decoration: BoxDecoration(
                    color: routeColor.withValues(alpha: 0.12),
                    borderRadius: BorderRadius.circular(12),
                  ),
                  child: Icon(
                    profile.isSsh
                        ? Icons.terminal
                        : profile.isWss
                            ? Icons.cloud_queue
                            : Icons.vpn_key,
                    color: routeColor,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        profile.label,
                        style: const TextStyle(fontWeight: FontWeight.w700),
                      ),
                      const SizedBox(height: 4),
                      Text(
                        profile.shortSummary,
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    ],
                  ),
                ),
                Switch(
                  value: profile.enabled,
                  onChanged: (enabled) => _toggleProfile(profile, enabled),
                ),
              ],
            ),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                _pill(
                  label: profile.isSsh
                      ? 'SSH'
                      : profile.isWss
                          ? 'WSS'
                          : 'VLESS',
                  color: routeColor,
                ),
                _pill(
                  label: profile.mode == 'telegram'
                      ? 'Telegram only'
                      : 'All traffic',
                  color: const Color(0xFF94A3B8),
                ),
                _pill(
                  label: profile.isBuiltin ? 'Built-in' : 'Imported',
                  color: const Color(0xFF94A3B8),
                ),
                _pill(
                  label: 'Priority ${profile.priority + 1}',
                  color: const Color(0xFF94A3B8),
                ),
              ],
            ),
            const SizedBox(height: 12),
            Text(
              profile.endpointSummary,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(fontFamily: 'monospace'),
            ),
            const SizedBox(height: 12),
            Row(
              children: [
                IconButton(
                  onPressed: profileIndex == 0
                      ? null
                      : () => _moveProfile(profile, -1),
                  icon: const Icon(Icons.arrow_upward),
                  tooltip: 'Move up',
                ),
                IconButton(
                  onPressed: profileIndex == _profiles.length - 1
                      ? null
                      : () => _moveProfile(profile, 1),
                  icon: const Icon(Icons.arrow_downward),
                  tooltip: 'Move down',
                ),
                TextButton.icon(
                  onPressed: () => _renameProfile(profile),
                  icon: const Icon(Icons.edit_outlined),
                  label: const Text('Rename'),
                ),
                const Spacer(),
                PopupMenuButton<String>(
                  onSelected: (value) => _updateMode(profile, value),
                  icon: const Icon(Icons.tune),
                  tooltip: 'Route mode',
                  itemBuilder: (context) => const [
                    PopupMenuItem<String>(
                      value: 'all',
                      child: Text('Apply to all traffic'),
                    ),
                    PopupMenuItem<String>(
                      value: 'telegram',
                      child: Text('Telegram only'),
                    ),
                  ],
                ),
                if (!profile.isBuiltin)
                  IconButton(
                    onPressed: () => _deleteProfile(profile),
                    icon: const Icon(Icons.delete_outline),
                    tooltip: 'Delete',
                  ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _pill({required String label, required Color color}) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(label, style: TextStyle(color: color, fontSize: 12)),
    );
  }
}
