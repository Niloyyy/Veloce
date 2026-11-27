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
  bool _textExpanded = false;

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
          'Selected file is stored in the cloud. Please download it to your device first.',
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
    if (mounted) {
      setState(() {
        if (text.isNotEmpty) {
          _quickTextController.text = text;
        }
        _textExpanded = true;
      });
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
        setState(() {
          _quickTextController.clear();
          _textExpanded = false;
        });
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
                        size: 36,
                        color: scheme.primary,
                      ),
                      const SizedBox(height: 8),
                      Text(
                        'Choose Files or Photos to Send',
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                      const SizedBox(height: 4),
                      Text(
                        'Send directly over Wi-Fi or anywhere with a share code.',
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
                              label: const Text('Select Files'),
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
                                  '${selected.length} ${selected.length == 1 ? 'file selected' : 'files selected'}',
                                  style: Theme.of(context)
                                      .textTheme
                                      .titleSmall
                                      ?.copyWith(fontWeight: FontWeight.w700),
                                ),
                                Text(
                                  'Tap × to unselect any file',
                                  style: Theme.of(context).textTheme.bodySmall,
                                ),
                              ],
                            ),
                          ),
                          TextButton.icon(
                            onPressed: ctrl.busy ? null : ctrl.clearSelectedPaths,
                            icon: Icon(Icons.delete_sweep_outlined,
                                size: 17, color: scheme.error),
                            label: Text('Clear', style: TextStyle(color: scheme.error)),
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
                              label: const Text('+ Photos'),
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
              'Nearby Devices',
              style: Theme.of(context).textTheme.titleSmall,
            ),
            Text(
              ctrl.peers.isNotEmpty
                  ? '${ctrl.peers.length} on your Wi-Fi'
                  : 'Same Wi-Fi / Hotspot',
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
                  const Icon(Icons.wifi_find_rounded, size: 20),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      'Open Veloce on another phone or computer on the same Wi-Fi — '
                      'it will appear here automatically.',
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
                                isTrusted
                                    ? 'Trusted • Ready on Wi-Fi'
                                    : 'Ready on Wi-Fi',
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: Theme.of(context).textTheme.bodySmall,
                              ),
                            ],
                          ),
                        ),
                        IconButton(
                          tooltip: isTrusted
                              ? 'Stop auto-accepting from ${peer.deviceName}'
                              : 'Always accept from ${peer.deviceName}',
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
                              'Share Code Ready',
                              style: Theme.of(context).textTheme.titleSmall,
                            ),
                            Text(
                              '${session.fileCount} ${session.fileCount == 1 ? 'file' : 'files'} • ${formatBytes(session.totalSize)}',
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
                      size: 180,
                      backgroundColor: Colors.white,
                    ),
                  ),
                  const SizedBox(height: 10),
                  Text(
                    'Send this code to the recipient — they paste it in Receive (or scan the QR).',
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
                                const SnackBar(content: Text('Share code copied')),
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
                      'Keep Veloce open until the download completes.',
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
                          'Send Anywhere (Internet Link / QR)',
                          style: Theme.of(context).textTheme.titleSmall,
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 6),
                  Text(
                    'Not on the same Wi-Fi? Create a one-time share code and send it to the recipient.',
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
                            ? 'Creating...'
                            : selected.isNotEmpty
                                ? 'Create Share Code (${selected.length})'
                                : 'Create Share Code & QR',
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
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Expanded(
                      child: InkWell(
                        borderRadius: BorderRadius.circular(8),
                        onTap: () => setState(() => _textExpanded = !_textExpanded),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(vertical: 4),
                          child: Row(
                            children: [
                              Icon(
                                Icons.chat_bubble_outline_rounded,
                                size: 18,
                                color: scheme.primary,
                              ),
                              const SizedBox(width: 8),
                              Expanded(
                                child: Text(
                                  'Send a Text Note or Link',
                                  style: Theme.of(context).textTheme.titleSmall,
                                ),
                              ),
                              Icon(
                                _textExpanded
                                    ? Icons.expand_less_rounded
                                    : Icons.expand_more_rounded,
                                size: 20,
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
                    const SizedBox(width: 8),
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
                if (_textExpanded) ...[
                  const SizedBox(height: 8),
                  TextField(
                    controller: _quickTextController,
                    minLines: 2,
                    maxLines: 4,
                    onChanged: (_) => setState(() {}),
                    decoration: const InputDecoration(
                      hintText: 'Type or paste a link, Wi-Fi password, or note...',
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
                              ? 'Tap Text on a nearby device above, or share via QR.'
                              : 'Sent privately over an encrypted connection.',
                          style: Theme.of(context).textTheme.bodySmall,
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
                      OutlinedButton.icon(
                        onPressed: ctrl.busy || !hasQuickText
                            ? null
                            : () => _beamQuickText(),
                        icon: const Icon(Icons.qr_code_rounded, size: 16),
                        label: const Text('Share QR'),
                      ),
                    ],
                  ),
                ],
              ],
            ),
          ),
        ),
      ],
    );
  }
}
