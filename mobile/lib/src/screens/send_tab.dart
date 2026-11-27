import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:share_plus/share_plus.dart';

import '../models/veloce_models.dart';
import '../state/veloce_controller.dart';
import '../utils/formatters.dart';

class SendTab extends StatefulWidget {
  const SendTab({
    super.key,
    required this.controller,
    this.onSwitchToTransfers,
  });

  final VeloceController controller;
  final VoidCallback? onSwitchToTransfers;

  @override
  State<SendTab> createState() => _SendTabState();
}

class _SendTabState extends State<SendTab> {
  final TextEditingController _quickTextController = TextEditingController();

  @override
  void dispose() {
    _quickTextController.dispose();
    super.dispose();
  }

  String _baseName(String path) {
    final parts = path.split(RegExp(r'[\\/]')).where((s) => s.isNotEmpty).toList();
    return parts.isNotEmpty ? parts.last : path;
  }

  Future<List<String>> _pickPaths({FileType type = FileType.any}) async {
    try {
      final result = await FilePicker.platform.pickFiles(
        allowMultiple: true,
        type: type,
        withData: false,
      );
      if (result == null) return const [];
      final resolved = <String>[];
      for (final file in result.files) {
        final path = file.path;
        if (path != null && path.isNotEmpty) {
          resolved.add(path);
        } else if (file.bytes != null) {
          final tmp = File(
            '${Directory.systemTemp.path}/${file.name.isNotEmpty ? file.name : 'shared_item.bin'}',
          );
          await tmp.writeAsBytes(file.bytes!, flush: true);
          resolved.add(tmp.path);
        }
      }
      if (resolved.isEmpty && result.files.isNotEmpty) {
        widget.controller.reportError(
          'Selected file could not be read locally (cloud-only file). '
          'Please download it to your device first.',
        );
      }
      return resolved;
    } catch (err) {
      widget.controller.reportError(err);
      return const [];
    }
  }

  Future<void> _stageFiles({FileType type = FileType.any}) async {
    final paths = await _pickPaths(type: type);
    if (paths.isEmpty) return;
    widget.controller.addSelectedPaths(paths);
  }

  Future<void> _generateGlobalCode() async {
    var paths = widget.controller.selectedPaths;
    if (paths.isEmpty) {
      final picked = await _pickPaths();
      if (picked.isEmpty) return;
      widget.controller.addSelectedPaths(picked);
      paths = widget.controller.selectedPaths;
    }
    await widget.controller.startSession(paths);
  }

  Future<void> _sendToLanPeer(AppPeer peer) async {
    var paths = widget.controller.selectedPaths;
    if (paths.isEmpty) {
      final picked = await _pickPaths();
      if (picked.isEmpty) return;
      widget.controller.addSelectedPaths(picked);
      paths = widget.controller.selectedPaths;
    }
    final ok = await widget.controller.startSession(
      paths,
      targetPeerNodeId: peer.nodeId,
    );
    if (ok) {
      widget.onSwitchToTransfers?.call();
    }
  }

