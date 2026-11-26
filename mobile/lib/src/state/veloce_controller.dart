import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:share_plus/share_plus.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import '../models/veloce_models.dart';
import '../services/platform_service.dart';
import '../theme/veloce_themes.dart';
import '../utils/device_name.dart';
import '../utils/text_clip.dart';

class TrustedPeer {
  const TrustedPeer({
    required this.nodeId,
    required this.shortId,
    required this.deviceName,
    required this.addedAtMs,
  });

  final String nodeId;
  final String shortId;
  final String deviceName;
  final int addedAtMs;

  Map<String, dynamic> toJson() => {
        'nodeId': nodeId,
        'shortId': shortId,
        'deviceName': deviceName,
        'addedAtMs': addedAtMs,
      };

  factory TrustedPeer.fromJson(Map<String, dynamic> json) {
    final nodeId = (json['nodeId'] as String? ?? '').trim();
    final shortId = (json['shortId'] as String? ?? '').trim();
    final deviceName = (json['deviceName'] as String? ?? '').trim();
    return TrustedPeer(
      nodeId: nodeId,
      shortId: shortId.isNotEmpty
          ? shortId
          : (nodeId.length >= 8 ? nodeId.substring(0, 8) : nodeId),
      deviceName: deviceName.isNotEmpty ? deviceName : shortId,
      addedAtMs: (json['addedAtMs'] as num?)?.toInt() ?? 0,
    );
  }
}

enum TransferStatus { active, paused, completed, failed, cancelled }

class TransferItem {
  const TransferItem({
    required this.transferId,
    required this.label,
    required this.direction,
    required this.status,
    required this.bytesTransferred,
    required this.totalBytes,
    required this.chunksVerified,
    required this.totalChunks,
    required this.speedBytesPerSec,
    required this.etaSeconds,
    required this.connectionType,
    this.elapsedMs = 0,
    this.error,
    this.outputPaths = const [],
    this.openUris = const [],
    this.textContent,
  });

  final String transferId;
  final String label;
  final String direction;
  final TransferStatus status;
  final int bytesTransferred;
  final int totalBytes;
  final int chunksVerified;
  final int totalChunks;
  final double speedBytesPerSec;
  final double etaSeconds;
  final String connectionType;
  final int elapsedMs;
  final String? error;
  final List<String> outputPaths;
  final List<String> openUris;
  final String? textContent;

  bool get isTerminal =>
      status == TransferStatus.completed ||
      status == TransferStatus.failed ||
      status == TransferStatus.cancelled;

  double get progress => totalBytes > 0
      ? (bytesTransferred / totalBytes).clamp(0.0, 1.0)
      : (status == TransferStatus.completed ? 1.0 : 0.0);

  Map<String, dynamic> toJson() => {
        'transferId': transferId,
        'label': label,
        'direction': direction,
        'status': status.name,
        'bytesTransferred': bytesTransferred,
        'totalBytes': totalBytes,
        'chunksVerified': chunksVerified,
        'totalChunks': totalChunks,
        'speedBytesPerSec': speedBytesPerSec,
        'etaSeconds': etaSeconds,
        'connectionType': connectionType,
        'elapsedMs': elapsedMs,
        'error': error,
        'outputPaths': outputPaths,
        'openUris': openUris,
        'textContent': textContent,
      };

  factory TransferItem.fromJson(Map<String, dynamic> json) {
    final statusName = json['status'] as String? ?? 'failed';
    final parsedStatus = TransferStatus.values.firstWhere(
      (s) => s.name == statusName,
      orElse: () => TransferStatus.failed,
    );
    // Any transfer that was still active when the app closed is treated as cancelled/interrupted.
    final safeStatus =
        parsedStatus == TransferStatus.active ? TransferStatus.cancelled : parsedStatus;
    return TransferItem(
      transferId: json['transferId'] as String? ?? '',
      label: json['label'] as String? ?? 'Transfer',
      direction: json['direction'] as String? ?? 'receive',
      status: safeStatus,
      bytesTransferred: (json['bytesTransferred'] as num?)?.toInt() ?? 0,
      totalBytes: (json['totalBytes'] as num?)?.toInt() ?? 0,
      chunksVerified: (json['chunksVerified'] as num?)?.toInt() ?? 0,
      totalChunks: (json['totalChunks'] as num?)?.toInt() ?? 0,
      speedBytesPerSec: (json['speedBytesPerSec'] as num?)?.toDouble() ?? 0,
      etaSeconds: 0,
      connectionType: json['connectionType'] as String? ?? 'LAN',
      elapsedMs: (json['elapsedMs'] as num?)?.toInt() ?? 0,
      error: json['error'] as String?,
      outputPaths: (json['outputPaths'] as List<dynamic>?)
              ?.whereType<String>()
              .toList(growable: false) ??
          const [],
      openUris: (json['openUris'] as List<dynamic>?)
              ?.whereType<String>()
              .toList(growable: false) ??
          const [],
      textContent: json['textContent'] as String?,
    );
  }

