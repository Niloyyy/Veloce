/// Wire-compatible Quick Text & Link clip utilities shared with Veloce Desktop.
const String kVeloceTextClipFilename = 'veloce-clip-message.txt';
const int kMaxTextClipBytes = 256 * 1024;

bool isTextClipPath(String filePath) {
  final normalized = filePath.replaceAll('\\', '/');
  final base = normalized.split('/').last.trim().toLowerCase();
  return base == kVeloceTextClipFilename;
}

String? extractFirstHttpUrl(String text) {
  final regex = RegExp(r'https?://[^\s<>"{}|\\^`\[\]]+', caseSensitive: false);
  final match = regex.firstMatch(text);
  if (match == null) return null;
  var candidate = match.group(0) ?? '';
  candidate = candidate.replaceFirst(RegExp(r'[),.;!?]+$'), '');
  final uri = Uri.tryParse(candidate);
  if (uri == null || (uri.scheme != 'http' && uri.scheme != 'https')) {
    return null;
  }
  return uri.toString();
}

String formatTextClipLabel(String text, {String? prefix}) {
  final singleLine = text.replaceAll(RegExp(r'\s+'), ' ').trim();
  final short =
      singleLine.length > 40 ? '${singleLine.substring(0, 40)}…' : singleLine;
  final kind = extractFirstHttpUrl(singleLine) != null ? 'Link' : 'Text';
  if (prefix != null && prefix.isNotEmpty) {
    return '$prefix $kind: ${short.isEmpty ? '(empty)' : short}';
  }
  return '$kind: ${short.isEmpty ? '(empty)' : short}';
}
