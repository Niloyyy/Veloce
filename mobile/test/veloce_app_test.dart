import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:veloce_mobile/src/models/veloce_models.dart';
import 'package:veloce_mobile/src/services/platform_service.dart';
import 'package:veloce_mobile/src/state/veloce_controller.dart';
import 'package:veloce_mobile/src/theme/veloce_themes.dart';
import 'package:veloce_mobile/src/utils/device_name.dart';
import 'package:veloce_mobile/src/utils/formatters.dart';
import 'package:veloce_mobile/src/utils/text_clip.dart';
import 'package:veloce_mobile/src/widgets/connection_badge.dart';
import 'package:veloce_mobile/src/widgets/settings_sheet.dart';
import 'package:veloce_mobile/src/widgets/transfer_card.dart';

class FakeRustBridge implements VeloceRustBridge {
  final StreamController<AppEngineEvent> events = StreamController.broadcast();
  final List<String> pausedIds = [];
  final List<String> resumedIds = [];
  final List<String> acceptedIds = [];

  @override
  Future<void> initRuntime() async {}

  @override
  Future<AppNodeInfo> initMobileEngine({
    required String deviceName,
    required String downloadDir,
    required List<String> relayAddrs,
    required AppTransferProfile profile,
    required List<String> overrideLanAddrs,
    String? identityKeyPath,
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
  void acceptIncomingTransfer({required String transferId, String? outputDir}) {
    acceptedIds.add(transferId);
  }

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

class RecordingPlatformService extends VelocePlatformService {
  final List<List<String>> exportedBatches = [];
  final List<String?> exportedTreeUris = [];
  final List<String> releasedUris = [];
  PickedSaveFolder? nextPickedFolder;
  String? detectedDeviceName;

  @override
  Future<String?> getDetectedDeviceName() async => detectedDeviceName;

  @override
  Future<PickedSaveFolder?> pickCustomSaveFolder() async => nextPickedFolder;

  @override
  Future<void> releaseCustomSaveFolder(String uri) async {
    releasedUris.add(uri);
  }

  @override
  Future<List<String>> exportToPublicDownloads(
    List<String> sourcePaths, {
    String? customTreeUri,
    String? customDisplayPath,
    String? stagingRoot,
  }) async {
    exportedBatches.add(sourcePaths);
    exportedTreeUris.add(customTreeUri);
    final base = (customDisplayPath != null && customDisplayPath.isNotEmpty)
        ? customDisplayPath
        : 'Download/Veloce';
    return sourcePaths
        .map((p) => '$base/${p.split('/').last}')
        .toList(growable: false);
  }

  @override
  Future<List<ExportedFileEntry>> exportToPublicDownloadsWithUris(
    List<String> sourcePaths, {
    String? customTreeUri,
    String? customDisplayPath,
    String? stagingRoot,
  }) async {
    final paths = await exportToPublicDownloads(
      sourcePaths,
      customTreeUri: customTreeUri,
      customDisplayPath: customDisplayPath,
      stagingRoot: stagingRoot,
    );
    return paths
        .map(
          (p) => ExportedFileEntry(
            displayPath: p,
            openUri: 'content://veloce.test/${p.split('/').last}',
          ),
        )
        .toList(growable: false);
  }
}

void main() {
  test('formatters produce human-readable byte, speed, ETA, and elapsed duration strings', () {
    expect(formatBytes(0), '0 B');
    expect(formatBytes(1024 * 1024), '1.00 MB');
    expect(formatSpeed(10 * 1024 * 1024), '10.0 MB/s');
    expect(formatEta(75), '1m 15s');
    expect(formatDurationMs(420), '420ms');
    expect(formatDurationMs(3460), '3.5s');
    expect(formatDurationMs(75000), '1m 15s');
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
            onRemove: () {},
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
            onRemove: () {},
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

  test('VeloceController exports received completed transfers to public Downloads',
      () async {
    final fake = FakeRustBridge();
    final platform = RecordingPlatformService();
    final ctrl = VeloceController(
      bridge: fake,
      platformService: platform,
      enableWakelock: false,
    );

    ctrl.handleEngineEvent(
      const AppEventTransferCompleted(
        transferId: 'rx1',
        direction: 'receive',
        totalBytes: 2048,
        totalChunks: 2,
        elapsedMs: 120,
        averageSpeedBytesPerSec: 17000,
        outputPaths: ['/data/user/0/dev.veloce.p2p/files/Veloce/clip.mp4'],
      ),
    );
    await Future<void>.delayed(Duration.zero);

    expect(platform.exportedBatches, hasLength(1));
    expect(
      ctrl.transfers.single.outputPaths,
      ['Download/Veloce/clip.mp4'],
    );

    // Verify single-item history deletion and bulk clear history.
    expect(ctrl.hasCompletedTransfers, isTrue);
    ctrl.removeTransfer('rx1');
    expect(ctrl.transfers, isEmpty);

    ctrl.handleEngineEvent(
      const AppEventTransferFailed(
        transferId: 'rx2',
        direction: 'receive',
        error: 'could not reach peer directly or via any configured relay',
      ),
    );
    expect(ctrl.transfers, hasLength(1));
    expect(ctrl.actionError, contains('Relay server'));
    ctrl.clearCompletedTransfers();
    expect(ctrl.transfers, isEmpty);
    ctrl.dispose();
  });

  test('VeloceController selects, exports to, and resets custom SAF save folder',
      () async {
    final fake = FakeRustBridge();
    final platform = RecordingPlatformService()
      ..nextPickedFolder = const PickedSaveFolder(
        uri: 'content://com.android.externalstorage.documents/tree/primary%3ADocuments%2FWork',
        displayPath: 'Internal Storage/Documents/Work',
      );
    final ctrl = VeloceController(
      bridge: fake,
      platformService: platform,
      enableWakelock: false,
    );

    final picked = await ctrl.chooseCustomSaveFolder();
    expect(picked, isTrue);
    expect(ctrl.settings.hasCustomSaveFolder, isTrue);
    expect(ctrl.settings.effectiveSaveLabel, 'Internal Storage/Documents/Work');

    ctrl.handleEngineEvent(
      const AppEventTransferCompleted(
        transferId: 'rx_custom',
        direction: 'receive',
        totalBytes: 1024,
        totalChunks: 1,
        elapsedMs: 40,
        averageSpeedBytesPerSec: 25600,
        outputPaths: ['/data/user/0/dev.veloce.p2p/files/Veloce/report.pdf'],
      ),
    );
    await Future<void>.delayed(Duration.zero);

    expect(platform.exportedTreeUris.last, ctrl.settings.customSaveUri);
    expect(
      ctrl.transfers.single.outputPaths,
      ['Internal Storage/Documents/Work/report.pdf'],
    );

    final prevUri = ctrl.settings.customSaveUri;
    await ctrl.resetCustomSaveFolder();
    expect(ctrl.settings.hasCustomSaveFolder, isFalse);
    expect(platform.releasedUris, [prevUri]);
    ctrl.dispose();
  });

  testWidgets('SettingsSheet lets user pick and save custom destination folder',
      (tester) async {
    VeloceSettings? saved;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SettingsSheet(
            settings: const VeloceSettings(
              deviceName: 'My Phone',
              relayHost: '',
              exportToDownloads: true,
              themeMode: ThemeMode.dark,
            ),
            onPickSaveFolder: () async => const PickedSaveFolder(
              uri: 'content://tree/custom',
              displayPath: 'Downloads/MyProjects',
            ),
            onSave: (next) => saved = next,
          ),
        ),
      ),
    );

    expect(find.text('Save Destination Folder'), findsOneWidget);
    await tester.ensureVisible(find.text('Choose Folder'));
    await tester.tap(find.text('Choose Folder'));
    await tester.pumpAndSettle();
    expect(find.text('Downloads/MyProjects'), findsOneWidget);

    await tester.ensureVisible(find.text('Nord Frost'));
    await tester.tap(find.text('Nord Frost'));
    await tester.pumpAndSettle();

    await tester.ensureVisible(find.text('Save & Apply'));
    await tester.tap(find.text('Save & Apply'));
    await tester.pumpAndSettle();
    expect(saved?.customSaveUri, 'content://tree/custom');
    expect(saved?.customSaveLabel, 'Downloads/MyProjects');
    expect(saved?.effectiveThemeId, VeloceThemeId.nord);
  });

  testWidgets('TransferCard displays chunk elapsed time while active and Took duration when done',
      (tester) async {
    const activeItem = TransferItem(
      transferId: 'act1',
      label: 'archive.tar',
      direction: 'send',
      status: TransferStatus.active,
      bytesTransferred: 512 * 1024,
      totalBytes: 1024 * 1024,
      chunksVerified: 2,
      totalChunks: 4,
      speedBytesPerSec: 12 * 1024 * 1024,
      etaSeconds: 1,
      elapsedMs: 1800,
      connectionType: 'LAN',
    );
    const doneItem = TransferItem(
      transferId: 'done1',
      label: 'archive.tar',
      direction: 'send',
      status: TransferStatus.completed,
      bytesTransferred: 1024 * 1024,
      totalBytes: 1024 * 1024,
      chunksVerified: 4,
      totalChunks: 4,
      speedBytesPerSec: 12 * 1024 * 1024,
      etaSeconds: 0,
      elapsedMs: 2400,
      connectionType: 'LAN',
    );

    await tester.pumpWidget(
      MaterialApp(
        theme: buildVeloceTheme(VeloceThemeId.midnight),
        home: Scaffold(
          body: Column(
            children: [
              TransferCard(
                item: activeItem,
                onPause: () {},
                onResume: () {},
                onCancel: () {},
                onRemove: () {},
              ),
              TransferCard(
                item: doneItem,
                onPause: () {},
                onResume: () {},
                onCancel: () {},
                onRemove: () {},
              ),
            ],
          ),
        ),
      ),
    );

    expect(find.text('1.8s'), findsOneWidget);
    expect(find.text('Took 2.4s'), findsOneWidget);
  });

  test('device name helpers map Apple/Android models and detect legacy placeholders', () {
    expect(mapAppleModelIdentifier('iPhone16,2'), 'iPhone 15 Pro Max');
    expect(mapAppleModelIdentifier('iPhone17,1'), 'iPhone 16 Pro');
    expect(mapAppleModelIdentifier('iPad16,3'), 'iPad Pro 11-inch (M4)');
    expect(mapAppleModelIdentifier('iPhone99,1'), 'iPhone');
    expect(formatManufacturerModel('google', 'Pixel 8 Pro'), 'Pixel 8 Pro');
    expect(formatManufacturerModel('samsung', 'SM-S928B'), 'Samsung SM-S928B');
    expect(formatManufacturerModel('OnePlus', 'OnePlus 12'), 'OnePlus 12');
    expect(isLegacyDefaultDeviceName('Veloce Android'), isTrue);
    expect(isLegacyDefaultDeviceName('Veloce iPhone'), isTrue);
    expect(isLegacyDefaultDeviceName('Galaxy S24 Ultra'), isFalse);
  });

  testWidgets('SettingsSheet edits custom device name and resets to detected model',
      (tester) async {
    VeloceSettings? saved;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SettingsSheet(
            settings: const VeloceSettings(
              deviceName: 'Pixel 8 Pro',
              relayHost: '',
              exportToDownloads: true,
            ),
            detectedDeviceName: 'Pixel 8 Pro',
            onSave: (next) => saved = next,
          ),
        ),
      ),
    );

    final nameField = find.widgetWithText(TextField, 'Device Name');
    await tester.enterText(nameField, 'Neel Phone');
    await tester.pumpAndSettle();

    // Reset button appears once edited away from detected model.
    expect(find.byTooltip('Reset to Pixel 8 Pro'), findsOneWidget);

    await tester.ensureVisible(find.text('Save & Apply'));
    await tester.tap(find.text('Save & Apply'));
    await tester.pumpAndSettle();
    expect(saved?.deviceName, 'Neel Phone');
  });

  test('VeloceController migrates legacy default device name to detected hardware model',
      () async {
    TestWidgetsFlutterBinding.ensureInitialized();
    SharedPreferences.setMockInitialValues({
      'veloce.deviceName': 'Veloce Android',
    });
    final fake = FakeRustBridge();
    final platform = RecordingPlatformService()
      ..detectedDeviceName = 'Galaxy S24 Ultra';
    final ctrl = VeloceController(
      bridge: fake,
      platformService: platform,
      enableWakelock: false,
    );

    await ctrl.initialize();
    expect(ctrl.detectedDeviceName, 'Galaxy S24 Ultra');
    expect(ctrl.settings.deviceName, 'Galaxy S24 Ultra');
    expect(ctrl.nodeInfo?.deviceName, 'Galaxy S24 Ultra');

    // Editing to a custom name persists and stays respected across restarts.
    await ctrl.updateSettings(ctrl.settings.copyWith(deviceName: 'Neel S24'));
    expect(ctrl.settings.deviceName, 'Neel S24');
    expect(ctrl.nodeInfo?.deviceName, 'Neel S24');

    ctrl.dispose();

    final ctrl2 = VeloceController(
      bridge: FakeRustBridge(),
      platformService: platform,
      enableWakelock: false,
    );
    await ctrl2.initialize();
    expect(ctrl2.settings.deviceName, 'Neel S24');
    ctrl2.dispose();
  });

  test('text clip helpers identify clips, extract links, and build labels', () {
    expect(isTextClipPath('/tmp/veloce-clip-message.txt'), isTrue);
    expect(isTextClipPath('/tmp/report.txt'), isFalse);
    expect(
      extractFirstHttpUrl('Check out https://github.com/Niloyyy/Veloce!'),
      'https://github.com/Niloyyy/Veloce',
    );
    expect(extractFirstHttpUrl('just plain text'), isNull);
    expect(formatTextClipLabel('https://example.com'), startsWith('Link:'));
    expect(formatTextClipLabel('wifi-pass-123'), startsWith('Text:'));
  });

  test('VeloceController auto-accepts trusted Ed25519 peers and reads received text clips',
      () async {
    TestWidgetsFlutterBinding.ensureInitialized();
    SharedPreferences.setMockInitialValues({});
    final fake = FakeRustBridge();
    final platform = RecordingPlatformService();
    final ctrl = VeloceController(
      bridge: fake,
      platformService: platform,
      enableWakelock: false,
    );

    // 1. Untrusted sender queues an incoming offer for manual approval.
    ctrl.handleEngineEvent(
      const AppEventIncomingOffer(
        transferId: 'offer1',
        senderNodeId: 'trusted_ed25519_key_abc',
        senderShortId: 'trusted1',
        senderName: 'ThinkPad X1',
        fileCount: 1,
        totalSize: 1024,
        files: [AppFileSummary(relativePath: 'notes.pdf', size: 1024)],
        connectionType: 'LAN',
      ),
    );
    expect(ctrl.pendingOffer?.transferId, 'offer1');
    expect(fake.acceptedIds, isEmpty);

    // Accept and pin sender as trusted.
    await ctrl.acceptPendingOffer(trustSender: true);
    expect(fake.acceptedIds, ['offer1']);
    expect(ctrl.isPeerTrusted('trusted_ed25519_key_abc'), isTrue);

    // 2. Subsequent offer from the same Ed25519 Node ID auto-accepts immediately!
    ctrl.handleEngineEvent(
      const AppEventIncomingOffer(
        transferId: 'offer2',
        senderNodeId: 'trusted_ed25519_key_abc',
        senderShortId: 'trusted1',
        senderName: 'ThinkPad X1',
        fileCount: 1,
        totalSize: 32,
        files: [AppFileSummary(relativePath: kVeloceTextClipFilename, size: 32)],
        connectionType: 'LAN',
      ),
    );
    expect(ctrl.pendingOffer, isNull);
    expect(fake.acceptedIds, ['offer1', 'offer2']);

    // 3. Completing a received text clip reads textContent, cleans temp file, and skips Downloads export.
    final tmpDir = await Directory.systemTemp.createTemp('veloce_clip_test_');
    final clipFile = File('${tmpDir.path}/$kVeloceTextClipFilename');
    await clipFile.writeAsString('https://veloce.dev/demo');

    ctrl.handleEngineEvent(
      AppEventTransferCompleted(
        transferId: 'offer2',
        direction: 'receive',
        totalBytes: 23,
        totalChunks: 1,
        elapsedMs: 12,
        averageSpeedBytesPerSec: 2048,
        outputPaths: [clipFile.path],
      ),
    );
    await Future<void>.delayed(const Duration(milliseconds: 50));

    final receivedClip = ctrl.transfers.firstWhere((t) => t.transferId == 'offer2');
    expect(receivedClip.textContent, 'https://veloce.dev/demo');
    expect(receivedClip.outputPaths, isEmpty);
    expect(platform.exportedBatches, isEmpty);
    expect(await clipFile.exists(), isFalse);

    await tmpDir.delete(recursive: true);
    ctrl.dispose();
  });

  testWidgets('TransferCard renders text clip Copy/Open Link and received file Open/Folder actions',
      (tester) async {
    String? openedUrl;
    var openedFile = false;
    var openedFolder = false;

    const clipItem = TransferItem(
      transferId: 'clip1',
      label: 'Received Link: https://github.com/Niloyyy/Veloce',
      direction: 'receive',
      status: TransferStatus.completed,
      bytesTransferred: 34,
      totalBytes: 34,
      chunksVerified: 1,
      totalChunks: 1,
      speedBytesPerSec: 1024,
      etaSeconds: 0,
      elapsedMs: 15,
      connectionType: 'LAN',
      textContent: 'https://github.com/Niloyyy/Veloce',
    );
    const fileItem = TransferItem(
      transferId: 'file1',
      label: 'photo.jpg',
      direction: 'receive',
      status: TransferStatus.completed,
      bytesTransferred: 4096,
      totalBytes: 4096,
      chunksVerified: 1,
      totalChunks: 1,
      speedBytesPerSec: 4096,
      etaSeconds: 0,
      elapsedMs: 25,
      connectionType: 'LAN',
      outputPaths: ['Download/Veloce/photo.jpg'],
      openUris: ['content://media/external/downloads/42'],
    );

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SingleChildScrollView(
            child: Column(
              children: [
                TransferCard(
                  item: clipItem,
                  onPause: () {},
                  onResume: () {},
                  onCancel: () {},
                  onRemove: () {},
                  onOpenUrl: (u) => openedUrl = u,
                ),
                TransferCard(
                  item: fileItem,
                  onPause: () {},
                  onResume: () {},
                  onCancel: () {},
                  onRemove: () {},
                  onOpenFile: () => openedFile = true,
                  onOpenFolder: () => openedFolder = true,
                ),
              ],
            ),
          ),
        ),
      ),
    );

    expect(find.text('Copy Text'), findsOneWidget);
    expect(find.text('Open Link'), findsOneWidget);
    await tester.tap(find.text('Open Link'));
    expect(openedUrl, 'https://github.com/Niloyyy/Veloce');

    expect(find.text('Open'), findsOneWidget);
    expect(find.text('Folder'), findsOneWidget);
    await tester.tap(find.text('Open'));
    await tester.tap(find.text('Folder'));
    expect(openedFile, isTrue);
    expect(openedFolder, isTrue);
  });
}
