import 'dart:io';

import 'package:device_info_plus/device_info_plus.dart';
import 'package:flutter/services.dart';

import '../utils/device_name.dart';

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

/// Exported received file record containing both user-facing path and launchable URI.
class ExportedFileEntry {
  const ExportedFileEntry({
    required this.displayPath,
    required this.openUri,
  });

  final String displayPath;
  final String openUri;
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

  /// Detects the actual human-readable Android or iOS device/model name
  /// (e.g. "Galaxy S24 Ultra", "Pixel 8 Pro", "iPhone 16 Pro", "iPad Air 11-inch (M2)").
  Future<String?> getDetectedDeviceName() async {
    try {
      final nativeName = await _channel.invokeMethod<String>('getDeviceModelName');
      if (nativeName != null && nativeName.trim().isNotEmpty) {
        return nativeName.trim();
      }
    } on MissingPluginException {
      // Fall through to device_info_plus on iOS or desktop hosts.
    } on PlatformException {
      // Fall through to device_info_plus.
    }

    try {
      final plugin = DeviceInfoPlugin();
      if (Platform.isIOS) {
        final ios = await plugin.iosInfo;
        var machine = ios.utsname.machine.trim();
        if (machine == 'x86_64' || machine == 'arm64') {
          final simModel = Platform.environment['SIMULATOR_MODEL_IDENTIFIER']?.trim() ?? '';
          if (simModel.isNotEmpty) {
            machine = simModel;
          }
        }
        final mapped = mapAppleModelIdentifier(machine);
        if (mapped.isNotEmpty) {
          return mapped;
        }
        final userAssigned = ios.name.trim();
        if (userAssigned.isNotEmpty &&
            !const {'iphone', 'ipad', 'ipod touch', 'iphone simulator'}
                .contains(userAssigned.toLowerCase())) {
          return userAssigned;
        }
        final model = ios.model.trim();
        return model.isNotEmpty ? model : 'iPhone';
      }
      if (Platform.isAndroid) {
        final android = await plugin.androidInfo;
        final formatted = formatManufacturerModel(android.manufacturer, android.model);
        if (formatted.isNotEmpty) {
          return formatted;
        }
      }
    } catch (_) {
      // Safe fallback in headless/test environments.
    }
    return null;
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

  /// Exports completed files like `exportToPublicDownloads`, also returning each
  /// exported file's launchable `content://` URI on Android so the user can tap Open.
  Future<List<ExportedFileEntry>> exportToPublicDownloadsWithUris(
    List<String> sourcePaths, {
    String? customTreeUri,
    String? customDisplayPath,
    String? stagingRoot,
  }) async {
    if (sourcePaths.isEmpty) return const [];
    try {
      final rawList = await _channel.invokeListMethod<dynamic>(
        'exportToPublicDownloadsWithUris',
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
      if (rawList != null && rawList.isNotEmpty) {
        final entries = <ExportedFileEntry>[];
        for (final item in rawList) {
          if (item is Map) {
            final displayPath = item['displayPath']?.toString() ?? '';
            final openUri = item['openUri']?.toString() ?? displayPath;
            if (displayPath.isNotEmpty) {
              entries.add(
                ExportedFileEntry(displayPath: displayPath, openUri: openUri),
              );
            }
          }
        }
        if (entries.isNotEmpty) return entries;
      }
    } on MissingPluginException {
      // Fall back to exportToPublicDownloads below (also supports test subclasses).
    } on PlatformException {
      // Fall back below.
    }

    final paths = await exportToPublicDownloads(
      sourcePaths,
      customTreeUri: customTreeUri,
      customDisplayPath: customDisplayPath,
      stagingRoot: stagingRoot,
    );
    return paths
        .map((p) => ExportedFileEntry(displayPath: p, openUri: p))
        .toList(growable: false);
  }

  /// Launches a received Android `content://` file URI in the system viewer.
  Future<bool> openFile({required String uri, String? displayName}) async {
    if (uri.trim().isEmpty) return false;
    try {
      final ok = await _channel.invokeMethod<bool>('openFile', {
        'uri': uri,
        if (displayName != null && displayName.isNotEmpty)
          'displayName': displayName,
      });
      return ok ?? false;
    } on MissingPluginException {
      return false;
    } on PlatformException {
      return false;
    }
  }

  /// Opens the configured SAF destination folder or system Downloads on Android.
  Future<bool> openFolder({String? treeUri}) async {
    try {
      final ok = await _channel.invokeMethod<bool>('openFolder', {
        if (treeUri != null && treeUri.isNotEmpty) 'treeUri': treeUri,
      });
      return ok ?? false;
    } on MissingPluginException {
      return false;
    } on PlatformException {
      return false;
    }
  }

  /// Opens an `http://` or `https://` URL in the system browser on Android.
  Future<bool> openExternalUrl(String url) async {
    final trimmed = url.trim();
    if (trimmed.isEmpty) return false;
    try {
      final ok = await _channel.invokeMethod<bool>('openExternalUrl', {
        'url': trimmed,
      });
      return ok ?? false;
    } on MissingPluginException {
      return false;
    } on PlatformException {
      return false;
    }
  }
}
