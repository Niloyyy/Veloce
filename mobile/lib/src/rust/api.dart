// Pre-codegen stub matching exact `flutter_rust_bridge` v2.13 generated API shapes.
// Overwritten automatically by `flutter_rust_bridge_codegen generate`.
// ignore_for_file: camel_case_types

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
  final BigInt size;
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
  final BigInt totalSize;
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
  final BigInt totalSize;
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
  final BigInt totalSize;
  final int fileCount;
}

sealed class MobileEngineEvent {
  const MobileEngineEvent();
}

class MobileEngineEvent_TransferProgress extends MobileEngineEvent {
  const MobileEngineEvent_TransferProgress({
    required this.transferId,
    required this.direction,
    required this.bytesTransferred,
    required this.totalBytes,
    required this.chunksVerified,
    required this.totalChunks,
    required this.speedBytesPerSec,
    required this.etaSeconds,
    required this.elapsedMs,
    required this.connectionType,
  });

  final String transferId;
  final String direction;
  final BigInt bytesTransferred;
  final BigInt totalBytes;
  final int chunksVerified;
  final int totalChunks;
  final double speedBytesPerSec;
  final double etaSeconds;
  final BigInt elapsedMs;
  final String connectionType;
}

class MobileEngineEvent_PeerDiscovered extends MobileEngineEvent {
  const MobileEngineEvent_PeerDiscovered({required this.peer});
  final MobilePeer peer;
}

class MobileEngineEvent_PeerLost extends MobileEngineEvent {
  const MobileEngineEvent_PeerLost({required this.nodeId});
  final String nodeId;
}

class MobileEngineEvent_IncomingOffer extends MobileEngineEvent {
  const MobileEngineEvent_IncomingOffer({
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
  final BigInt totalSize;
  final List<MobileFileSummary> files;
  final String connectionType;
}

class MobileEngineEvent_ConnectionTypeChanged extends MobileEngineEvent {
  const MobileEngineEvent_ConnectionTypeChanged({
    required this.transferId,
    required this.peerNodeId,
    required this.connectionType,
  });

  final String transferId;
  final String peerNodeId;
  final String connectionType;
}

class MobileEngineEvent_TransferPaused extends MobileEngineEvent {
  const MobileEngineEvent_TransferPaused({
    required this.transferId,
    required this.direction,
  });

  final String transferId;
  final String direction;
}

class MobileEngineEvent_TransferResumed extends MobileEngineEvent {
  const MobileEngineEvent_TransferResumed({
    required this.transferId,
    required this.direction,
  });

  final String transferId;
  final String direction;
}

class MobileEngineEvent_TransferCompleted extends MobileEngineEvent {
  const MobileEngineEvent_TransferCompleted({
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
  final BigInt totalBytes;
  final int totalChunks;
  final BigInt elapsedMs;
  final double averageSpeedBytesPerSec;
  final List<String> outputPaths;
}

class MobileEngineEvent_TransferFailed extends MobileEngineEvent {
  const MobileEngineEvent_TransferFailed({
    required this.transferId,
    required this.direction,
    required this.error,
  });

  final String transferId;
  final String direction;
  final String error;
}

Never _unlinked() => throw UnsupportedError(
      'Run `flutter_rust_bridge_codegen generate` to generate native FFI bindings.',
    );

Future<MobileNodeInfo> initMobileEngine({
  required String deviceName,
  required String downloadDir,
  required List<String> relayAddrs,
  required MobileTransferProfile profile,
  required List<String> overrideLanAddrs,
  String? identityKeyPath,
}) async =>
    _unlinked();

Stream<MobileEngineEvent> subscribeEvents() => _unlinked();

Future<MobileStartSessionResult> startSession({
  required List<String> paths,
  String? targetPeerNodeId,
  required List<String> relayHints,
}) async =>
    _unlinked();

Future<MobileJoinSessionResult> joinSession({
  required String ticketCode,
  String? outputDir,
  int? parallelStreams,
}) async =>
    _unlinked();

MobileTicketPreview parseTicket({required String ticketCode}) => _unlinked();

List<MobilePeer> listLocalPeers() => _unlinked();

void acceptIncomingTransfer({required String transferId, String? outputDir}) =>
    _unlinked();

void rejectIncomingTransfer({required String transferId, String? reason}) =>
    _unlinked();

bool pauseTransfer({required String transferId}) => _unlinked();

Future<bool> resumeTransfer({required String transferId}) async => _unlinked();

bool cancelTransfer({required String transferId}) => _unlinked();

void shutdown() {}
