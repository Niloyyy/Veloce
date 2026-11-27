import 'package:flutter/material.dart';

import '../models/veloce_models.dart';
import '../utils/formatters.dart';
import '../utils/text_clip.dart';
import 'connection_badge.dart';

class IncomingOfferSheet extends StatefulWidget {
  const IncomingOfferSheet({
    super.key,
    required this.offer,
    required this.onAccept,
    required this.onReject,
    this.saveDestinationLabel,
    this.onChooseSaveFolder,
  });

  final AppEventIncomingOffer offer;
  final ValueChanged<bool> onAccept;
  final VoidCallback onReject;
  final String? saveDestinationLabel;
  final VoidCallback? onChooseSaveFolder;

  @override
  State<IncomingOfferSheet> createState() => _IncomingOfferSheetState();
}

class _IncomingOfferSheetState extends State<IncomingOfferSheet> {
  bool _trustSender = false;

  @override
  Widget build(BuildContext context) {
    final offer = widget.offer;
    final isTextClip = offer.fileCount == 1 &&
        offer.files.length == 1 &&
        isTextClipPath(offer.files.first.relativePath);

    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 12, 20, 20),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        isTextClip ? 'INCOMING TEXT / LINK' : 'INCOMING FILES',
                        style: Theme.of(context).textTheme.labelSmall?.copyWith(
                              color: Theme.of(context).colorScheme.primary,
                              fontWeight: FontWeight.w700,
                              letterSpacing: 0.8,
                            ),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        offer.senderName,
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                    ],
                  ),
                ),
                ConnectionBadge(connectionType: offer.connectionType),
              ],
            ),
            const SizedBox(height: 14),
            Text(
              isTextClip
                  ? 'Text / Link • ${formatBytes(offer.totalSize)} • Encrypted'
                  : '${offer.fileCount} ${offer.fileCount == 1 ? 'file' : 'files'} • ${formatBytes(offer.totalSize)} • Encrypted',
              style: Theme.of(context).textTheme.bodyMedium,
            ),
            if (!isTextClip) ...[
              const SizedBox(height: 10),
              ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 160),
                child: ListView.builder(
                  shrinkWrap: true,
                  itemCount: offer.files.length > 6 ? 7 : offer.files.length,
                  itemBuilder: (context, idx) {
                    if (idx == 6) {
                      return Padding(
                        padding: const EdgeInsets.symmetric(vertical: 4),
                        child: Text(
                          '+ ${offer.files.length - 6} more file(s)',
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                      );
                    }
                    final file = offer.files[idx];
                    return Padding(
                      padding: const EdgeInsets.symmetric(vertical: 3),
                      child: Row(
                        children: [
                          Expanded(
                            child: Text(
                              file.relativePath,
                              overflow: TextOverflow.ellipsis,
                              style: Theme.of(context).textTheme.bodySmall,
                            ),
                          ),
                          const SizedBox(width: 8),
                          Text(
                            formatBytes(file.size),
                            style: Theme.of(context).textTheme.labelSmall,
                          ),
                        ],
                      ),
                    );
                  },
                ),
              ),
              if (widget.saveDestinationLabel != null) ...[
                const SizedBox(height: 12),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
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
                          'Save to: ${widget.saveDestinationLabel}',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                      ),
                      if (widget.onChooseSaveFolder != null)
                        TextButton.icon(
                          onPressed: widget.onChooseSaveFolder,
                          icon: const Icon(Icons.folder_open_rounded, size: 16),
                          label: const Text('Change'),
                          style: TextButton.styleFrom(
                            visualDensity: VisualDensity.compact,
                          ),
                        ),
                    ],
                  ),
                ),
              ],
            ],
            const SizedBox(height: 8),
            CheckboxListTile(
              contentPadding: EdgeInsets.zero,
              controlAffinity: ListTileControlAffinity.leading,
              dense: true,
              value: _trustSender,
              onChanged: (v) => setState(() => _trustSender = v ?? false),
              title: Text('Always accept from ${offer.senderName}'),
              subtitle: const Text(
                'Future transfers from this device will start automatically',
              ),
            ),
            const SizedBox(height: 12),
            Row(
              children: [
                Expanded(
                  child: OutlinedButton(
                    onPressed: widget.onReject,
                    child: const Text('Decline'),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: FilledButton.icon(
                    onPressed: () => widget.onAccept(_trustSender),
                    icon: const Icon(Icons.verified_user_rounded, size: 18),
                    label: Text(isTextClip ? 'Receive Text' : 'Accept'),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
