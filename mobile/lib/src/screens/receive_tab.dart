import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import '../models/veloce_models.dart';
import '../state/veloce_controller.dart';
import '../utils/formatters.dart';

class ReceiveTab extends StatefulWidget {
  const ReceiveTab({
    super.key,
    required this.controller,
    this.onJoinedSession,
  });

  final VeloceController controller;
  final VoidCallback? onJoinedSession;

  @override
  State<ReceiveTab> createState() => _ReceiveTabState();
}

class _ReceiveTabState extends State<ReceiveTab> {
  final TextEditingController _ticketController = TextEditingController();
  AppTicketPreview? _preview;
  String? _parseError;
  Timer? _debounce;

  @override
  void dispose() {
    _debounce?.cancel();
    _ticketController.dispose();
    super.dispose();
  }

  void _onTicketChanged(String value) {
    _debounce?.cancel();
    final trimmed = value.trim();
    if (trimmed.isEmpty) {
      setState(() {
        _preview = null;
        _parseError = null;
      });
      return;
    }
    _debounce = Timer(const Duration(milliseconds: 150), () {
      try {
        final parsed = widget.controller.parseTicket(trimmed);
        if (mounted) {
          setState(() {
            _preview = parsed;
            _parseError = null;
          });
        }
      } catch (err) {
        if (mounted) {
          setState(() {
            _preview = null;
            _parseError = err
                .toString()
                .replaceFirst(RegExp(r'^AnyhowException\((.*)\)$', dotAll: true), r'$1');
          });
        }
      }
    });
  }

