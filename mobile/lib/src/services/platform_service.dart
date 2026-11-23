import 'package:flutter/services.dart';

/// Platform MethodChannel bridge for Android MulticastLock, Foreground Service,
/// and API 30+ Wi-Fi LAN IP enumeration. Safe no-op on iOS and test hosts.
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

  /// Copies/moves completed verified transfers out of Android app-private sandbox storage
  /// into user-visible `Downloads/Veloce` via MediaStore (Android 10+) or public Downloads.
  /// Safe no-op on iOS (which exposes `Documents/Veloce` directly in Files) and tests.
  Future<List<String>> exportToPublicDownloads(List<String> sourcePaths) async {
    if (sourcePaths.isEmpty) return const [];
    try {
      final exported = await _channel.invokeListMethod<String>(
        'exportToPublicDownloads',
        {'paths': sourcePaths},
      );
      return exported ?? sourcePaths;
    } on MissingPluginException {
      return sourcePaths;
    } on PlatformException {
      return sourcePaths;
    }
  }
}
