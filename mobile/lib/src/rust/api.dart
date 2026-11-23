// Typed Dart mirror of `core/core-mobile-ffi/src/api.rs`.
// Regenerated alongside `frb_generated.dart` when `flutter_rust_bridge_codegen generate` runs.

enum MobileTransferProfile {
  desktop,
  mobile,
}

class MobileNodeInfo {
  const MobileNodeInfo({
    required this.nodeId,
    required this.shortId,
    required this.deviceName,
    required this.listenAddr,
    required this.listenPort,
    required this.defaultDownloadDir,
  });

  final String nodeId;
  final String shortId;
  final String deviceName;
  final String listenAddr;
  final int listenPort;
  final String defaultDownloadDir;
}

class MobilePeer {
  const MobilePeer({
    required this.nodeId,
    required this.shortId,
    required this.deviceName,
    required this.addresses,
    required this.port,
  });

  final String nodeId;
  final String shortId;
  final String deviceName;
  final List<String> addresses;
  final int port;
}

class MobileFileSummary {
  const MobileFileSummary({
    required this.relativePath,
    required this.size,
  });

  final String relativePath;
  final int size;
}

class MobileStartSessionResult {
  const MobileStartSessionResult({
    required this.transferId,
    required this.ticketCode,
    required this.qrUri,
    required this.totalSize,
    required this.totalChunks,
    required this.chunkSize,
    required this.fileCount,
    required this.files,
  });

  final String transferId;
  final String ticketCode;
  final String qrUri;
  final int totalSize;
  final int totalChunks;
  final int chunkSize;
  final int fileCount;
  final List<MobileFileSummary> files;
}

class MobileTicketPreview {
  const MobileTicketPreview({
    required this.transferId,
    required this.senderNodeId,
    required this.senderShortId,
    required this.totalSize,
    required this.fileCount,
    required this.directAddrs,
    required this.relayHints,
    required this.merkleRootHex,
  });

  final String transferId;
  final String senderNodeId;
  final String senderShortId;
  final int totalSize;
  final int fileCount;
  final List<String> directAddrs;
  final List<String> relayHints;
  final String merkleRootHex;
}

class MobileJoinSessionResult {
  const MobileJoinSessionResult({
    required this.transferId,
    required this.totalSize,
    required this.fileCount,
  });

  final String transferId;
  final int totalSize;
  final int fileCount;
}

sealed class MobileEngineEvent {
  const MobileEngineEvent();
}

class MobileEventTransferProgress extends MobileEngineEvent {
  const MobileEventTransferProgress({
    required this.transferId,
    required this.direction,
    required this.bytesTransferred,
    required this.totalBytes,
    required this.chunksVerified,
    required this.totalChunks,
    required this.speedBytesPerSec,
    required this.etaSeconds,
    required this.connectionType,
  });

  final String transferId;
  final String direction;
  final int bytesTransferred;
  final int totalBytes;
  final int chunksVerified;
  final int totalChunks;
  final double speedBytesPerSec;
  final double etaSeconds;
  final String connectionType;
}

class MobileEventPeerDiscovered extends MobileEngineEvent {
  const MobileEventPeerDiscovered({required this.peer});
  final MobilePeer peer;
}

class MobileEventPeerLost extends MobileEngineEvent {
  const MobileEventPeerLost({required this.nodeId});
  final String nodeId;
}

class MobileEventIncomingOffer extends MobileEngineEvent {
  const MobileEventIncomingOffer({
    required this.transferId,
    required this.senderNodeId,
    required this.senderShortId,
    required this.senderName,
    required this.fileCount,
    required this.totalSize,
    required this.files,
    required this.connectionType,
  });

  final String transferId;
  final String senderNodeId;
  final String senderShortId;
  final String senderName;
  final int fileCount;
  final int totalSize;
  final List<MobileFileSummary> files;
  final String connectionType;
}

class MobileEventConnectionTypeChanged extends MobileEngineEvent {
  const MobileEventConnectionTypeChanged({
    required this.transferId,
    required this.peerNodeId,
    required this.connectionType,
  });

  final String transferId;
  final String peerNodeId;
  final String connectionType;
}

class MobileEventTransferPaused extends MobileEngineEvent {
  const MobileEventTransferPaused({
    required this.transferId,
    required this.direction,
  });

  final String transferId;
  final String direction;
}

class MobileEventTransferResumed extends MobileEngineEvent {
  const MobileEventTransferResumed({
    required this.transferId,
    required this.direction,
  });

  final String transferId;
  final String direction;
}

class MobileEventTransferCompleted extends MobileEngineEvent {
  const MobileEventTransferCompleted({
    required this.transferId,
    required this.direction,
    required this.totalBytes,
    required this.totalChunks,
    required this.elapsedMs,
    required this.averageSpeedBytesPerSec,
    required this.outputPaths,
  });

  final String transferId;
  final String direction;
  final int totalBytes;
  final int totalChunks;
  final int elapsedMs;
  final double averageSpeedBytesPerSec;
  final List<String> outputPaths;
}

class MobileEventTransferFailed extends MobileEngineEvent {
  const MobileEventTransferFailed({
    required this.transferId,
    required this.direction,
    required this.error,
  });

  final String transferId;
  final String direction;
  final String error;
}

/// Abstract bridge contract implemented by generated `flutter_rust_bridge` bindings
/// (and mockable in Flutter widget/unit tests).
abstract class VeloceRustBridge {
  Future<void> initRuntime();

  Future<MobileNodeInfo> initMobileEngine({
    required String deviceName,
    required String downloadDir,
    required List<String> relayAddrs,
    required MobileTransferProfile profile,
    required List<String> overrideLanAddrs,
  });

  Stream<MobileEngineEvent> subscribeEvents();

  Future<MobileStartSessionResult> startSession({
    required List<String> paths,
    String? targetPeerNodeId,
    required List<String> relayHints,
  });

  Future<MobileJoinSessionResult> joinSession({
    required String ticketCode,
    String? outputDir,
    int? parallelStreams,
  });

  MobileTicketPreview parseTicket({required String ticketCode});

  List<MobilePeer> listLocalPeers();

  void acceptIncomingTransfer({
    required String transferId,
    String? outputDir,
  });

  void rejectIncomingTransfer({
    required String transferId,
    String? reason,
  });

  bool pauseTransfer({required String transferId});

  Future<bool> resumeTransfer({required String transferId});

  bool cancelTransfer({required String transferId});

  void shutdown();
}