  TransferItem copyWith({
    String? label,
    TransferStatus? status,
    int? bytesTransferred,
    int? totalBytes,
    int? chunksVerified,
    int? totalChunks,
    double? speedBytesPerSec,
    double? etaSeconds,
    String? connectionType,
    int? elapsedMs,
    String? error,
    bool clearError = false,
    List<String>? outputPaths,
    List<String>? openUris,
    String? textContent,
  }) {
    return TransferItem(
      transferId: transferId,
      label: label ?? this.label,
      direction: direction,
      status: status ?? this.status,
      bytesTransferred: bytesTransferred ?? this.bytesTransferred,
      totalBytes: totalBytes ?? this.totalBytes,
      chunksVerified: chunksVerified ?? this.chunksVerified,
      totalChunks: totalChunks ?? this.totalChunks,
      speedBytesPerSec: speedBytesPerSec ?? this.speedBytesPerSec,
      etaSeconds: etaSeconds ?? this.etaSeconds,
      connectionType: connectionType ?? this.connectionType,
      elapsedMs: elapsedMs ?? this.elapsedMs,
      error: clearError ? null : (error ?? this.error),
      outputPaths: outputPaths ?? this.outputPaths,
      openUris: openUris ?? this.openUris,
      textContent: textContent ?? this.textContent,
    );
  }
}

class VeloceSettings {
  const VeloceSettings({
    required this.deviceName,
    required this.relayHost,
    required this.exportToDownloads,
    this.themeId = VeloceThemeId.obsidian,
    ThemeMode? themeMode,
    this.customSaveUri = '',
    this.customSaveLabel = '',
  }) : _legacyThemeMode = themeMode;

  final String deviceName;
  final String relayHost;
  final bool exportToDownloads;
  final VeloceThemeId themeId;
  final ThemeMode? _legacyThemeMode;
  final String customSaveUri;
  final String customSaveLabel;

  VeloceThemeId get effectiveThemeId {
    if (_legacyThemeMode == ThemeMode.light && themeId == VeloceThemeId.obsidian) {
      return VeloceThemeId.daylight;
    }
    return themeId;
  }

  ThemeMode get themeMode =>
      effectiveThemeId.isDark ? ThemeMode.dark : ThemeMode.light;

  bool get hasCustomSaveFolder => customSaveUri.trim().isNotEmpty;

  String get effectiveSaveLabel {
    if (hasCustomSaveFolder) {
      return customSaveLabel.trim().isNotEmpty
          ? customSaveLabel.trim()
          : customSaveUri.trim();
    }
    if (exportToDownloads) {
      return Platform.isIOS ? 'Files / Veloce (Default)' : 'Downloads/Veloce (Default)';
    }
    return 'App Storage / Veloce';
  }

  VeloceSettings copyWith({
    String? deviceName,
    String? relayHost,
    bool? exportToDownloads,
    VeloceThemeId? themeId,
    ThemeMode? themeMode,
    String? customSaveUri,
    String? customSaveLabel,
  }) {
    final resolvedTheme = themeId ??
        (themeMode != null
            ? (themeMode == ThemeMode.light
                ? VeloceThemeId.daylight
                : VeloceThemeId.obsidian)
            : effectiveThemeId);
    return VeloceSettings(
      deviceName: deviceName ?? this.deviceName,
      relayHost: relayHost ?? this.relayHost,
      exportToDownloads: exportToDownloads ?? this.exportToDownloads,
      themeId: resolvedTheme,
      customSaveUri: customSaveUri ?? this.customSaveUri,
      customSaveLabel: customSaveLabel ?? this.customSaveLabel,
    );
  }
}

class VeloceController extends ChangeNotifier with WidgetsBindingObserver {
  VeloceController({
    required VeloceRustBridge bridge,
    VelocePlatformService? platformService,
    this.enableWakelock = true,
  })  : _bridge = bridge,
        _platform = platformService ?? VelocePlatformService();

  final VeloceRustBridge _bridge;
  final VelocePlatformService _platform;
  final bool enableWakelock;

  static const _prefDeviceName = 'veloce.deviceName';
  static const _prefDeviceNameCustom = 'veloce.deviceNameCustom';
  static const _prefRelayHost = 'veloce.relayHost';
  static const _prefExportDownloads = 'veloce.exportDownloads';
  static const _prefThemeDark = 'veloce.themeDark';
  static const _prefThemeId = 'veloce.themeId';
  static const _prefCustomSaveUri = 'veloce.customSaveUri';
  static const _prefCustomSaveLabel = 'veloce.customSaveLabel';
  static const _prefHistory = 'veloce.transferHistory';
  static const _prefTrustedPeers = 'veloce.trustedPeers';

  VeloceSettings _settings = VeloceSettings(
    deviceName: Platform.isIOS ? 'iPhone' : 'Android Device',
    relayHost: '',
    exportToDownloads: true,
    themeId: VeloceThemeId.obsidian,
  );

  String _detectedDeviceName = '';
  AppNodeInfo? _nodeInfo;
  String? _initError;
  String? _actionError;
  bool _busy = false;
  final Map<String, AppPeer> _peers = {};
  final Map<String, TrustedPeer> _trustedPeers = {};
  final Map<String, TransferItem> _transfers = {};
  AppStartSessionResult? _activeSession;
  AppEventIncomingOffer? _pendingOffer;
  StreamSubscription<AppEngineEvent>? _eventSub;
  bool _foregroundServiceActive = false;
  bool _disposed = false;
  int _lastProgressNotifyMs = 0;

  void _safeNotifyListeners() {
    if (!_disposed) {
      notifyListeners();
    }
  }

