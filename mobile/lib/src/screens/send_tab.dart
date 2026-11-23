import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:share_plus/share_plus.dart';

import '../rust/api.dart';
import '../state/veloce_controller.dart';
import '../utils/formatters.dart';

class SendTab extends StatefulWidget {
  const SendTab({super.key, required this.controller});

  final VeloceController controller;

  @override
  State<SendTab> createState() => _SendTabState();
}

class _SendTabState extends State<SendTab> {
  List<String> _selectedPaths = const [];

  Future<void> _pickAndHost({FileType type = FileType.any, String? targetPeer}) async {
    final result = await FilePicker.platform.pickFiles(
      allowMultiple: true,
      type: type,
    );
    if (result == null) return;
    final paths = result.paths.whereType<String>().toList();
    if (paths.isEmpty) return;
    setState(() => _selectedPaths = paths);
    await widget.controller.startSession(paths, targetPeerNodeId: targetPeer);
  }

  Future<void> _sendToLanPeer(MobilePeer peer) async {
    if (_selectedPaths.isEmpty) {
      await _pickAndHost(targetPeer: peer.nodeId);
      return;
    }
    await widget.controller.startSession(_selectedPaths, targetPeerNodeId: peer.nodeId);
  }

  @override
  Widget build(BuildContext context) {
    final ctrl = widget.controller;
    final session = ctrl.activeSession;

    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Card(
          child: Padding(
            padding: const EdgeInsets.all(20),
            child: Column(
              children: [
                Icon(
                  Icons.cloud_upload_outlined,
                  size: 42,
                  color: Theme.of(context).colorScheme.primary,
                ),
                const SizedBox(height: 10),
                Text(
                  'Share Files or Media',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                const SizedBox(height: 4),
                Text(
                  'BLAKE3 Merkle verified • Mobile QUIC (BBR) • 4 streams',
                  textAlign: TextAlign.center,
                  style: Theme.of(context).textTheme.bodySmall,
                ),
                const SizedBox(height: 16),
                Row(
                  children: [
                    Expanded(
                      child: FilledButton.icon(
                        onPressed: ctrl.busy ? null : () => _pickAndHost(),
                        icon: const Icon(Icons.insert_drive_file_outlined, size: 18),
                        label: Text(ctrl.busy ? 'Hashing...' : 'Pick Files'),
                      ),
                    ),
                    const SizedBox(width: 10),
                    Expanded(
                      child: OutlinedButton.icon(
                        onPressed: ctrl.busy
                            ? null
                            : () => _pickAndHost(type: FileType.media),
                        icon: const Icon(Icons.photo_library_outlined, size: 18),
                        label: const Text('Photos / Video'),
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
        if (session != null) ...[
          const SizedBox(height: 14),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.center,
                children: [
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      Text(
                        'Global Session Ticket',
                        style: Theme.of(context).textTheme.titleSmall,
                      ),
                      Text(
                        '${session.fileCount} item(s) • ${formatBytes(session.totalSize)}',
                        style: Theme.of(context).textTheme.labelSmall,
                      ),
                    ],
                  ),
                  const SizedBox(height: 14),
                  Container(
                    padding: const EdgeInsets.all(10),
                    decoration: BoxDecoration(
                      color: Colors.white,
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: QrImageView(
                      data: session.qrUri,
                      size: 190,
                      backgroundColor: Colors.white,
                    ),
                  ),
                  const SizedBox(height: 12),
                  Container(
                    width: double.infinity,
                    padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                    decoration: BoxDecoration(
                      color: Theme.of(context).colorScheme.surfaceContainerHighest,
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
                                const SnackBar(content: Text('Session ticket copied')),
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
                ],
              ),
            ),
          ),
        ],
        const SizedBox(height: 16),
        Text(
          'Nearby LAN Devices (mDNS)',
          style: Theme.of(context).textTheme.titleSmall,
        ),
        const SizedBox(height: 8),
        if (ctrl.peers.isEmpty)
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Row(
                children: [
                  const Icon(Icons.radar_rounded),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      'Scanning _veloce._udp.local. on Wi-Fi for nearby devices...',
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ),
                ],
              ),
            ),
          )
        else
          for (final peer in ctrl.peers)
            Card(
              margin: const EdgeInsets.only(bottom: 8),
              child: ListTile(
                leading: const CircleAvatar(child: Icon(Icons.devices_rounded, size: 20)),
                title: Text(peer.deviceName),
                subtitle: Text(
                  '${peer.shortId} • ${peer.addresses.isNotEmpty ? peer.addresses.first : 'LAN'}',
                ),
                trailing: FilledButton.tonal(
                  onPressed: ctrl.busy ? null : () => _sendToLanPeer(peer),
                  child: const Text('Send'),
                ),
              ),
            ),
      ],
    );
  }
}
