import 'dart:math' as math;

String formatBytes(int bytes) {
  if (bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  final idx = math.min(
    (math.log(bytes) / math.log(1024)).floor(),
    units.length - 1,
  );
  final value = bytes / math.pow(1024, idx);
  final formatted = value >= 100
      ? value.toStringAsFixed(0)
      : value >= 10
          ? value.toStringAsFixed(1)
          : value.toStringAsFixed(2);
  return '$formatted ${units[idx]}';
}

String formatSpeed(double bytesPerSec) {
  if (!bytesPerSec.isFinite || bytesPerSec <= 0) return '0.0 MB/s';
  final mb = bytesPerSec / (1024 * 1024);
  if (mb >= 1) {
    return '${mb >= 100 ? mb.toStringAsFixed(0) : mb.toStringAsFixed(1)} MB/s';
  }
  return '${(bytesPerSec / 1024).toStringAsFixed(0)} KB/s';
}

String formatEta(double seconds) {
  if (!seconds.isFinite || seconds <= 0) return '—';
  if (seconds < 1) return '< 1s';
  final total = seconds.ceil();
  final hours = total ~/ 3600;
  final mins = (total % 3600) ~/ 60;
  final secs = total % 60;
  if (hours > 0) return '${hours}h ${mins}m';
  if (mins > 0) return '${mins}m ${secs}s';
  return '${secs}s';
}

/// Formats elapsed transfer duration (measured from when chunks started transferring).
String formatDurationMs(int ms) {
  if (ms <= 0) return '—';
  if (ms < 1000) return '${ms}ms';
  final totalSeconds = ms / 1000.0;
  if (totalSeconds < 10) return '${totalSeconds.toStringAsFixed(1)}s';
  final rounded = totalSeconds.round();
  final hours = rounded ~/ 3600;
  final mins = (rounded % 3600) ~/ 60;
  final secs = rounded % 60;
  if (hours > 0) return '${hours}h ${mins}m ${secs}s';
  if (mins > 0) return '${mins}m ${secs}s';
  return '${secs}s';
}
