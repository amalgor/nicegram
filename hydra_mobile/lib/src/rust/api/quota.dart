// Quota tracking - client-side implementation
// Server sync will be added when FRB codegen is run for quota.rs

class QuotaInfo {
  final int used;
  final int limit;
  final int remaining;
  final String resetsAt;

  QuotaInfo({
    required this.used,
    required this.limit,
    required this.remaining,
    required this.resetsAt,
  });
}

// Local quota state (will be replaced by Rust bridge after codegen)
int _localBytesUsed = 0;
int _quotaLimit = 52428800; // 50 MB

QuotaInfo getQuotaStatus() {
  final remaining = (_quotaLimit - _localBytesUsed).clamp(0, _quotaLimit);
  final now = DateTime.now().toUtc();
  final tomorrow = DateTime.utc(now.year, now.month, now.day + 1);
  return QuotaInfo(
    used: _localBytesUsed,
    limit: _quotaLimit,
    remaining: remaining,
    resetsAt: tomorrow.toIso8601String(),
  );
}

void recordQuotaBytes(int bytes) {
  _localBytesUsed += bytes;
}

void resetDailyQuota() {
  _localBytesUsed = 0;
}
