import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:hydra_mobile/mvp/mobile_state_repository.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:share_plus/share_plus.dart';
import 'package:url_launcher/url_launcher.dart';

class RelayUsageScreen extends StatefulWidget {
  const RelayUsageScreen({super.key});

  @override
  State<RelayUsageScreen> createState() => _RelayUsageScreenState();
}

class _RelayUsageScreenState extends State<RelayUsageScreen>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  static const _repository = MobileStateRepository();

  bool _loading = true;
  RelayUsageSummary _summary = const RelayUsageSummary(
    todayBytes: 0,
    last7dBytes: 0,
    last30dBytes: 0,
  );
  List<RelayUsageSample> _samples = const [];
  double _costPerGb = kDefaultRelayCostPerGb;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    try {
      final results = await Future.wait<dynamic>([
        _repository.loadRelayUsageSummary(),
        _repository.loadRelayUsageSamples(),
        _repository.loadRelayCostPerGb(),
      ]);
      if (!mounted) {
        return;
      }
      setState(() {
        _summary = results[0] as RelayUsageSummary;
        _samples = results[1] as List<RelayUsageSample>;
        _costPerGb = results[2] as double;
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
      ).showSnackBar(SnackBar(content: Text('Failed to load relay usage: $e')));
    }
  }

  List<_DailyBucket> _dailyBuckets(int days) {
    final now = DateTime.now();
    final dayStarts = <DateTime>[];
    for (var offset = days - 1; offset >= 0; offset--) {
      dayStarts.add(
        DateTime(now.year, now.month, now.day).subtract(Duration(days: offset)),
      );
    }

    final totals = <DateTime, int>{for (final day in dayStarts) day: 0};
    for (final sample in _samples) {
      final day = DateTime(
        sample.bucketStart.year,
        sample.bucketStart.month,
        sample.bucketStart.day,
      );
      if (totals.containsKey(day)) {
        totals[day] = totals[day]! + sample.bytes;
      }
    }

    return dayStarts
        .map((day) => _DailyBucket(day: day, bytes: totals[day] ?? 0))
        .toList();
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }

    final todayCost = (_summary.todayBytes / (1024 * 1024 * 1024)) * _costPerGb;
    final weekCost = (_summary.last7dBytes / (1024 * 1024 * 1024)) * _costPerGb;
    final monthCost =
        (_summary.last30dBytes / (1024 * 1024 * 1024)) * _costPerGb;

    return RefreshIndicator(
      onRefresh: _refresh,
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
                    'Cloudflare WSS Relay',
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const SizedBox(height: 8),
                  Text(
                    'This screen tracks only WSS relay traffic written to relay_usage.json. Direct and imported VLESS traffic do not contribute to the estimate.',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                  const SizedBox(height: 16),
                  Wrap(
                    spacing: 12,
                    runSpacing: 12,
                    children: [
                      _metricCard(
                        context,
                        title: 'Today',
                        bytes: _summary.todayBytes,
                        cost: todayCost,
                      ),
                      _metricCard(
                        context,
                        title: '7 Days',
                        bytes: _summary.last7dBytes,
                        cost: weekCost,
                      ),
                      _metricCard(
                        context,
                        title: '30 Days',
                        bytes: _summary.last30dBytes,
                        cost: monthCost,
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: 16),
          _chartCard(context, title: 'Last 7 Days', buckets: _dailyBuckets(7)),
          const SizedBox(height: 16),
          _chartCard(
            context,
            title: 'Last 30 Days',
            buckets: _dailyBuckets(30),
          ),
          const SizedBox(height: 16),
          _buildSupportCard(context),
        ],
      ),
    );
  }

  Widget _metricCard(
    BuildContext context, {
    required String title,
    required int bytes,
    required double cost,
  }) {
    return SizedBox(
      width: 180,
      child: Container(
        padding: const EdgeInsets.all(14),
        decoration: BoxDecoration(
          color: Theme.of(context).colorScheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(18),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(title, style: Theme.of(context).textTheme.bodySmall),
            const SizedBox(height: 8),
            Text(
              formatBytes(bytes),
              style: Theme.of(
                context,
              ).textTheme.titleLarge?.copyWith(fontWeight: FontWeight.w700),
            ),
            const SizedBox(height: 4),
            Text('\$${formatUsd(cost)} estimated'),
          ],
        ),
      ),
    );
  }

  Widget _chartCard(
    BuildContext context, {
    required String title,
    required List<_DailyBucket> buckets,
  }) {
    final maxBytes = buckets.fold<int>(0, (max, bucket) {
      return bucket.bytes > max ? bucket.bytes : max;
    });

    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(title, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 12),
            SizedBox(
              height: 140,
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.end,
                children: buckets.map((bucket) {
                  final heightFactor = maxBytes == 0
                      ? 0.02
                      : bucket.bytes / maxBytes;
                  final label = title == 'Last 7 Days'
                      ? '${bucket.day.month}/${bucket.day.day}'
                      : bucket.day.day.toString();
                  return Expanded(
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 2),
                      child: Column(
                        mainAxisAlignment: MainAxisAlignment.end,
                        children: [
                          Expanded(
                            child: Align(
                              alignment: Alignment.bottomCenter,
                              child: Container(
                                height: (110 * heightFactor)
                                    .clamp(4, 110)
                                    .toDouble(),
                                decoration: BoxDecoration(
                                  gradient: const LinearGradient(
                                    colors: [
                                      Color(0xFF0EA5E9),
                                      Color(0xFF22D3EE),
                                    ],
                                    begin: Alignment.bottomCenter,
                                    end: Alignment.topCenter,
                                  ),
                                  borderRadius: BorderRadius.circular(999),
                                ),
                              ),
                            ),
                          ),
                          const SizedBox(height: 8),
                          Text(
                            label,
                            style: Theme.of(context).textTheme.bodySmall,
                          ),
                        ],
                      ),
                    ),
                  );
                }).toList(),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildSupportCard(BuildContext context) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Support Relay',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 8),
            const Text(
              'Donations stay external in v1. Use the support link below to top up the project-controlled relay balance outside the app.',
            ),
            const SizedBox(height: 16),
            Center(
              child: Container(
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(
                  color: Colors.white,
                  borderRadius: BorderRadius.circular(20),
                ),
                child: QrImageView(
                  data: kRelaySupportUrl,
                  size: 164,
                  backgroundColor: Colors.white,
                ),
              ),
            ),
            const SizedBox(height: 16),
            SelectableText(
              kRelaySupportUrl,
              style: Theme.of(
                context,
              ).textTheme.bodyMedium?.copyWith(fontFamily: 'monospace'),
            ),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                FilledButton.icon(
                  onPressed: () async {
                    await launchUrl(
                      Uri.parse(kRelaySupportUrl),
                      mode: LaunchMode.externalApplication,
                    );
                  },
                  icon: const Icon(Icons.open_in_new),
                  label: const Text('Open'),
                ),
                OutlinedButton.icon(
                  onPressed: () async {
                    final messenger = ScaffoldMessenger.of(context);
                    await Clipboard.setData(
                      const ClipboardData(text: kRelaySupportUrl),
                    );
                    if (!mounted) {
                      return;
                    }
                    messenger.showSnackBar(
                      const SnackBar(content: Text('Support link copied.')),
                    );
                  },
                  icon: const Icon(Icons.copy),
                  label: const Text('Copy'),
                ),
                OutlinedButton.icon(
                  onPressed: () {
                    SharePlus.instance.share(
                      ShareParams(text: kRelaySupportUrl),
                    );
                  },
                  icon: const Icon(Icons.share),
                  label: const Text('Share'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _DailyBucket {
  const _DailyBucket({required this.day, required this.bytes});

  final DateTime day;
  final int bytes;
}
