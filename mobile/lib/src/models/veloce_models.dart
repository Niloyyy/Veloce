// Clean app-layer models and bridge interface decoupled from generated FRB files
// under `lib/src/rust/` so `flutter_rust_bridge_codegen generate` never overwrites them.

enum AppTransferProfile {
  desktop,
  mobile,
}

class AppNodeInfo {
  const AppNodeInfo({
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

class AppPeer {
  const AppPeer({
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

class AppFileSummary {
  const AppFileSummary({
    required this.relativePath,
    required this.size,
  });

  final String relativePath;
  final int size;
}

class AppStartSessionResult {
  const AppStartSessionResult({
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
  final List<AppFileSummary> files;
}

class AppTicketPreview {
  const AppTicketPreview({
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

class AppJoinSessionResult {
  const AppJoinSessionResult({
    required this.transferId,
    required this.totalSize,
    required this.fileCount,
  });

  final String transferId;
  final int totalSize;
  final int fileCount;
}

sealed class AppEngineEvent {
  const AppEngineEvent();
}

class AppEventTransferProgress extends AppEngineEvent {
  const AppEventTransferProgress({
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

class AppEventPeerDiscovered extends AppEngineEvent {
  const AppEventPeerDiscovered({required this.peer});
  final AppPeer peer;
}

class AppEventPeerLost extends AppEngineEvent {
  const AppEventPeerLost({required this.nodeId});
  final String nodeId;
}

class AppEventIncomingOffer extends AppEngineEvent {
  const AppEventIncomingOffer({
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
  final List<AppFileSummary> files;
  final String connectionType;
}

class AppEventConnectionTypeChanged extends AppEngineEvent {
  const AppEventConnectionTypeChanged({
    required this.transferId,
    required this.peerNodeId,
    required this.connectionType,
  });

  final String transferId;
  final String peerNodeId;
  final String connectionType;
}

class AppEventTransferPaused extends AppEngineEvent {
  const AppEventTransferPaused({
    required this.transferId,
    required this.direction,
  });

  final String transferId;
  final String direction;
}

class AppEventTransferResumed extends AppEngineEvent {
  const AppEventTransferResumed({
    required this.transferId,
    required this.direction,
  });

  final String transferId;
  final String direction;
}

class AppEventTransferCompleted extends AppEngineEvent {
  const AppEventTransferCompleted({
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

class AppEventTransferFailed extends AppEngineEvent {
  const AppEventTransferFailed({
    required this.transferId,
    required this.direction,
    required this.error,
  });

  final String transferId;
  final String direction;
  final String error;
}

abstract class VeloceRustBridge {
  Future<void> initRuntime();

  Future<AppNodeInfo> initMobileEngine({
    required String deviceName,
    required String downloadDir,
    required List<String> relayAddrs,
    required AppTransferProfile profile,
    required List<String> overrideLanAddrs,
  });

  Stream<AppEngineEvent> subscribeEvents();

  Future<AppStartSessionResult> startSession({
    required List<String> paths,
    String? targetPeerNodeId,
    required List<String> relayHints,
  });

  Future<AppJoinSessionResult> joinSession({
    required String ticketCode,
    String? outputDir,
    int? parallelStreams,
  });

  AppTicketPreview parseTicket({required String ticketCode});

  List<AppPeer> listLocalPeers();

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