  Future<void> _pasteFromClipboard() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final text = data?.text?.trim();
    if (text != null && text.isNotEmpty) {
      _ticketController.text = text;
      _onTicketChanged(text);
    }
  }

  Future<void> _openQrScanner() async {
    final scanned = await Navigator.of(context).push<String>(
      MaterialPageRoute(builder: (_) => const _QrScannerPage()),
    );
    if (scanned != null && scanned.isNotEmpty) {
      _ticketController.text = scanned;
      _onTicketChanged(scanned);
    }
  }

  Future<void> _joinSession() async {
    final code = _ticketController.text.trim();
    if (code.isEmpty || _preview == null) return;
    final ok = await widget.controller.joinSession(code);
    if (!ok) return;
    _ticketController.clear();
    if (mounted) {
      setState(() {
        _preview = null;
        _parseError = null;
      });
      widget.onJoinedSession?.call();
    }
  }

  @override
  Widget build(BuildContext context) {
    final ctrl = widget.controller;
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Text(
                      'Join Global Session',
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        IconButton(
                          tooltip: 'Paste ticket from clipboard',
                          onPressed: _pasteFromClipboard,
                          icon: const Icon(Icons.content_paste_rounded, size: 19),
                        ),
                        FilledButton.tonalIcon(
                          onPressed: _openQrScanner,
                          icon: const Icon(Icons.qr_code_scanner_rounded, size: 18),
                          label: const Text('Scan QR'),
                        ),
                      ],
                    ),
                  ],
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _ticketController,
                  minLines: 2,
                  maxLines: 4,
                  onChanged: _onTicketChanged,
                  style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
                  decoration: InputDecoration(
                    labelText: 'Session Code (veloce1...)',
                    hintText: 'Paste a session ticket or scan a QR code',
                    border: const OutlineInputBorder(),
                    errorText: _parseError,
                    suffixIcon: _ticketController.text.isNotEmpty
                        ? IconButton(
                            tooltip: 'Clear input',
                            icon: const Icon(Icons.clear_rounded, size: 18),
                            onPressed: () {
                              _ticketController.clear();
                              _onTicketChanged('');
                            },
                          )
                        : null,
                  ),
                ),
                if (_preview != null) ...[
                  const SizedBox(height: 12),
                  Container(
                    width: double.infinity,
                    padding: const EdgeInsets.all(12),
                    decoration: BoxDecoration(
                      color: Theme.of(context).colorScheme.surfaceContainerHighest,
                      borderRadius: BorderRadius.circular(10),
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          mainAxisAlignment: MainAxisAlignment.spaceBetween,
                          children: [
                            const Row(
                              children: [
                                Icon(Icons.verified_user_rounded,
                                    size: 15, color: Color(0xFF10B981)),
                                SizedBox(width: 6),
                                Text(
                                  'Valid BLAKE3 Ticket',
                                  style: TextStyle(
                                    color: Color(0xFF10B981),
                                    fontWeight: FontWeight.w700,
                                    fontSize: 12,
                                  ),
                                ),
                              ],
                            ),
                            Text(
                              '${_preview!.fileCount} item(s) • ${formatBytes(_preview!.totalSize)}',
                              style: Theme.of(context).textTheme.labelMedium,
                            ),
                          ],
                        ),
                        const SizedBox(height: 6),
                        Text(
                          'Sender: ${_preview!.senderShortId} • '
                          '${_preview!.directAddrs.length} direct endpoint(s), '
                          '${_preview!.relayHints.length} relay(s)',
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                      ],
                    ),
                  ),
                ],
                const SizedBox(height: 12),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                  decoration: BoxDecoration(
                    color: Theme.of(context).colorScheme.surfaceContainerHighest,
                    borderRadius: BorderRadius.circular(10),
                  ),
                  child: Row(
                    children: [
                      const Icon(Icons.folder_outlined, size: 17),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          'Save to: ${ctrl.settings.effectiveSaveLabel}',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                      ),
                      TextButton.icon(
                        onPressed: ctrl.busy ? null : ctrl.chooseCustomSaveFolder,
                        icon: const Icon(Icons.folder_open_rounded, size: 16),
                        label: const Text('Change'),
                        style: TextButton.styleFrom(
                          visualDensity: VisualDensity.compact,
                        ),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 14),
                SizedBox(
                  width: double.infinity,
                  child: FilledButton.icon(
                    onPressed: ctrl.busy || _preview == null ? null : _joinSession,
                    icon: const Icon(Icons.download_rounded, size: 18),
                    label: Text(ctrl.busy ? 'Connecting...' : 'Download & Verify'),
                  ),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 14),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Row(
              children: [
                const Icon(Icons.wifi_tethering_rounded, color: Color(0xFF10B981)),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Listening for LAN Offers',
                        style: Theme.of(context).textTheme.titleSmall,
                      ),
                      const SizedBox(height: 2),
                      Text(
                        'Nearby devices on Wi-Fi or Hotspot can send directly. '
                        'Incoming offers prompt for approval before writing any data.',
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

class _QrScannerPage extends StatefulWidget {
  const _QrScannerPage();

  @override
  State<_QrScannerPage> createState() => _QrScannerPageState();
}

class _QrScannerPageState extends State<_QrScannerPage> {
  final MobileScannerController _scannerController = MobileScannerController(
    formats: const [BarcodeFormat.qrCode],
    detectionSpeed: DetectionSpeed.noDuplicates,
  );
  bool _handled = false;

  @override
  void dispose() {
    _scannerController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Scan Veloce QR Code'),
        actions: [
          IconButton(
            tooltip: 'Toggle flashlight',
            icon: const Icon(Icons.flashlight_on_rounded),
            onPressed: _scannerController.toggleTorch,
          ),
          IconButton(
            tooltip: 'Switch camera',
            icon: const Icon(Icons.cameraswitch_rounded),
            onPressed: _scannerController.switchCamera,
          ),
        ],
      ),
      body: MobileScanner(
        controller: _scannerController,
        onDetect: (capture) {
          if (_handled) return;
          for (final barcode in capture.barcodes) {
            final value = barcode.rawValue?.trim();
            if (value != null &&
                (value.startsWith('veloce1') || value.startsWith('veloce://'))) {
              _handled = true;
              Navigator.of(context).pop(value);
              return;
            }
          }
        },
      ),
    );
  }
}