  VeloceSettings get settings => _settings;
  String get detectedDeviceName => _detectedDeviceName;
  AppNodeInfo? get nodeInfo => _nodeInfo;
  String? get initError => _initError;
  String? get actionError => _actionError;
  bool get busy => _busy;
  bool get isReady => _nodeInfo != null && _initError == null;
  List<AppPeer> get peers => _peers.values.toList(growable: false);
  List<TrustedPeer> get trustedPeers =>
      _trustedPeers.values.toList(growable: false);
  bool isPeerTrusted(String nodeId) => _trustedPeers.containsKey(nodeId.trim());
  List<TransferItem> get transfers =>
      _transfers.values.toList(growable: false).reversed.toList(growable: false);
  int get activeTransferCount =>
      _transfers.values.where((t) => t.status == TransferStatus.active).length;
  bool get hasCompletedTransfers =>
      _transfers.values.any((t) => t.isTerminal);
  AppStartSessionResult? get activeSession => _activeSession;
  AppEventIncomingOffer? get pendingOffer => _pendingOffer;

  void clearActionError() {
    if (_actionError != null) {
      _actionError = null;
      notifyListeners();
    }
  }

  void reportError(Object err) {
    _actionError = _humanizeError(err);
    notifyListeners();
  }

  String _humanizeError(Object raw) {
    final msg = raw
        .toString()
        .replaceFirst(RegExp(r'^AnyhowException\((.*)\)$', dotAll: true), r'$1')
        .replaceFirst(RegExp(r'^Exception:\s*'), '')
        .trim();
    if (msg.contains('could not reach peer directly or via any configured relay')) {
      return 'Could not reach sender over Wi-Fi or Direct P2P. Ensure both devices '
          'are on the same Wi-Fi/Hotspot, or add a Veloce Relay server (host:port) '
          'in Settings when sending across cellular / strict NAT networks.';
    }
    if (msg.contains('not initialized')) {
      return 'Transfer engine is still starting. Tap Retry in the top banner.';
    }
    return msg;
  }

  Future<void> initialize() async {
    WidgetsBinding.instance.addObserver(this);
    await _loadSettingsAndHistory();
    await _startEngine();
  }

  Future<void> _loadSettingsAndHistory() async {
    final fallbackModel = Platform.isIOS ? 'iPhone' : 'Android Device';
    final detectedRaw = await _platform.getDetectedDeviceName();
    final detected = (detectedRaw != null && detectedRaw.trim().isNotEmpty)
        ? detectedRaw.trim()
        : fallbackModel;
    _detectedDeviceName = detected;

    try {
      final prefs = await SharedPreferences.getInstance();
      final savedName = prefs.getString(_prefDeviceName)?.trim() ?? '';
      final isCustomName = prefs.getBool(_prefDeviceNameCustom) ?? false;
      final resolvedDeviceName =
          (!isCustomName && isLegacyDefaultDeviceName(savedName))
              ? detected
              : (savedName.isNotEmpty ? savedName : detected);
      final savedThemeId = VeloceThemeId.parse(
        prefs.getString(_prefThemeId),
        fallbackDark: prefs.getBool(_prefThemeDark) ?? true,
      );
      _settings = VeloceSettings(
        deviceName: resolvedDeviceName,
        relayHost: prefs.getString(_prefRelayHost) ?? '',
        exportToDownloads: prefs.getBool(_prefExportDownloads) ?? true,
        themeId: savedThemeId,
        customSaveUri: prefs.getString(_prefCustomSaveUri) ?? '',
        customSaveLabel: prefs.getString(_prefCustomSaveLabel) ?? '',
      );

      final rawHistory = prefs.getString(_prefHistory);
      if (rawHistory != null && rawHistory.isNotEmpty) {
        final decoded = jsonDecode(rawHistory);
        if (decoded is List) {
          for (final entry in decoded) {
            if (entry is Map<String, dynamic>) {
              final item = TransferItem.fromJson(entry);
              if (item.transferId.isNotEmpty) {
                _transfers[item.transferId] = item;
              }
            }
          }
        }
      }

      final rawTrusted = prefs.getString(_prefTrustedPeers);
      if (rawTrusted != null && rawTrusted.isNotEmpty) {
        final decoded = jsonDecode(rawTrusted);
        if (decoded is List) {
          for (final entry in decoded) {
            if (entry is Map<String, dynamic>) {
              final peer = TrustedPeer.fromJson(entry);
              if (peer.nodeId.isNotEmpty) {
                _trustedPeers[peer.nodeId] = peer;
              }
            }
          }
        }
      }
    } catch (_) {
      // Keep detected device name even when SharedPreferences is unavailable in tests.
      _settings = _settings.copyWith(deviceName: detected);
    }
  }

  Future<void> _persistTrustedPeers() async {
    try {
      final prefs = await SharedPreferences.getInstance();
      await prefs.setString(
        _prefTrustedPeers,
        jsonEncode(
          _trustedPeers.values.map((p) => p.toJson()).toList(growable: false),
        ),
      );
    } catch (_) {
      // Ignore persistence errors in headless tests.
    }
  }

  Future<void> trustPeer({
    required String nodeId,
    required String shortId,
    required String deviceName,
  }) async {
    final cleanId = nodeId.trim();
    if (cleanId.isEmpty) return;
    _trustedPeers[cleanId] = TrustedPeer(
      nodeId: cleanId,
      shortId: shortId.trim().isNotEmpty
          ? shortId.trim()
          : (cleanId.length >= 8 ? cleanId.substring(0, 8) : cleanId),
      deviceName: deviceName.trim().isNotEmpty ? deviceName.trim() : shortId,
      addedAtMs: DateTime.now().millisecondsSinceEpoch,
    );
    _safeNotifyListeners();
    await _persistTrustedPeers();
  }

  Future<void> untrustPeer(String nodeId) async {
    if (_trustedPeers.remove(nodeId.trim()) != null) {
      _safeNotifyListeners();
      await _persistTrustedPeers();
    }
  }

