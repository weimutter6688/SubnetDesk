import 'package:flutter_test/flutter_test.dart';
import '../lib/models/reconnect_policy.dart';

void main() {
  test('transient reconnect has a two-minute window and bounded backoff', () {
    final policy = ReconnectPolicy();
    final now = DateTime(2026);
    bool retry(String text, int seconds, {bool connected = true}) => policy.shouldRetry(
      established: connected, type: 'error', title: 'Connection Error', text: text,
      now: now.add(Duration(seconds: seconds)));
    expect(retry('Timeout', 0, connected: false), false);
    expect(retry('Reset by the peer', 0), true);
    expect(retry('Failed to connect: ConnectionRefused', 119), true);
    expect(retry('Timeout', 120), false);
    policy.reset();
    expect(retry('Timeout', 121), true);
    expect(nextReconnectDelay(8), 8);
  });
  test('authentication, changed identity and explicit closes do not retry', () {
    for (final text in ['Wrong Password', 'Access credentials changed',
      'Fingerprint changed', 'Failed to connect: certificate rejected',
      'Connection closed by the peer', 'Connection failed due to inactivity']) {
      expect(ReconnectPolicy().shouldRetry(established: true, type: 'error',
        title: 'Connection Error', text: text, now: DateTime(2026)), false, reason: text);
    }
  });
}
