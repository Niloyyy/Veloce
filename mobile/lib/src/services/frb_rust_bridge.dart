import '../models/veloce_models.dart';
import '../rust/api.dart' as frb;
import '../rust/frb_generated.dart';

/// Concrete `VeloceRustBridge` implementation delegating to `flutter_rust_bridge`
/// generated bindings under `lib/src/rust/`.
class FrbVeloceBridge implements VeloceRustBridge {
  FrbVeloceBridge._();

  static final FrbVeloceBridge instance = FrbVeloceBridge._();
  bool _initialized = false;

  @override
  Future<void> initRuntime() async {
    if (_initialized) return;
    await VeloceRustLib.init();
    _initialized = true;
  }

  @override
  Future<AppNodeInfo> initMobileEngine({
    required String deviceName,
    required String downloadDir,
    required List<String> relayAddrs,
    required AppTransferProfile profile,
    required List<String> overrideLanAddrs,
    String? identityKeyPath,
  }) async {
    final res = await frb.initMobileEngine(
      deviceName: deviceName,
      downloadDir: downloadDir,
      relayAddrs: relayAddrs,
      profile: profile == AppTransferProfile.mobile
          ? frb.MobileTransferProfile.mobile
          : frb.MobileTransferProfile.desktop,
      overrideLanAddrs: overrideLanAddrs,
      identityKeyPath: identityKeyPath,
    );
    return AppNodeInfo(
      nodeId: res.nodeId,
      shortId: res.shortId,
      deviceName: res.deviceName,
      listenAddr: res.listenAddr,
      listenPort: res.listenPort,
      defaultDownloadDir: res.defaultDownloadDir,
    );
  }

  @override
  Stream<AppEngineEvent> subscribeEvents() {
    return frb.subscribeEvents().map(_mapEvent);
  }

  @override
  Future<AppStartSessionResult> startSession({
    required List<String> paths,
    String? targetPeerNodeId,
    required List<String> relayHints,
  }) async {
    final res = await frb.startSession(
      paths: paths,
      targetPeerNodeId: targetPeerNodeId,
      relayHints: relayHints,
    );
    return AppStartSessionResult(
      transferId: res.transferId,
      ticketCode: res.ticketCode,
      qrUri: res.qrUri,
      totalSize: res.totalSize.toInt(),
      totalChunks: res.totalChunks,
      chunkSize: res.chunkSize,
      fileCount: res.fileCount,
      files: res.files
          .map((f) => AppFileSummary(relativePath: f.relativePath, size: f.size.toInt()))
          .toList(growable: false),
    );
  }

  @override
  Future<AppJoinSessionResult> joinSession({
    required String ticketCode,
    String? outputDir,
    int? parallelStreams,
  }) async {
    final res = await frb.joinSession(
      ticketCode: ticketCode,
      outputDir: outputDir,
      parallelStreams: parallelStreams,
    );
    return AppJoinSessionResult(
      transferId: res.transferId,
      totalSize: res.totalSize.toInt(),
      fileCount: res.fileCount,
    );
  }

  @override
  AppTicketPreview parseTicket({required String ticketCode}) {
    final res = frb.parseTicket(ticketCode: ticketCode);
    return AppTicketPreview(
      transferId: res.transferId,
      senderNodeId: res.senderNodeId,
      senderShortId: res.senderShortId,
      totalSize: res.totalSize.toInt(),
      fileCount: res.fileCount,
      directAddrs: res.directAddrs,
      relayHints: res.relayHints,
      merkleRootHex: res.merkleRootHex,
    );
  }

  @override
  List<AppPeer> listLocalPeers() {
    return frb
        .listLocalPeers()
        .map(
          (p) => AppPeer(
            nodeId: p.nodeId,
            shortId: p.shortId,
            deviceName: p.deviceName,
            addresses: p.addresses,
            port: p.port,
          ),
        )
        .toList(growable: false);
  }