  Future<void> toggleTrustPeer(AppPeer peer) async {
    if (isPeerTrusted(peer.nodeId)) {
      await untrustPeer(peer.nodeId);
    } else {
      await trustPeer(
        nodeId: peer.nodeId,
        shortId: peer.shortId,
        deviceName: peer.deviceName,
      );
    }
  }

  Future<void> _persistHistory() async {
    try {
      final prefs = await SharedPreferences.getInstance();
      final saved = _transfers.values
          .where((t) => t.isTerminal)
          .toList(growable: false);
      final trimmed =
          saved.length > 100 ? saved.sublist(saved.length - 100) : saved;
      await prefs.setString(
        _prefHistory,
        jsonEncode(trimmed.map((t) => t.toJson()).toList(growable: false)),
      );
    } catch (_) {
      // Ignore persistence errors in headless tests.
    }
  }

  Future<String> _resolveDownloadDir() async {
    try {
      // On iOS / desktop, when the user picked a direct filesystem folder, write verified chunks directly there.
      // On Android, verified chunks always stage in app-private storage first (avoiding Scoped Storage
      // EACCES on POSIX pwrite & resume bitfields), then export to the chosen SAF tree or MediaStore Downloads.
      if (!Platform.isAndroid &&
          _settings.hasCustomSaveFolder &&
          !_settings.customSaveUri.startsWith('content://')) {
        final customDir = Directory(_settings.customSaveUri.trim());
        await customDir.create(recursive: true);
        return customDir.path;
      }
      if (!Platform.isIOS && (_settings.exportToDownloads || _settings.hasCustomSaveFolder)) {
        final downloads = await getDownloadsDirectory();
        if (downloads != null) {
          final dir = Directory('${downloads.path}/Veloce');
          await dir.create(recursive: true);
          return dir.path;
        }
      }
      final docs = await getApplicationDocumentsDirectory();
      final dir = Directory('${docs.path}/Veloce');
      await dir.create(recursive: true);
      return dir.path;
    } catch (_) {
      return Directory.systemTemp.path;
    }
  }

  String? _activeReceiveOutputDir() {
    if (!Platform.isAndroid &&
        _settings.hasCustomSaveFolder &&
        !_settings.customSaveUri.startsWith('content://')) {
      return _settings.customSaveUri.trim();
    }
    return _nodeInfo?.defaultDownloadDir;
  }

  Future<String?> _resolveIdentityKeyPath() async {
    try {
      final support = await getApplicationSupportDirectory();
      await support.create(recursive: true);
      return '${support.path}/veloce-identity.key';
    } catch (_) {
      return null;
    }
  }

  Future<void> _startEngine() async {
    _initError = null;
    _safeNotifyListeners();
    try {
      await _bridge.initRuntime();
      await _platform.acquireMulticastLock();
      final wifiAddrs = await _platform.getLocalWifiAddresses();
      final downloadDir = await _resolveDownloadDir();
      final identityKeyPath = await _resolveIdentityKeyPath();
      final relays = _settings.relayHost
          .split(',')
          .map((s) => s.trim())
          .where((s) => s.isNotEmpty)
          .toList();

      _nodeInfo = await _bridge.initMobileEngine(
        deviceName: _settings.deviceName,
        downloadDir: downloadDir,
        relayAddrs: relays,
        profile: AppTransferProfile.mobile,
        overrideLanAddrs: wifiAddrs,
        identityKeyPath: identityKeyPath,
      );

      _peers.clear();
      for (final peer in _bridge.listLocalPeers()) {
        _peers[peer.nodeId] = peer;
      }

      await _eventSub?.cancel();
      _eventSub = _bridge.subscribeEvents().listen(handleEngineEvent);
    } catch (err) {
      _initError = _humanizeError(err);
    }
    if (!_disposed) {
      _safeNotifyListeners();
    }
  }

  /// Opens the native folder picker (Android SAF `ACTION_OPEN_DOCUMENT_TREE` or iOS directory picker)
  /// without mutating settings yet, so `SettingsSheet` can preview before applying.
  Future<PickedSaveFolder?> pickSaveFolderOnly() async {
    try {
      final nativePicked = await _platform.pickCustomSaveFolder();
      if (nativePicked != null) {
        return nativePicked;
      }
      if (!Platform.isAndroid) {
        final dirPath = await FilePicker.platform.getDirectoryPath(
          dialogTitle: 'Choose Save Folder',
        );
        if (dirPath != null && dirPath.trim().isNotEmpty) {
          final clean = dirPath.trim();
          final parts = clean.split(Platform.pathSeparator).where((s) => s.isNotEmpty).toList();
          final label = parts.length >= 2
              ? '${parts[parts.length - 2]}/${parts.last}'
              : (parts.isNotEmpty ? parts.last : clean);
          return PickedSaveFolder(uri: clean, displayPath: label);
        }
      }
    } catch (err) {
      reportError('Could not open folder picker: $err');
    }
    return null;
  }

  /// Picks a custom destination folder and immediately persists it as the active save location.
  Future<bool> chooseCustomSaveFolder() async {
    final picked = await pickSaveFolderOnly();
    if (picked == null) return false;
    await updateSettings(
      _settings.copyWith(
        customSaveUri: picked.uri,
        customSaveLabel: picked.displayPath,
      ),
    );
    return true;
  }

