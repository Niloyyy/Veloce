import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import '../models/veloce_models.dart';
import '../services/platform_service.dart';

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
    this.error,
    this.outputPaths = const [],
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
  final String? error;
  final List<String> outputPaths;

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
        'error': error,
        'outputPaths': outputPaths,
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
      error: json['error'] as String?,
      outputPaths: (json['outputPaths'] as List<dynamic>?)
              ?.whereType<String>()
              .toList(growable: false) ??
          const [],
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
    String? error,
    bool clearError = false,
    List<String>? outputPaths,
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
      error: clearError ? null : (error ?? this.error),
      outputPaths: outputPaths ?? this.outputPaths,
    );
  }
}

class VeloceSettings {
  const VeloceSettings({
    required this.deviceName,
    required this.relayHost,
    required this.exportToDownloads,
    required this.themeMode,
  });

  final String deviceName;
  final String relayHost;
  final bool exportToDownloads;
  final ThemeMode themeMode;

  VeloceSettings copyWith({
    String? deviceName,
    String? relayHost,
    bool? exportToDownloads,
    ThemeMode? themeMode,
  }) {
    return VeloceSettings(
      deviceName: deviceName ?? this.deviceName,
      relayHost: relayHost ?? this.relayHost,
      exportToDownloads: exportToDownloads ?? this.exportToDownloads,
      themeMode: themeMode ?? this.themeMode,
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
  static const _prefRelayHost = 'veloce.relayHost';
  static const _prefExportDownloads = 'veloce.exportDownloads';
  static const _prefThemeDark = 'veloce.themeDark';
  static const _prefHistory = 'veloce.transferHistory';

  VeloceSettings _settings = VeloceSettings(
    deviceName: Platform.isIOS ? 'Veloce iPhone' : 'Veloce Mobile',
    relayHost: '',
    exportToDownloads: true,
    themeMode: ThemeMode.dark,
  );

  AppNodeInfo? _nodeInfo;
  String? _initError;
  String? _actionError;
  bool _busy = false;
  final Map<String, AppPeer> _peers = {};
  final Map<String, TransferItem> _transfers = {};
  AppStartSessionResult? _activeSession;
  AppEventIncomingOffer? _pendingOffer;
  StreamSubscription<AppEngineEvent>? _eventSub;
  bool _foregroundServiceActive = false;

  VeloceSettings get settings => _settings;
  AppNodeInfo? get nodeInfo => _nodeInfo;
  String? get initError => _initError;
  String? get actionError => _actionError;
  bool get busy => _busy;
  bool get isReady => _nodeInfo != null && _initError == null;
  List<AppPeer> get peers => _peers.values.toList(growable: false);
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
    try {
      final prefs = await SharedPreferences.getInstance();
      final defaultName = Platform.isIOS ? 'Veloce iPhone' : 'Veloce Android';
      _settings = VeloceSettings(
        deviceName: prefs.getString(_prefDeviceName) ?? defaultName,
        relayHost: prefs.getString(_prefRelayHost) ?? '',
        exportToDownloads: prefs.getBool(_prefExportDownloads) ?? true,
        themeMode: (prefs.getBool(_prefThemeDark) ?? true)
            ? ThemeMode.dark
            : ThemeMode.light,
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
    } catch (_) {
      // Keep default settings in tests or restricted environments.
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
      // On iOS, ApplicationDocumentsDirectory/Veloce is directly exposed in the iOS Files
      // app via UIFileSharingEnabled + LSSupportsOpeningDocumentsInPlace.
      // On Android, verified chunks write to app-private storage first (for POSIX pwrite
      // & resume bitfields), then export to public Downloads/Veloce via MediaStore on completion.
      if (!Platform.isIOS && _settings.exportToDownloads) {
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

  Future<void> _startEngine() async {
    _initError = null;
    notifyListeners();
    try {
      await _bridge.initRuntime();
      await _platform.acquireMulticastLock();
      final wifiAddrs = await _platform.getLocalWifiAddresses();
      final downloadDir = await _resolveDownloadDir();
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
    notifyListeners();
  }

  Future<void> updateSettings(VeloceSettings next) async {
    final needsRestart = next.deviceName != _settings.deviceName ||
        next.relayHost != _settings.relayHost ||
        next.exportToDownloads != _settings.exportToDownloads;
    _settings = next;
    notifyListeners();

    try {
      final prefs = await SharedPreferences.getInstance();
      await prefs.setString(_prefDeviceName, next.deviceName);
      await prefs.setString(_prefRelayHost, next.relayHost);
      await prefs.setBool(_prefExportDownloads, next.exportToDownloads);
      await prefs.setBool(_prefThemeDark, next.themeMode == ThemeMode.dark);
    } catch (_) {
      // Ignore persistence errors in tests.
    }

    if (needsRestart) {
      _activeSession = null;
      await _startEngine();
    }
  }

  void toggleTheme() {
    final nextMode =
        _settings.themeMode == ThemeMode.dark ? ThemeMode.light : ThemeMode.dark;
    unawaited(updateSettings(_settings.copyWith(themeMode: nextMode)));
  }

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
      notifyListeners();
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
        outputDir: _nodeInfo?.defaultDownloadDir,
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
      notifyListeners();
    }
  }

  Future<void> acceptPendingOffer() async {
    final offer = _pendingOffer;
    if (offer == null) return;
    _pendingOffer = null;
    _transfers[offer.transferId] = TransferItem(
      transferId: offer.transferId,
      label: 'From ${offer.senderName} (${offer.fileCount} item(s))',
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
        outputDir: _nodeInfo?.defaultDownloadDir,
      );
      await _syncActiveTransferGuards();
    } catch (err) {
      _actionError = _humanizeError(err);
    }
    notifyListeners();
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
        notifyListeners();
      }
    } catch (err) {
      _actionError = _humanizeError(err);
      notifyListeners();
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
    notifyListeners();
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
        _pendingOffer = event;
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
        );
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
          outputPaths: outputPaths,
        );
        unawaited(_persistHistory());
        if (direction == 'receive' &&
            _settings.exportToDownloads &&
            outputPaths.isNotEmpty) {
          unawaited(_exportCompletedTransfer(transferId, outputPaths));
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
    notifyListeners();
  }

  Future<void> _exportCompletedTransfer(
    String transferId,
    List<String> outputPaths,
  ) async {
    final exported = await _platform.exportToPublicDownloads(outputPaths);
    final current = _transfers[transferId];
    if (current != null && exported.isNotEmpty) {
      _transfers[transferId] = current.copyWith(outputPaths: exported);
      await _persistHistory();
      notifyListeners();
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
    WidgetsBinding.instance.removeObserver(this);
    unawaited(_eventSub?.cancel());
    unawaited(_platform.stopForegroundTransferService());
    unawaited(_platform.releaseMulticastLock());
    _bridge.shutdown();
    super.dispose();
  }
}
