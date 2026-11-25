import 'package:flutter/services.dart';

/// Selected custom destination folder returned by Android SAF (`content://...`) or OS folder picker.
class PickedSaveFolder {
  const PickedSaveFolder({
    required this.uri,
    required this.displayPath,
  });

  final String uri;
  final String displayPath;

  bool get isContentUri => uri.startsWith('content://');
}

/// Platform MethodChannel bridge for Android MulticastLock, Foreground Service,
/// API 30+ Wi-Fi LAN IP enumeration, and SAF custom destination folder selection/export.
/// Safe no-op on iOS and test hosts.
class VelocePlatformService {
  static const MethodChannel _channel = MethodChannel('dev.veloce.p2p/platform');

  Future<void> acquireMulticastLock() async {
    try {
      await _channel.invokeMethod<void>('acquireMulticastLock');
    } on MissingPluginException {
      // No-op on iOS / unit tests.
    } on PlatformException {
      // Ignore if device lacks Wi-Fi hardware.
    }
  }

  Future<void> releaseMulticastLock() async {
    try {
      await _channel.invokeMethod<void>('releaseMulticastLock');
    } on MissingPluginException {
      // No-op.
    } on PlatformException {
      // Ignore.
    }
  }

  Future<void> startForegroundTransferService({
    required String title,
    required String body,
  }) async {
    try {
      await _channel.invokeMethod<void>('startForegroundService', {
        'title': title,
        'body': body,
      });
    } on MissingPluginException {
      // No-op on iOS / unit tests.
    } on PlatformException {
      // Ignore.
    }
  }

  Future<void> stopForegroundTransferService() async {
    try {
      await _channel.invokeMethod<void>('stopForegroundService');
    } on MissingPluginException {
      // No-op.
    } on PlatformException {
      // Ignore.
    }
  }

  /// Queries Android `ConnectivityManager` for active Wi-Fi LAN IP addresses so
  /// Android 11+ (API 30+) devices advertise real LAN IPs even when `RTM_GETLINK` is blocked.
  Future<List<String>> getLocalWifiAddresses() async {
    try {
      final result = await _channel.invokeListMethod<String>('getLocalWifiAddresses');
      return result ?? const [];
    } on MissingPluginException {
      return const [];
    } on PlatformException {
      return const [];
    }
  }

  /// Opens Android's native Storage Access Framework (SAF) folder picker
  /// (`ACTION_OPEN_DOCUMENT_TREE`) with persisted read/write permissions across restarts.
  /// Returns `null` if cancelled or unavailable (e.g., iOS / desktop / unit tests).
  Future<PickedSaveFolder?> pickCustomSaveFolder() async {
    try {
      final result =
          await _channel.invokeMapMethod<String, String>('pickCustomSaveFolder');
      if (result == null) return null;
      final uri = result['uri']?.trim() ?? '';
      final displayPath = result['displayPath']?.trim() ?? '';
      if (uri.isEmpty) return null;
      return PickedSaveFolder(
        uri: uri,
        displayPath: displayPath.isEmpty ? uri : displayPath,
      );
    } on MissingPluginException {
      return null;
    } on PlatformException {
      return null;
    }
  }

  /// Releases a previously persisted Android SAF tree URI grant when resetting to default.
  Future<void> releaseCustomSaveFolder(String uri) async {
    if (uri.trim().isEmpty) return;
    try {
      await _channel.invokeMethod<void>('releaseCustomSaveFolder', {'uri': uri});
    } on MissingPluginException {
      // No-op.
    } on PlatformException {
      // Ignore.
    }
  }

  /// Copies/moves completed verified transfers out of Android app-private sandbox storage
  /// into either the user's selected SAF custom folder (`customTreeUri`) or user-visible
  /// `Downloads/Veloce` via MediaStore (Android 10+) / public Downloads.
  /// Safe no-op on iOS (which writes directly into Documents or chosen directory) and tests.
  Future<List<String>> exportToPublicDownloads(
    List<String> sourcePaths, {
    String? customTreeUri,
    String? customDisplayPath,
    String? stagingRoot,
  }) async {
    if (sourcePaths.isEmpty) return const [];
    try {
      final exported = await _channel.invokeListMethod<String>(
        'exportToPublicDownloads',
        {
          'paths': sourcePaths,
          if (customTreeUri != null && customTreeUri.isNotEmpty)
            'customTreeUri': customTreeUri,
          if (customDisplayPath != null && customDisplayPath.isNotEmpty)
            'customDisplayPath': customDisplayPath,
          if (stagingRoot != null && stagingRoot.isNotEmpty)
            'stagingRoot': stagingRoot,
        },
      );
      return exported ?? sourcePaths;
    } on MissingPluginException {
      return sourcePaths;
    } on PlatformException {
      return sourcePaths;
    }
  }
}