  /// Clears any custom save folder (releasing Android SAF URI permission) and restores default Downloads/Files.
  Future<void> resetCustomSaveFolder() async {
    await updateSettings(
      _settings.copyWith(
        customSaveUri: '',
        customSaveLabel: '',
      ),
    );
  }

  Future<void> updateSettings(VeloceSettings next) async {
    final trimmedName = next.deviceName.trim();
    final rawSafeName = trimmedName.isNotEmpty
        ? trimmedName
        : (_detectedDeviceName.isNotEmpty
            ? _detectedDeviceName
            : _settings.deviceName);
    final safeName =
        rawSafeName.length > 64 ? rawSafeName.substring(0, 64) : rawSafeName;
    final sanitizedNext = next.copyWith(deviceName: safeName);

    final prevUri = _settings.customSaveUri.trim();
    final nextUri = sanitizedNext.customSaveUri.trim();
    final needsRestart = sanitizedNext.deviceName != _settings.deviceName ||
        sanitizedNext.relayHost != _settings.relayHost ||
        sanitizedNext.exportToDownloads != _settings.exportToDownloads ||
        (!Platform.isAndroid &&
            prevUri != nextUri &&
            !prevUri.startsWith('content://') &&
            !nextUri.startsWith('content://'));

    if (prevUri.startsWith('content://') && prevUri != nextUri) {
      unawaited(_platform.releaseCustomSaveFolder(prevUri));
    }

    _settings = sanitizedNext;
    _safeNotifyListeners();

    try {
      final prefs = await SharedPreferences.getInstance();
      final isCustom = _detectedDeviceName.isEmpty ||
          sanitizedNext.deviceName != _detectedDeviceName;
      await prefs.setString(_prefDeviceName, sanitizedNext.deviceName);
      await prefs.setBool(_prefDeviceNameCustom, isCustom);
      await prefs.setString(_prefRelayHost, sanitizedNext.relayHost);
      await prefs.setBool(_prefExportDownloads, sanitizedNext.exportToDownloads);
      await prefs.setBool(_prefThemeDark, sanitizedNext.effectiveThemeId.isDark);
      await prefs.setString(_prefThemeId, sanitizedNext.effectiveThemeId.name);
      await prefs.setString(_prefCustomSaveUri, sanitizedNext.customSaveUri);
      await prefs.setString(_prefCustomSaveLabel, sanitizedNext.customSaveLabel);
    } catch (_) {
      // Ignore persistence errors in tests.
    }

    if (needsRestart) {
      _activeSession = null;
      await _startEngine();
    }
  }

  void cycleTheme() {
    unawaited(updateSettings(_settings.copyWith(themeId: _settings.effectiveThemeId.next)));
  }

  void toggleTheme() => cycleTheme();

  Future<bool> startSession(List<String> paths, {String? targetPeerNodeId}) async {
    if (paths.isEmpty) return false;
    _busy = true;
    _actionError = null;
    notifyListeners();
    try {
      final relays = _settings.relayHost
          .split(',')
          .map((s) => s.trim())
          .where((s) => s.isNotEmpty)
          .toList();
      final res = await _bridge.startSession(
        paths: paths,
        targetPeerNodeId: targetPeerNodeId,
        relayHints: relays,
      );
      _activeSession = targetPeerNodeId == null ? res : _activeSession;
      if (targetPeerNodeId != null) {
        final label = res.fileCount == 1 && res.files.isNotEmpty
            ? res.files.first.relativePath
            : '${res.fileCount} files';
        _transfers[res.transferId] = TransferItem(
          transferId: res.transferId,
          label: label,
          direction: 'send',
          status: TransferStatus.active,
          bytesTransferred: 0,
          totalBytes: res.totalSize,
          chunksVerified: 0,
          totalChunks: res.totalChunks,
          speedBytesPerSec: 0,
          etaSeconds: 0,
          connectionType: 'LAN',
        );
        await _syncActiveTransferGuards();
      }
      return true;
    } catch (err) {
      _actionError = _humanizeError(err);
      return false;
    } finally {
      _busy = false;
      _safeNotifyListeners();
    }
  }

  /// Sends a quick UTF-8 text message or URL over Veloce's encrypted QUIC channel
  /// either directly to a nearby LAN peer or as a shareable Global Session Ticket/QR.
  Future<bool> sendTextClip(String rawText, {String? targetPeerNodeId}) async {
    final trimmed = rawText.trim();
    if (trimmed.isEmpty) return false;
    final bytes = utf8.encode(trimmed);
    if (bytes.length > kMaxTextClipBytes) {
      reportError('Quick text message exceeds the 256 KB limit.');
      return false;
    }

    _busy = true;
    _actionError = null;
    _safeNotifyListeners();
    try {
      final clipDir = Directory(
        '${Directory.systemTemp.path}/veloce-clips/${DateTime.now().microsecondsSinceEpoch}',
      );
      await clipDir.create(recursive: true);
      final clipFile = File('${clipDir.path}/$kVeloceTextClipFilename');
      await clipFile.writeAsBytes(bytes, flush: true);

      final relays = _settings.relayHost
          .split(',')
          .map((s) => s.trim())
          .where((s) => s.isNotEmpty)
          .toList();
      final res = await _bridge.startSession(
        paths: [clipFile.path],
        targetPeerNodeId: targetPeerNodeId,
        relayHints: relays,
      );
      _activeSession = targetPeerNodeId == null ? res : _activeSession;
      if (targetPeerNodeId != null) {
        _transfers[res.transferId] = TransferItem(
          transferId: res.transferId,
          label: formatTextClipLabel(trimmed),
          direction: 'send',
          status: TransferStatus.active,
          bytesTransferred: 0,
          totalBytes: res.totalSize,
          chunksVerified: 0,
          totalChunks: res.totalChunks,
          speedBytesPerSec: 0,
          etaSeconds: 0,
          connectionType: 'LAN',
          textContent: trimmed,
        );
        await _syncActiveTransferGuards();
      }
      return true;
    } catch (err) {
      _actionError = _humanizeError(err);
      return false;
    } finally {
      _busy = false;
      _safeNotifyListeners();
    }
  }

