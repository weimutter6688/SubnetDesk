/// Retry only transport failures after an established session. Authentication,
/// identity confirmation and an explicit remote close always require the user.
class ReconnectPolicy {
  DateTime? _started;
  void reset() => _started = null;

  bool shouldRetry({required bool established, required String type,
    required String title, required String text, required DateTime now}) {
    if (!established || title != 'Connection Error' || type != 'error') return false;
    final error = text.toLowerCase();
    if (['password', 'credential', 'fingerprint', 'certificate', 'permission',
      'denied', 'rejected', 'closed manually', 'inactivity', 'not allowed']
        .any(error.contains)) return false;
    final transient = ['timeout', 'timed out', 'reset by the peer',
      'connection reset', 'connection refused', 'connectionrefused',
      'connection aborted', 'network is unreachable', 'network unreachable',
      'remote desktop is offline', 'failed to connect', 'os error 10061',
      'os error 10054'].any(error.contains);
    if (!transient) return false;
    _started ??= now;
    return now.difference(_started!) < const Duration(minutes: 2);
  }
}

int nextReconnectDelay(int seconds) => (seconds * 2).clamp(1, 8);
