import 'package:flutter/material.dart';

import 'src/screens/receive_tab.dart';
import 'src/screens/send_tab.dart';
import 'src/screens/transfers_tab.dart';
import 'src/services/frb_rust_bridge.dart';
import 'src/state/veloce_controller.dart';
import 'src/widgets/incoming_offer_sheet.dart';
import 'src/widgets/settings_sheet.dart';

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
          builder: (ctx) => IncomingOfferSheet(
            offer: offer,
            onAccept: () {
              Navigator.of(ctx).pop();
              _controller.acceptPendingOffer();
              setState(() => _tabIndex = 2);
            },
            onReject: () {
              Navigator.of(ctx).pop();
              _controller.rejectPendingOffer();
            },
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
        const seed = Color(0xFF3B82F6);
        return MaterialApp(
          title: 'Veloce',
          debugShowCheckedModeBanner: false,
          themeMode: _controller.settings.themeMode,
          theme: ThemeData(
            useMaterial3: true,
            colorScheme: ColorScheme.fromSeed(seedColor: seed, brightness: Brightness.light),
          ),
          darkTheme: ThemeData(
            useMaterial3: true,
            colorScheme: ColorScheme.fromSeed(
              seedColor: seed,
              brightness: Brightness.dark,
              surface: const Color(0xFF0C0F14),
            ),
          ),
          home: Builder(
            builder: (innerCtx) => Scaffold(
              appBar: AppBar(
                title: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    const Text('Veloce', style: TextStyle(fontWeight: FontWeight.w700)),
                    if (_controller.nodeInfo != null)
                      Text(
                        '${_controller.nodeInfo!.deviceName} • ${_controller.nodeInfo!.shortId}',
                        style: Theme.of(innerCtx).textTheme.labelSmall,
                      ),
                  ],
                ),
                actions: [
                  IconButton(
                    tooltip: 'Toggle theme',
                    icon: Icon(
                      _controller.settings.themeMode == ThemeMode.dark
                          ? Icons.light_mode_outlined
                          : Icons.dark_mode_outlined,
                    ),
                    onPressed: _controller.toggleTheme,
                  ),
                  IconButton(
                    tooltip: 'Settings',
                    icon: const Icon(Icons.tune_rounded),
                    onPressed: () {
                      showModalBottomSheet<void>(
                        context: innerCtx,
                        isScrollControlled: true,
                        builder: (_) => SettingsSheet(
                          settings: _controller.settings,
                          onSave: _controller.updateSettings,
                        ),
                      );
                    },
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
                    child: IndexedStack(
                      index: _tabIndex,
                      children: [
                        SendTab(
                          controller: _controller,
                          onSwitchToTransfers: () => setState(() => _tabIndex = 2),
                        ),
                        ReceiveTab(
                          controller: _controller,
                          onJoinedSession: () => setState(() => _tabIndex = 2),
                        ),
                        TransfersTab(controller: _controller),
                      ],
                    ),
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
            ),
          ),
        );
      },
    );
  }
}
