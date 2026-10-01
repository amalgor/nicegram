import 'package:flutter_test/flutter_test.dart';
import 'package:hydra_mobile/app/models.dart';
import 'package:hydra_mobile/logging/log_store.dart';

void main() {
  group('parseSshTarget', () {
    test('full ssh -D command', () {
      final t = parseSshTarget('ssh -D1080 -p 2222 alice@example.com')!;
      expect((t.user, t.host, t.port), ('alice', 'example.com', 2222));
    });

    test('options with values are skipped', () {
      final t = parseSshTarget('ssh -D 1080 -i ~/.ssh/id root@10.0.0.1 -N')!;
      expect((t.user, t.host, t.port), ('root', '10.0.0.1', null));
    });

    test('user@host:port and ssh:// url', () {
      expect(parseSshTarget('bob@h.io:2200')!.port, 2200);
      final url = parseSshTarget('ssh://bob@h.io:22')!;
      expect((url.user, url.host, url.port), ('bob', 'h.io', 22));
    });

    test('bracketed IPv6 and bare IPv6', () {
      final b = parseSshTarget('u@[2001:db8::1]:2022')!;
      expect((b.host, b.port), ('2001:db8::1', 2022));
      expect(parseSshTarget('2001:db8::1')!.host, '2001:db8::1');
    });

    test('nothing usable', () {
      expect(parseSshTarget(''), isNull);
      expect(parseSshTarget('ssh -v'), isNull);
    });
  });

  group('LogRecord.parse', () {
    test('timestamped rust line', () {
      final r = LogRecord.parse('12:03:04.567 [WARN] hydra_core::transport::ssh: lost ssh_event=closed');
      expect(r.time, '12:03:04.567');
      expect(r.level, LogLevel.warn);
      expect(r.target, 'hydra_core::transport::ssh');
      expect(r.message, 'lost ssh_event=closed');
      expect(r.isSsh, isTrue);
    });

    test('legacy line without time', () {
      final r = LogRecord.parse('[INFO] dart::startup: ok');
      expect((r.time, r.level, r.target), (null, LogLevel.info, 'dart::startup'));
    });

    test('unparsed line kept verbatim', () {
      final r = LogRecord.parse('random text');
      expect((r.level, r.message), (LogLevel.other, 'random text'));
    });
  });

  test('formatBytes / describeError', () {
    expect(formatBytes(512), '512 B');
    expect(formatBytes(1536), '1.5 KB');
    expect(describeError('AnyhowException(SSH auth failed\n\nStack backtrace: x)'), 'SSH auth failed');
  });
}