  Future<void> openTransferFile(TransferItem item) async {
    final candidateUri =
        item.openUris.isNotEmpty ? item.openUris.first : (item.outputPaths.isNotEmpty ? item.outputPaths.first : '');
    if (candidateUri.isEmpty) return;

    final displayName = item.outputPaths.isNotEmpty ? item.outputPaths.first : item.label;
    if (candidateUri.startsWith('content://')) {
      final opened = await _platform.openFile(
        uri: candidateUri,
        displayName: displayName,
      );
      if (opened) return;
    }

    final file = File(candidateUri);
    if (await file.exists()) {
      await Share.shareXFiles([XFile(file.path)], text: item.label);
      return;
    }

    final openedFolder = await _platform.openFolder(
      treeUri: _settings.customSaveUri.startsWith('content://')
          ? _settings.customSaveUri
          : null,
    );
    if (!openedFolder) {
      reportError('Saved at $displayName. Open it from your device Files app.');
    }
  }

  Future<void> openSaveFolder() async {
    final treeUri = _settings.customSaveUri.startsWith('content://')
        ? _settings.customSaveUri
        : null;
    final opened = await _platform.openFolder(treeUri: treeUri);
    if (!opened) {
      reportError('Files are saved in ${_settings.effectiveSaveLabel}.');
    }
  }

  Future<void> openExternalUrl(String url) async {
    final opened = await _platform.openExternalUrl(url);
    if (!opened) {
      await Share.share(url);
    }
  }

  void stopHostingSession() {
    final session = _activeSession;
    if (session == null) return;
    _activeSession = null;
    try {
      _bridge.cancelTransfer(transferId: session.transferId);
    } catch (_) {}
    notifyListeners();
  }

  AppTicketPreview parseTicket(String code) {
    return _bridge.parseTicket(ticketCode: code);
  }

  Future<bool> joinSession(String ticketCode) async {
    _busy = true;
    _actionError = null;
    notifyListeners();
    try {
      final res = await _bridge.joinSession(
        ticketCode: ticketCode,
        outputDir: _activeReceiveOutputDir(),
        parallelStreams: 4,
      );
      _transfers[res.transferId] = TransferItem(
        transferId: res.transferId,
        label: '${res.fileCount} item(s) via Ticket',
        direction: 'receive',
        status: TransferStatus.active,
        bytesTransferred: 0,
        totalBytes: res.totalSize,
        chunksVerified: 0,
        totalChunks: 0,
        speedBytesPerSec: 0,
        etaSeconds: 0,
        connectionType: 'Direct-P2P',
      );
      await _syncActiveTransferGuards();
      return true;
    } catch (err) {
      _actionError = _humanizeError(err);
      return false;
    } finally {
      _busy = false;
      _safeNotifyListeners();
    }
  }

  Future<void> acceptPendingOffer({bool trustSender = false}) async {
    final offer = _pendingOffer;
    if (offer == null) return;
    _pendingOffer = null;
    if (trustSender) {
      await trustPeer(
        nodeId: offer.senderNodeId,
        shortId: offer.senderShortId,
        deviceName: offer.senderName,
      );
    }
    final isTextClip = offer.fileCount == 1 &&
        offer.files.length == 1 &&
        isTextClipPath(offer.files.first.relativePath);
    _transfers[offer.transferId] = TransferItem(
      transferId: offer.transferId,
      label: isTextClip
          ? 'Quick Text from ${offer.senderName}'
          : 'From ${offer.senderName} (${offer.fileCount} item(s))',
      direction: 'receive',
      status: TransferStatus.active,
      bytesTransferred: 0,
      totalBytes: offer.totalSize,
      chunksVerified: 0,
      totalChunks: 0,
      speedBytesPerSec: 0,
      etaSeconds: 0,
      connectionType: offer.connectionType,
    );
    try {
      _bridge.acceptIncomingTransfer(
        transferId: offer.transferId,
        outputDir: _activeReceiveOutputDir(),
      );
      await _syncActiveTransferGuards();
    } catch (err) {
      _actionError = _humanizeError(err);
    }
    _safeNotifyListeners();
  }

  void rejectPendingOffer() {
    final offer = _pendingOffer;
    if (offer == null) return;
    _pendingOffer = null;
    try {
      _bridge.rejectIncomingTransfer(
        transferId: offer.transferId,
        reason: 'Declined on mobile device',
      );
    } catch (_) {}
    notifyListeners();
  }

  void pauseTransfer(String transferId) {
    try {
      _bridge.pauseTransfer(transferId: transferId);
    } catch (err) {
      _actionError = _humanizeError(err);
      notifyListeners();
    }
  }

  Future<void> resumeTransfer(String transferId) async {
    _actionError = null;
    try {
      final resumed = await _bridge.resumeTransfer(transferId: transferId);
      if (resumed) {
        final existing = _transfers[transferId];
        if (existing != null) {
          _transfers[transferId] = existing.copyWith(
            status: TransferStatus.active,
            clearError: true,
          );
        }
        await _syncActiveTransferGuards();
        _safeNotifyListeners();
      }
    } catch (err) {
      _actionError = _humanizeError(err);
      _safeNotifyListeners();
    }
  }

