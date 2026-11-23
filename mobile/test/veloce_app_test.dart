import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:veloce_mobile/src/models/veloce_models.dart';
import 'package:veloce_mobile/src/state/veloce_controller.dart';
import 'package:veloce_mobile/src/utils/formatters.dart';
import 'package:veloce_mobile/src/widgets/connection_badge.dart';
import 'package:veloce_mobile/src/widgets/transfer_card.dart';

class FakeRustBridge implements VeloceRustBridge {
  final StreamController<AppEngineEvent> events = StreamController.broadcast();
  final List<String> pausedIds = [];
  final List<String> resumedIds = [];

  @override
  Future<void> initRuntime() async {}

  @override
  Future<AppNodeInfo> initMobileEngine({
    required String deviceName,
    required String downloadDir,
    required List<String> relayAddrs,
    required AppTransferProfile profile,
    required List<String> overrideLanAddrs,
  }) async {
    return AppNodeInfo(
      nodeId: 'testnodeid',
      shortId: 'testnode',
      deviceName: deviceName,
      listenAddr: '0.0.0.0:5555',
      listenPort: 5555,
      defaultDownloadDir: downloadDir,
    );
  }

  @override
  Stream<AppEngineEvent> subscribeEvents() => events.stream;

  @override
  Future<AppStartSessionResult> startSession({
    required List<String> paths,
    String? targetPeerNodeId,
    required List<String> relayHints,
  }) async {
    return const AppStartSessionResult(
      transferId: 'tr1',
      ticketCode: 'veloce1testcode',
      qrUri: 'veloce://veloce1testcode',
      totalSize: 2048,
      totalChunks: 2,
      chunkSize: 1024,
      fileCount: 1,
      files: [AppFileSummary(relativePath: 'photo.jpg', size: 2048)],
    );
  }

  @override
  Future<AppJoinSessionResult> joinSession({
    required String ticketCode,
    String? outputDir,
    int? parallelStreams,
  }) async {
    return const AppJoinSessionResult(
      transferId: 'tr2',
      totalSize: 4096,
      fileCount: 1,
    );
  }

  @override
  AppTicketPreview parseTicket({required String ticketCode}) {
    return const AppTicketPreview(
      transferId: 'tr2',
      senderNodeId: 'senderfull',
      senderShortId: 'sender12',
      totalSize: 4096,
      fileCount: 1,
      directAddrs: ['192.168.1.5:5555'],
      relayHints: ['relay.veloce.dev:7878'],
      merkleRootHex: 'deadbeef',
    );
  }

  @override
  List<AppPeer> listLocalPeers() => const [];

  @override
  void acceptIncomingTransfer({required String transferId, String? outputDir}) {}

  @override
  void rejectIncomingTransfer({required String transferId, String? reason}) {}

  @override
  bool pauseTransfer({required String transferId}) {
    pausedIds.add(transferId);
    events.add(AppEventTransferPaused(transferId: transferId, direction: 'receive'));
    return true;
  }

  @override
  Future<bool> resumeTransfer({required String transferId}) async {
    resumedIds.add(transferId);
    events.add(AppEventTransferResumed(transferId: transferId, direction: 'receive'));
    return true;
  }

  @override
  bool cancelTransfer({required String transferId}) => true;

  @override
  void shutdown() {
    unawaited(events.close());
  }
}

void main() {
  test('formatters produce human-readable byte, speed, and ETA strings', () {
    expect(formatBytes(0), '0 B');
    expect(formatBytes(1024 * 1024), '1.00 MB');
    expect(formatSpeed(10 * 1024 * 1024), '10.0 MB/s');
    expect(formatEta(75), '1m 15s');
  });

  testWidgets('ConnectionBadge renders LAN, Direct P2P (UDP), and Relayed labels',
      (tester) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: Column(
            children: [
              ConnectionBadge(connectionType: 'LAN'),
              ConnectionBadge(connectionType: 'Direct-P2P'),
              ConnectionBadge(connectionType: 'Relayed'),
            ],
          ),
        ),
      ),
    );

    expect(find.text('LAN'), findsOneWidget);
    expect(find.text('Direct P2P (UDP)'), findsOneWidget);
    expect(find.text('Relayed'), findsOneWidget);
  });

  testWidgets('TransferCard invokes Pause and Resume callbacks cleanly', (tester) async {
    var paused = false;
    var resumed = false;

    const activeItem = TransferItem(
      transferId: 'abc',
      label: 'video.mp4',
      direction: 'receive',
      status: TransferStatus.active,
      bytesTransferred: 512 * 1024,
      totalBytes: 1024 * 1024,
      chunksVerified: 1,
      totalChunks: 2,
      speedBytesPerSec: 8 * 1024 * 1024,
      etaSeconds: 2,
      connectionType: 'Direct-P2P',
    );

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: TransferCard(
            item: activeItem,
            onPause: () => paused = true,
            onResume: () => resumed = true,
            onCancel: () {},
          ),
        ),
      ),
    );

    await tester.tap(find.byTooltip('Pause transfer'));
    expect(paused, isTrue);

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: TransferCard(
            item: activeItem.copyWith(status: TransferStatus.paused),
            onPause: () {},
            onResume: () => resumed = true,
            onCancel: () {},
          ),
        ),
      ),
    );

    await tester.tap(find.byTooltip('Resume transfer'));
    expect(resumed, isTrue);
  });

  test('VeloceController tracks progress, live path upgrades, and pause/resume events',
      () async {
    final fake = FakeRustBridge();
    final ctrl = VeloceController(bridge: fake, enableWakelock: false);

    ctrl.handleEngineEvent(
      const AppEventTransferProgress(
        transferId: 't1',
        direction: 'receive',
        bytesTransferred: 1024,
        totalBytes: 4096,
        chunksVerified: 1,
        totalChunks: 4,
        speedBytesPerSec: 2048,
        etaSeconds: 1.5,
        connectionType: 'Relayed',
      ),
    );
    expect(ctrl.transfers.single.connectionType, 'Relayed');

    ctrl.handleEngineEvent(
      const AppEventConnectionTypeChanged(
        transferId: 't1',
        peerNodeId: 'peer1',
        connectionType: 'Direct-P2P',
      ),
    );
    expect(ctrl.transfers.single.connectionType, 'Direct-P2P');

    ctrl.pauseTransfer('t1');
    expect(fake.pausedIds, ['t1']);
    ctrl.dispose();
  });
}
