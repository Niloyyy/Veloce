import 'package:flutter/material.dart';

import 'src/screens/receive_tab.dart';
import 'src/screens/send_tab.dart';
import 'src/screens/transfers_tab.dart';
import 'src/services/frb_rust_bridge.dart';
import 'src/state/veloce_controller.dart';
import 'src/theme/veloce_themes.dart';
import 'src/widgets/incoming_offer_sheet.dart';
import 'src/widgets/settings_sheet.dart';
import 'src/widgets/veloce_logo.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const VeloceMobileApp());
}

class VeloceMobileApp extends StatefulWidget {
  const VeloceMobileApp({super.key, this.controller});

  final VeloceController? controller;

  @override
  State<VeloceMobileApp> createState() => _VeloceMobileAppState();
}

class _VeloceMobileAppState extends State<VeloceMobileApp> {
  late final VeloceController _controller;
  late final bool _ownsController;
  int _tabIndex = 0;
  String? _shownOfferId;

  @override
  void initState() {
    super.initState();
    _ownsController = widget.controller == null;
    _controller = widget.controller ?? VeloceController(bridge: FrbVeloceBridge.instance);
    _controller.addListener(_onControllerChanged);
    if (_ownsController) {
      _controller.initialize();
    }
  }

  void _onControllerChanged() {
    final offer = _controller.pendingOffer;
    if (offer != null && offer.transferId != _shownOfferId && mounted) {
      _shownOfferId = offer.transferId;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted || _controller.pendingOffer == null) return;
        showModalBottomSheet<void>(
          context: context,
          isDismissible: false,
          builder: (ctx) => ListenableBuilder(
            listenable: _controller,
            builder: (_, __) => IncomingOfferSheet(
              offer: offer,
              saveDestinationLabel: _controller.settings.effectiveSaveLabel,
              onChooseSaveFolder: () => _controller.chooseCustomSaveFolder(),
              onAccept: (trustSender) {
                Navigator.of(ctx).pop();
                _controller.acceptPendingOffer(trustSender: trustSender);
                setState(() => _tabIndex = 2);
              },
              onReject: () {
                Navigator.of(ctx).pop();
                _controller.rejectPendingOffer();
              },
            ),
          ),
        );
      });
    } else if (offer == null) {
      _shownOfferId = null;
    }
  }

  @override
  void dispose() {
    _controller.removeListener(_onControllerChanged);
    if (_ownsController) {
      _controller.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: _controller,
      builder: (context, _) {
        final activeTheme = _controller.activeThemeId;
        return MaterialApp(
          title: 'Veloce',
          debugShowCheckedModeBanner: false,
          theme: buildVeloceTheme(activeTheme),
          home: Builder(
            builder: (innerCtx) {
              void openSettings() {
                showModalBottomSheet<void>(
                  context: innerCtx,
                  isScrollControlled: true,
                  builder: (_) => ListenableBuilder(
                    listenable: _controller,
                    builder: (_, __) => SettingsSheet(
                      settings: _controller.settings,
                      detectedDeviceName: _controller.detectedDeviceName,
                      trustedPeers: _controller.trustedPeers,
                      onUntrustPeer: _controller.untrustPeer,
                      onPreviewTheme: _controller.previewTheme,
                      onSave: _controller.updateSettings,
                      onPickSaveFolder: _controller.pickSaveFolderOnly,
                    ),
                  ),
                ).whenComplete(() => _controller.previewTheme(null));
              }

              return Scaffold(
              appBar: AppBar(
                title: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    const VeloceLogoMark(size: 30),
                    const SizedBox(width: 10),
                    Flexible(
                      child: InkWell(
                        borderRadius: BorderRadius.circular(8),
                        onTap: openSettings,
                        child: Padding(
                          padding: const EdgeInsets.symmetric(vertical: 2, horizontal: 2),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              const Text('Veloce', style: TextStyle(fontWeight: FontWeight.w700)),
                              if (_controller.nodeInfo != null)
                                Row(
                                  mainAxisSize: MainAxisSize.min,
                                  children: [
                                    Container(
                                      width: 7,
                                      height: 7,
                                      margin: const EdgeInsets.only(right: 5),
                                      decoration: const BoxDecoration(
                                        color: Color(0xFF10B981),
                                        shape: BoxShape.circle,
                                      ),
                                    ),
                                    Flexible(
                                      child: Text(
                                        _controller.nodeInfo!.deviceName,
                                        overflow: TextOverflow.ellipsis,
                                        style: Theme.of(innerCtx).textTheme.labelSmall,
                                      ),
                                    ),
                                    const SizedBox(width: 4),
                                    Icon(
                                      Icons.edit_outlined,
                                      size: 12,
                                      color: Theme.of(innerCtx).colorScheme.onSurfaceVariant,
                                    ),
                                  ],
                                ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
                actions: [
                  IconButton(
                    tooltip: 'Theme: ${activeTheme.label} (tap to switch)',
                    icon: const Icon(Icons.palette_outlined),
                    onPressed: _controller.cycleTheme,
                  ),
                  IconButton(
                    tooltip: 'Settings',
                    icon: const Icon(Icons.tune_rounded),
                    onPressed: openSettings,
                  ),
                ],
              ),
              body: Column(
                children: [
                  if (_controller.initError != null)
                    MaterialBanner(
                      content: Text(_controller.initError!),
                      actions: [
                        TextButton(
                          onPressed: _controller.initialize,
                          child: const Text('Retry'),
                        ),
                      ],
                    ),
                  if (_controller.actionError != null)
                    MaterialBanner(
                      content: Text(_controller.actionError!),
                      actions: [
                        TextButton(
                          onPressed: _controller.clearActionError,
                          child: const Text('Dismiss'),
                        ),
                      ],
                    ),
                  Expanded(
                    child: switch (_tabIndex) {
                      0 => SendTab(
                          controller: _controller,
                          onSwitchToTransfers: () => setState(() => _tabIndex = 2),
                        ),
                      1 => ReceiveTab(
                          controller: _controller,
                          onJoinedSession: () => setState(() => _tabIndex = 2),
                        ),
                      _ => TransfersTab(controller: _controller),
                    },
                  ),
                ],
              ),
              bottomNavigationBar: NavigationBar(
                selectedIndex: _tabIndex,
                onDestinationSelected: (idx) => setState(() => _tabIndex = idx),
                destinations: [
                  const NavigationDestination(
                    icon: Icon(Icons.north_east_rounded),
                    label: 'Send',
                  ),
                  const NavigationDestination(
                    icon: Icon(Icons.south_west_rounded),
                    label: 'Receive',
                  ),
                  NavigationDestination(
                    icon: Badge(
                      isLabelVisible: _controller.activeTransferCount > 0,
                      label: Text('${_controller.activeTransferCount}'),
                      child: const Icon(Icons.swap_vert_rounded),
                    ),
                    label: 'Transfers',
                  ),
                ],
              ),
            );
            },
          ),
        );
      },
    );
  }
}