  Future<void> _pasteClipboard() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final text = data?.text ?? '';
    if (text.isNotEmpty && mounted) {
      setState(() => _quickTextController.text = text);
    }
  }

  Future<void> _beamQuickText({String? targetPeerNodeId}) async {
    final text = _quickTextController.text.trim();
    if (text.isEmpty) return;
    final ok = await widget.controller.sendTextClip(
      text,
      targetPeerNodeId: targetPeerNodeId,
    );
    if (ok && targetPeerNodeId != null) {
      if (mounted) {
        setState(() => _quickTextController.clear());
      }
      widget.onSwitchToTransfers?.call();
    }
  }

  @override
  Widget build(BuildContext context) {
    final ctrl = widget.controller;
    final selected = ctrl.selectedPaths;
    final session = ctrl.activeSession;
    final hasRelay = ctrl.settings.relayHost.trim().isNotEmpty;
    final hasQuickText = _quickTextController.text.trim().isNotEmpty;
    final scheme = Theme.of(context).colorScheme;

    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: selected.isEmpty
                ? Column(
                    children: [
                      Icon(
                        Icons.cloud_upload_outlined,
                        size: 38,
                        color: scheme.primary,
                      ),
                      const SizedBox(height: 8),
                      Text(
                        'Select Files or Media to Send',
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                      const SizedBox(height: 4),
                      Text(
                        'Zero-buffer streaming • Missing-chunk auto-resume • Encrypted P2P',
                        textAlign: TextAlign.center,
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                      const SizedBox(height: 14),
                      Row(
                        children: [
                          Expanded(
                            child: FilledButton.icon(
                              onPressed: ctrl.busy ? null : () => _stageFiles(),
                              icon: const Icon(Icons.insert_drive_file_outlined, size: 18),
                              label: const Text('Pick Files'),
                            ),
                          ),
                          const SizedBox(width: 10),
                          Expanded(
                            child: OutlinedButton.icon(
                              onPressed: ctrl.busy
                                  ? null
                                  : () => _stageFiles(type: FileType.media),
                              icon: const Icon(Icons.photo_library_outlined, size: 18),
                              label: const Text('Photos / Video'),
                            ),
                          ),
                        ],
                      ),
                    ],
                  )
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  '${selected.length} ${selected.length == 1 ? 'item ready' : 'items ready'}',
                                  style: Theme.of(context)
                                      .textTheme
                                      .titleSmall
                                      ?.copyWith(fontWeight: FontWeight.w700),
                                ),
                                Text(
                                  'Tap × to unselect any item',
                                  style: Theme.of(context).textTheme.bodySmall,
                                ),
                              ],
                            ),
                          ),
                          TextButton.icon(
                            onPressed: ctrl.busy ? null : ctrl.clearSelectedPaths,
                            icon: Icon(Icons.delete_sweep_outlined,
                                size: 17, color: scheme.error),
                            label: Text('Clear All', style: TextStyle(color: scheme.error)),
                            style: TextButton.styleFrom(
                              visualDensity: VisualDensity.compact,
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      ConstrainedBox(
                        constraints: const BoxConstraints(maxHeight: 176),
                        child: ListView.separated(
                          shrinkWrap: true,
                          itemCount: selected.length,
                          separatorBuilder: (_, __) => const SizedBox(height: 6),
                          itemBuilder: (context, idx) {
                            final path = selected[idx];
                            final name = _baseName(path);
                            return Container(
                              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                              decoration: BoxDecoration(
                                color: scheme.surfaceContainerHighest,
                                borderRadius: BorderRadius.circular(10),
                              ),
                              child: Row(
                                children: [
                                  Icon(Icons.insert_drive_file_outlined,
                                      size: 16, color: scheme.primary),
                                  const SizedBox(width: 8),
                                  Expanded(
                                    child: Text(
                                      name,
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                      style: Theme.of(context)
                                          .textTheme
                                          .bodySmall
                                          ?.copyWith(fontWeight: FontWeight.w600),
                                    ),
                                  ),
                                  IconButton(
                                    tooltip: 'Remove $name',
                                    icon: const Icon(Icons.close_rounded, size: 18),
                                    visualDensity: VisualDensity.compact,
                                    constraints: const BoxConstraints(
                                      minWidth: 30,
                                      minHeight: 30,
                                    ),
                                    padding: EdgeInsets.zero,
                                    onPressed: () => ctrl.removeSelectedPath(path),
                                  ),
                                ],
                              ),
                            );
                          },
                        ),
                      ),
                      const SizedBox(height: 10),
                      Row(
                        children: [
                          Expanded(
                            child: OutlinedButton.icon(
                              onPressed: ctrl.busy ? null : () => _stageFiles(),
                              icon: const Icon(Icons.add_rounded, size: 17),
                              label: const Text('+ Files'),
                            ),
                          ),
                          const SizedBox(width: 8),
                          Expanded(
                            child: OutlinedButton.icon(
                              onPressed: ctrl.busy
                                  ? null
                                  : () => _stageFiles(type: FileType.media),
                              icon: const Icon(Icons.photo_library_outlined, size: 17),
                              label: const Text('+ Media'),
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),
          ),
        ),
        const SizedBox(height: 14),
        Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children: [
            Text(
              'Nearby Devices (Same Wi-Fi / Hotspot)',
              style: Theme.of(context).textTheme.titleSmall,
            ),
            Text(
              ctrl.peers.isNotEmpty
                  ? '${ctrl.peers.length} online'
                  : 'Live ~1s sync',
              style: Theme.of(context).textTheme.labelSmall,
            ),
          ],
        ),
        const SizedBox(height: 8),
        if (ctrl.peers.isEmpty)
          Card(
            child: Padding(
              padding: const EdgeInsets.all(14),
              child: Row(
                children: [
                  const Icon(Icons.radar_rounded, size: 20),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      'Open Veloce on another device on the same Wi-Fi or Hotspot — '
                      'it appears here within a second.',
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ),
                ],
              ),
            ),
          )
        else
          for (final peer in ctrl.peers)
            Builder(
              builder: (context) {
                final isTrusted = ctrl.isPeerTrusted(peer.nodeId);
                return Card(
                  margin: const EdgeInsets.only(bottom: 8),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
                    child: Row(
                      children: [
                        const CircleAvatar(
                          child: Icon(Icons.devices_rounded, size: 20),
                        ),
                        const SizedBox(width: 12),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Row(
                                children: [
                                  Flexible(
                                    child: Text(
                                      peer.deviceName,
                                      overflow: TextOverflow.ellipsis,
                                      style: Theme.of(context)
                                          .textTheme
                                          .titleSmall
                                          ?.copyWith(fontWeight: FontWeight.w600),
                                    ),
                                  ),
                                  if (isTrusted) ...[
                                    const SizedBox(width: 6),
                                    const Icon(
                                      Icons.verified_user_rounded,
                                      size: 15,
                                      color: Color(0xFF10B981),
                                    ),
                                  ],
                                ],
                              ),
                              Text(
                                '${peer.shortId} • ${peer.addresses.isNotEmpty ? peer.addresses.first : 'LAN'}'
                                '${isTrusted ? ' • Trusted' : ''}',
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: Theme.of(context).textTheme.bodySmall,
                              ),
                            ],
                          ),
                        ),
                        IconButton(
                          tooltip: isTrusted
                              ? 'Untrust ${peer.deviceName}'
                              : 'Trust ${peer.deviceName} (auto-accept)',
                          icon: Icon(
                            isTrusted
                                ? Icons.verified_user_rounded
                                : Icons.verified_user_outlined,
                            size: 20,
                            color: isTrusted ? const Color(0xFF10B981) : null,
                          ),
                          onPressed: () => ctrl.toggleTrustPeer(peer),
                        ),
                        if (hasQuickText) ...[
                          OutlinedButton(
                            onPressed: ctrl.busy
                                ? null
                                : () => _beamQuickText(targetPeerNodeId: peer.nodeId),
                            style: OutlinedButton.styleFrom(
                              visualDensity: VisualDensity.compact,
                              padding: const EdgeInsets.symmetric(horizontal: 10),
                            ),
                            child: const Text('Text'),
                          ),
                          const SizedBox(width: 6),
                        ],
                        FilledButton.tonal(
                          onPressed: ctrl.busy ? null : () => _sendToLanPeer(peer),
                          style: FilledButton.styleFrom(
                            visualDensity: VisualDensity.compact,
                            padding: const EdgeInsets.symmetric(horizontal: 12),
                          ),
                          child: Text(
                            selected.isNotEmpty ? 'Send (${selected.length})' : 'Send',
                          ),
                        ),
                      ],
                    ),
                  ),
                );
              },
            ),
        const SizedBox(height: 12),
        if (session != null)
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.center,
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              'Global Transfer Ready',
                              style: Theme.of(context).textTheme.titleSmall,
                            ),
                            Text(
                              '${session.fileCount} item(s) • ${formatBytes(session.totalSize)}',
                              style: Theme.of(context).textTheme.labelSmall,
                            ),
                          ],
                        ),
                      ),
                      TextButton.icon(
                        onPressed: ctrl.stopHostingSession,
                        icon: const Icon(Icons.stop_circle_outlined, size: 16),
                        label: const Text('Stop'),
                      ),
                    ],
                  ),
                  const SizedBox(height: 10),
                  Container(
                    padding: const EdgeInsets.all(10),
                    decoration: BoxDecoration(
                      color: Colors.white,
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: QrImageView(
                      data: session.qrUri,
                      size: 184,
                      backgroundColor: Colors.white,
                    ),
                  ),
                  const SizedBox(height: 10),
                  Text(
                    '1. Copy or Share code below (or scan QR) • 2. Receiver opens Receive & pastes code',
                    textAlign: TextAlign.center,
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                  const SizedBox(height: 10),
                  Container(
                    width: double.infinity,
                    padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                    decoration: BoxDecoration(
                      color: scheme.surfaceContainerHighest,
                      borderRadius: BorderRadius.circular(8),
                    ),
                    child: Text(
                      session.ticketCode,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
                    ),
                  ),
                  const SizedBox(height: 10),
                  Row(
                    children: [
                      Expanded(
                        child: OutlinedButton.icon(
                          onPressed: () async {
                            await Clipboard.setData(
                              ClipboardData(text: session.ticketCode),
                            );
                            if (context.mounted) {
                              ScaffoldMessenger.of(context).showSnackBar(
                                const SnackBar(content: Text('Session code copied')),
                              );
                            }
                          },
                          icon: const Icon(Icons.copy_rounded, size: 16),
                          label: const Text('Copy Code'),
                        ),
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: FilledButton.tonalIcon(
                          onPressed: () => Share.share(session.ticketCode),
                          icon: const Icon(Icons.share_rounded, size: 16),
                          label: const Text('Share'),
                        ),
                      ),
                    ],
                  ),
                  if (!hasRelay) ...[
                    const SizedBox(height: 8),
                    Text(
                      'Keep Veloce open while sending. Works over IPv6, UPnP & STUN Direct P2P — '
                      'add a Relay Server in Settings only for IPv4-only double CGNAT.',
                      textAlign: TextAlign.center,
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ],
                ],
              ),
            ),
          )
        else
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Icon(Icons.public_rounded, size: 19, color: scheme.primary),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          'Global Transfer (Send Anywhere)',
                          style: Theme.of(context).textTheme.titleSmall,
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 6),
                  Text(
                    '1. Generate a one-time code & QR • 2. Send code to receiver anywhere • '
                    '3. Receiver pastes in Receive (automatically skips already-sent chunks).',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                  const SizedBox(height: 12),
                  SizedBox(
                    width: double.infinity,
                    child: FilledButton.icon(
                      onPressed: ctrl.busy ? null : _generateGlobalCode,
                      icon: const Icon(Icons.qr_code_2_rounded, size: 18),
                      label: Text(
                        ctrl.busy
                            ? 'Preparing...'
                            : selected.isNotEmpty
                                ? 'Generate Global Code & QR (${selected.length})'
                                : 'Pick Files & Generate Global Code',
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
        const SizedBox(height: 12),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Icon(
                      Icons.chat_bubble_outline_rounded,
                      size: 18,
                      color: scheme.primary,
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        'Quick Text / Link',
                        style: Theme.of(context).textTheme.titleSmall,
                      ),
                    ),
                    if (hasQuickText)
                      TextButton(
                        onPressed: ctrl.busy
                            ? null
                            : () => setState(() => _quickTextController.clear()),
                        style: TextButton.styleFrom(
                          visualDensity: VisualDensity.compact,
                        ),
                        child: const Text('Clear'),
                      ),
                    TextButton.icon(
                      onPressed: ctrl.busy ? null : _pasteClipboard,
                      icon: const Icon(Icons.content_paste_rounded, size: 16),
                      label: const Text('Paste'),
                      style: TextButton.styleFrom(
                        visualDensity: VisualDensity.compact,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: _quickTextController,
                  minLines: 2,
                  maxLines: 4,
                  onChanged: (_) => setState(() {}),
                  decoration: const InputDecoration(
                    hintText: 'Paste a URL, Wi-Fi password, or note to beam...',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                ),
                const SizedBox(height: 10),
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        hasQuickText
                            ? 'Tap Text on a nearby device above, or create a QR.'
                            : 'Encrypted end-to-end across LAN & Global Codes.',
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    ),
                    const SizedBox(width: 8),
                    OutlinedButton.icon(
                      onPressed: ctrl.busy || !hasQuickText
                          ? null
                          : () => _beamQuickText(),
                      icon: const Icon(Icons.qr_code_rounded, size: 16),
                      label: const Text('Text QR'),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}