  void cancelTransfer(String transferId) {
    try {
      _bridge.cancelTransfer(transferId: transferId);
    } catch (_) {}
    if (_activeSession?.transferId == transferId) {
      _activeSession = null;
    }
    final existing = _transfers[transferId];
    if (existing != null) {
      _transfers[transferId] = existing.copyWith(
        status: TransferStatus.cancelled,
        speedBytesPerSec: 0,
        etaSeconds: 0,
      );
      unawaited(_persistHistory());
      unawaited(_syncActiveTransferGuards());
      notifyListeners();
    }
  }

  /// Removes a single transfer entry from history (cancelling first if still active/paused).
  void removeTransfer(String transferId) {
    final existing = _transfers.remove(transferId);
    if (existing == null) return;
    if (!existing.isTerminal) {
      try {
        _bridge.cancelTransfer(transferId: transferId);
      } catch (_) {}
    }
    if (_activeSession?.transferId == transferId) {
      _activeSession = null;
    }
    unawaited(_persistHistory());
    unawaited(_syncActiveTransferGuards());
    _safeNotifyListeners();
  }

  /// Clears all completed, failed, and cancelled transfers from history while keeping active ones.
  void clearCompletedTransfers() {
    _transfers.removeWhere((_, item) => item.isTerminal);
    unawaited(_persistHistory());
    notifyListeners();
  }

