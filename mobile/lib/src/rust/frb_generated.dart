// Pre-codegen stub matching `flutter_rust_bridge.yaml` (`dart_entrypoint_class_name: VeloceRustLib`).
// Overwritten automatically by `flutter_rust_bridge_codegen generate` in CI or local mobile builds.

import 'api.dart';

class VeloceRustLib implements VeloceRustBridge {
  VeloceRustLib._();

  static final VeloceRustLib instance = VeloceRustLib._();

  static Future<void> init() async {
    // Generated FRB v2 code loads `libcore_mobile_ffi.so` (Android) or static symbols (iOS).
  }

  Never _unlinked() => throw UnsupportedError(
        'Native Rust bindings have not been generated yet. '
        'Run `flutter_rust_bridge_codegen generate` from the repository root.',
      );

  @override
  Future<void> initRuntime() => init();

  @override
  Future<MobileNodeInfo> initMobileEngine({
    required String deviceName,
    required String downloadDir,
    required List<String> relayAddrs,
    required MobileTransferProfile profile,
    required List<String> overrideLanAddrs,
  }) async =>
      _unlinked();

  @override
  Stream<MobileEngineEvent> subscribeEvents() => _unlinked();

  @override
  Future<MobileStartSessionResult> startSession({
    required List<String> paths,
    String? targetPeerNodeId,
    required List<String> relayHints,
  }) async =>
      _unlinked();

  @override
  Future<MobileJoinSessionResult> joinSession({
    required String ticketCode,
    String? outputDir,
    int? parallelStreams,
  }) async =>
      _unlinked();

  @override
  MobileTicketPreview parseTicket({required String ticketCode}) => _unlinked();

  @override
  List<MobilePeer> listLocalPeers() => _unlinked();

  @override
  void acceptIncomingTransfer({required String transferId, String? outputDir}) =>
      _unlinked();

  @override
  void rejectIncomingTransfer({required String transferId, String? reason}) =>
      _unlinked();

  @override
  bool pauseTransfer({required String transferId}) => _unlinked();

  @override
  Future<bool> resumeTransfer({required String transferId}) async => _unlinked();

  @override
  bool cancelTransfer({required String transferId}) => _unlinked();

  @override
  void shutdown() {}
}