  @override
  void acceptIncomingTransfer({required String transferId, String? outputDir}) {
    frb.acceptIncomingTransfer(transferId: transferId, outputDir: outputDir);
  }

  @override
  void rejectIncomingTransfer({required String transferId, String? reason}) {
    frb.rejectIncomingTransfer(transferId: transferId, reason: reason);
  }

  @override
  bool pauseTransfer({required String transferId}) {
    return frb.pauseTransfer(transferId: transferId);
  }

  @override
  Future<bool> resumeTransfer({required String transferId}) {
    return frb.resumeTransfer(transferId: transferId);
  }

  @override
  bool cancelTransfer({required String transferId}) {
    return frb.cancelTransfer(transferId: transferId);
  }

  @override
  void shutdown() {
    frb.shutdown();
  }

  AppEngineEvent _mapEvent(frb.MobileEngineEvent evt) {
    return switch (evt) {
      frb.MobileEngineEvent_TransferProgress(
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
      ) =>
        AppEventTransferProgress(
          transferId: transferId,
          direction: direction,
          bytesTransferred: bytesTransferred.toInt(),
          totalBytes: totalBytes.toInt(),
          chunksVerified: chunksVerified,
          totalChunks: totalChunks,
          speedBytesPerSec: speedBytesPerSec,
          etaSeconds: etaSeconds,
          elapsedMs: elapsedMs.toInt(),
          connectionType: connectionType,
        ),
      frb.MobileEngineEvent_PeerDiscovered(:final peer) => AppEventPeerDiscovered(
          peer: AppPeer(
            nodeId: peer.nodeId,
            shortId: peer.shortId,
            deviceName: peer.deviceName,
            addresses: peer.addresses,
            port: peer.port,
          ),
        ),
      frb.MobileEngineEvent_PeerLost(:final nodeId) =>
        AppEventPeerLost(nodeId: nodeId),
      frb.MobileEngineEvent_IncomingOffer(
        :final transferId,
        :final senderNodeId,
        :final senderShortId,
        :final senderName,
        :final fileCount,
        :final totalSize,
        :final files,
        :final connectionType,
      ) =>
        AppEventIncomingOffer(
          transferId: transferId,
          senderNodeId: senderNodeId,
          senderShortId: senderShortId,
          senderName: senderName,
          fileCount: fileCount,
          totalSize: totalSize.toInt(),
          files: files
              .map(
                (f) => AppFileSummary(
                  relativePath: f.relativePath,
                  size: f.size.toInt(),
                ),
              )
              .toList(growable: false),
          connectionType: connectionType,
        ),
      frb.MobileEngineEvent_ConnectionTypeChanged(
        :final transferId,
        :final peerNodeId,
        :final connectionType,
      ) =>
        AppEventConnectionTypeChanged(
          transferId: transferId,
          peerNodeId: peerNodeId,
          connectionType: connectionType,
        ),
      frb.MobileEngineEvent_TransferPaused(:final transferId, :final direction) =>
        AppEventTransferPaused(transferId: transferId, direction: direction),
      frb.MobileEngineEvent_TransferResumed(:final transferId, :final direction) =>
        AppEventTransferResumed(transferId: transferId, direction: direction),
      frb.MobileEngineEvent_TransferCompleted(
        :final transferId,
        :final direction,
        :final totalBytes,
        :final totalChunks,
        :final elapsedMs,
        :final averageSpeedBytesPerSec,
        :final outputPaths,
      ) =>
        AppEventTransferCompleted(
          transferId: transferId,
          direction: direction,
          totalBytes: totalBytes.toInt(),
          totalChunks: totalChunks,
          elapsedMs: elapsedMs.toInt(),
          averageSpeedBytesPerSec: averageSpeedBytesPerSec,
          outputPaths: outputPaths,
        ),
      frb.MobileEngineEvent_TransferFailed(
        :final transferId,
        :final direction,
        :final error,
      ) =>
        AppEventTransferFailed(
          transferId: transferId,
          direction: direction,
          error: error,
        ),
    };
  }
}