  /// Auto-pauses active transfers when the OS suspends the app (crucial on iOS to flush
  /// `.veloce-resume-*.state` cleanly before Jetsam/background socket reclamation).
  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.paused) {
      for (final item in _transfers.values) {
        if (item.status == TransferStatus.active && item.direction == 'receive') {
          try {
            _bridge.pauseTransfer(transferId: item.transferId);
          } catch (_) {}
        }
      }
    }
  }

  @visibleForTesting
  void handleEngineEvent(AppEngineEvent event) {
    switch (event) {
      case AppEventPeerDiscovered(:final peer):
        _peers[peer.nodeId] = peer;
      case AppEventPeerLost(:final nodeId):
        _peers.remove(nodeId);
      case AppEventIncomingOffer():
        final isTextClip = event.fileCount == 1 &&
            event.files.length == 1 &&
            isTextClipPath(event.files.first.relativePath);
        if (isPeerTrusted(event.senderNodeId)) {
          _transfers[event.transferId] = TransferItem(
            transferId: event.transferId,
            label: isTextClip
                ? 'Quick Text from ${event.senderName}'
                : 'Auto-accepted from ${event.senderName} (${event.fileCount} item(s))',
            direction: 'receive',
            status: TransferStatus.active,
            bytesTransferred: 0,
            totalBytes: event.totalSize,
            chunksVerified: 0,
            totalChunks: 0,
            speedBytesPerSec: 0,
            etaSeconds: 0,
            connectionType: event.connectionType,
          );
          try {
            _bridge.acceptIncomingTransfer(
              transferId: event.transferId,
              outputDir: _activeReceiveOutputDir(),
            );
          } catch (err) {
            _actionError = _humanizeError(err);
          }
        } else {
          _pendingOffer = event;
        }
      case AppEventConnectionTypeChanged(:final transferId, :final connectionType):
        final existing = _transfers[transferId];
        if (existing != null) {
          _transfers[transferId] = existing.copyWith(connectionType: connectionType);
        }
      case AppEventTransferProgress(
          :final transferId,
          :final direction,
          :final bytesTransferred,
          :final totalBytes,
          :final chunksVerified,
          :final totalChunks,
          :final speedBytesPerSec,
          :final etaSeconds,
          :final elapsedMs,
          :final connectionType,
        ):
        final existing = _transfers[transferId];
        _transfers[transferId] = TransferItem(
          transferId: transferId,
          label: existing?.label ??
              (direction == 'send'
                  ? 'Sending #$transferId'
                  : 'Receiving #$transferId'),
          direction: direction,
          status: TransferStatus.active,
          bytesTransferred: bytesTransferred,
          totalBytes: totalBytes,
          chunksVerified: chunksVerified,
          totalChunks: totalChunks,
          speedBytesPerSec: speedBytesPerSec,
          etaSeconds: etaSeconds,
          connectionType: connectionType,
          elapsedMs: elapsedMs > 0 ? elapsedMs : (existing?.elapsedMs ?? 0),
          textContent: existing?.textContent,
        );
        final wasKnownActive =
            existing != null && existing.status == TransferStatus.active;
        final nowMs = DateTime.now().millisecondsSinceEpoch;
        if (wasKnownActive &&
            chunksVerified < totalChunks &&
            (nowMs - _lastProgressNotifyMs) < 50) {
          return;
        }
        _lastProgressNotifyMs = nowMs;
      case AppEventTransferPaused(:final transferId):
        final existing = _transfers[transferId];
        if (existing != null) {
          _transfers[transferId] = existing.copyWith(
            status: TransferStatus.paused,
            speedBytesPerSec: 0,
            etaSeconds: 0,
          );
        }
      case AppEventTransferResumed(:final transferId):
        final existing = _transfers[transferId];
        if (existing != null) {
          _transfers[transferId] = existing.copyWith(
            status: TransferStatus.active,
            clearError: true,
          );
        }
      case AppEventTransferCompleted(
          :final transferId,
          :final direction,
          :final totalBytes,
          :final totalChunks,
          :final elapsedMs,
          :final averageSpeedBytesPerSec,
          :final outputPaths,
        ):
        final existing = _transfers[transferId];
        _transfers[transferId] = TransferItem(
          transferId: transferId,
          label: existing?.label ??
              (direction == 'send'
                  ? 'Sent #$transferId'
                  : 'Received #$transferId'),
          direction: direction,
          status: TransferStatus.completed,
          bytesTransferred: totalBytes,
          totalBytes: totalBytes,
          chunksVerified: totalChunks,
          totalChunks: totalChunks,
          speedBytesPerSec: averageSpeedBytesPerSec,
          etaSeconds: 0,
          connectionType: existing?.connectionType ?? 'LAN',
          elapsedMs: elapsedMs > 0 ? elapsedMs : (existing?.elapsedMs ?? 0),
          outputPaths: outputPaths,
          openUris: existing?.openUris ?? outputPaths,
          textContent: existing?.textContent,
        );
        unawaited(_persistHistory());
        final isReceivedTextClip = direction == 'receive' &&
            outputPaths.length == 1 &&
            isTextClipPath(outputPaths.first);
        if (isReceivedTextClip) {
          unawaited(_consumeReceivedTextClip(transferId, outputPaths.first));
        } else {
          final shouldExport = direction == 'receive' &&
              outputPaths.isNotEmpty &&
              (_settings.customSaveUri.startsWith('content://') ||
                  (!_settings.hasCustomSaveFolder && _settings.exportToDownloads));
          if (shouldExport) {
            unawaited(_exportCompletedTransfer(transferId, outputPaths));
          }
        }
      case AppEventTransferFailed(:final transferId, :final direction, :final error):
        final existing = _transfers[transferId];
        final isCancel = error.toLowerCase().contains('cancel');
        final humanized = _humanizeError(error);
        _transfers[transferId] = (existing ??
                TransferItem(
                  transferId: transferId,
                  label: 'Transfer #$transferId',
                  direction: direction,
                  status: TransferStatus.failed,
                  bytesTransferred: 0,
                  totalBytes: 0,
                  chunksVerified: 0,
                  totalChunks: 0,
                  speedBytesPerSec: 0,
                  etaSeconds: 0,
                  connectionType: 'LAN',
                ))
            .copyWith(
          status: isCancel ? TransferStatus.cancelled : TransferStatus.failed,
          speedBytesPerSec: 0,
          etaSeconds: 0,
          error: humanized,
        );
        if (!isCancel) {
          _actionError = humanized;
        }
        unawaited(_persistHistory());
    }

    unawaited(_syncActiveTransferGuards());
    _safeNotifyListeners();
  }

  Future<void> _consumeReceivedTextClip(
    String transferId,
    String filePath,
  ) async {
    try {
      final file = File(filePath);
      if (!await file.exists()) return;
      final stat = await file.stat();
      if (stat.size > kMaxTextClipBytes) return;
      final content = await file.readAsString();
      try {
        await file.delete();
      } catch (_) {
        // Ignore cleanup failures.
      }
      if (_disposed) return;
      final current = _transfers[transferId];
      if (current != null) {
        _transfers[transferId] = current.copyWith(
          label: formatTextClipLabel(content, prefix: 'Received'),
          textContent: content,
          outputPaths: const [],
          openUris: const [],
        );
        await _persistHistory();
        _safeNotifyListeners();
      }
    } catch (_) {
      // Keep file path intact if reading fails.
    }
  }

  Future<void> _exportCompletedTransfer(
    String transferId,
    List<String> outputPaths,
  ) async {
    final treeUri = _settings.customSaveUri.startsWith('content://')
        ? _settings.customSaveUri.trim()
        : null;
    final displayPath =
        _settings.customSaveLabel.trim().isNotEmpty ? _settings.customSaveLabel.trim() : null;
    final exported = await _platform.exportToPublicDownloadsWithUris(
      outputPaths,
      customTreeUri: treeUri,
      customDisplayPath: displayPath,
      stagingRoot: _nodeInfo?.defaultDownloadDir,
    );
    if (_disposed) return;
    final current = _transfers[transferId];
    if (current != null && exported.isNotEmpty) {
      _transfers[transferId] = current.copyWith(
        outputPaths: exported.map((e) => e.displayPath).toList(growable: false),
        openUris: exported.map((e) => e.openUri).toList(growable: false),
      );
      await _persistHistory();
      _safeNotifyListeners();
    }
  }

  Future<void> _syncActiveTransferGuards() async {
    final hasActive = activeTransferCount > 0;
    if (enableWakelock) {
      try {
        await WakelockPlus.toggle(enable: hasActive);
      } catch (_) {
        // Ignore in headless/test environments.
      }
    }
    if (hasActive && !_foregroundServiceActive) {
      _foregroundServiceActive = true;
      await _platform.startForegroundTransferService(
        title: 'Veloce Transfer Active',
        body: '$activeTransferCount encrypted QUIC session(s) in progress',
      );
    } else if (!hasActive && _foregroundServiceActive) {
      _foregroundServiceActive = false;
      await _platform.stopForegroundTransferService();
    }
  }

  @override
  void dispose() {
    _disposed = true;
    WidgetsBinding.instance.removeObserver(this);
    unawaited(_eventSub?.cancel());
    unawaited(_platform.stopForegroundTransferService());
    unawaited(_platform.releaseMulticastLock());
    _bridge.shutdown();
    super.dispose();
  }
}
