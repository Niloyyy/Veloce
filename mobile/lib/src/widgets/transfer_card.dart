import 'package:flutter/material.dart';

import '../state/veloce_controller.dart';
import '../utils/formatters.dart';
import 'connection_badge.dart';

class TransferCard extends StatelessWidget {
  const TransferCard({
    super.key,
    required this.item,
    required this.onPause,
    required this.onResume,
    required this.onCancel,
  });

  final TransferItem item;
  final VoidCallback onPause;
  final VoidCallback onResume;
  final VoidCallback onCancel;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final isSend = item.direction == 'send';

    return Card(
      margin: const EdgeInsets.only(bottom: 12),
      child: Padding(
        padding: const EdgeInsets.all(14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                CircleAvatar(
                  radius: 16,
                  backgroundColor: (isSend ? scheme.primary : const Color(0xFF10B981))
                      .withValues(alpha: 0.15),
                  child: Icon(
                    isSend ? Icons.north_east_rounded : Icons.south_west_rounded,
                    size: 16,
                    color: isSend ? scheme.primary : const Color(0xFF10B981),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        item.label,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context)
                            .textTheme
                            .titleSmall
                            ?.copyWith(fontWeight: FontWeight.w600),
                      ),
                      Text(
                        '${formatBytes(item.bytesTransferred)} / ${formatBytes(item.totalBytes)}'
                        '${item.status == TransferStatus.paused ? ' • Paused' : ''}',
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    ],
                  ),
                ),
                ConnectionBadge(connectionType: item.connectionType),
              ],
            ),
            const SizedBox(height: 12),
            ClipRRect(
              borderRadius: BorderRadius.circular(999),
              child: LinearProgressIndicator(
                value: item.progress,
                minHeight: 6,
                color: item.status == TransferStatus.paused
                    ? const Color(0xFFF59E0B)
                    : null,
              ),
            ),
            const SizedBox(height: 10),
            Row(
              children: [
                Expanded(
                  child: Wrap(
                    spacing: 12,
                    runSpacing: 4,
                    children: [
                      Text(
                        item.status == TransferStatus.paused
                            ? 'Paused'
                            : formatSpeed(item.speedBytesPerSec),
                        style: Theme.of(context)
                            .textTheme
                            .labelMedium
                            ?.copyWith(fontWeight: FontWeight.w700),
                      ),
                      Text(
                        'ETA: ${item.status == TransferStatus.completed ? 'Done' : formatEta(item.etaSeconds)}',
                        style: Theme.of(context).textTheme.labelSmall,
                      ),
                      Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          const Icon(
                            Icons.verified_user_outlined,
                            size: 13,
                            color: Color(0xFF10B981),
                          ),
                          const SizedBox(width: 4),
                          Text(
                            '${item.chunksVerified}/${item.totalChunks} chunks',
                            style: Theme.of(context)
                                .textTheme
                                .labelSmall
                                ?.copyWith(color: const Color(0xFF10B981)),
                          ),
                        ],
                      ),
                    ],
                  ),
                ),
                if (item.status == TransferStatus.active) ...[
                  IconButton(
                    tooltip: 'Pause transfer',
                    icon: const Icon(Icons.pause_rounded, size: 20),
                    onPressed: onPause,
                  ),
                  IconButton(
                    tooltip: 'Cancel transfer',
                    icon: const Icon(Icons.close_rounded, size: 20),
                    onPressed: onCancel,
                  ),
                ] else if (item.status == TransferStatus.paused) ...[
                  IconButton(
                    tooltip: 'Resume transfer',
                    icon: const Icon(
                      Icons.play_arrow_rounded,
                      size: 22,
                      color: Color(0xFF10B981),
                    ),
                    onPressed: onResume,
                  ),
                  IconButton(
                    tooltip: 'Cancel transfer',
                    icon: const Icon(Icons.close_rounded, size: 20),
                    onPressed: onCancel,
                  ),
                ] else if (item.status == TransferStatus.completed)
                  const Icon(Icons.check_circle_rounded, color: Color(0xFF10B981))
                else
                  Icon(Icons.error_outline_rounded, color: scheme.error),
              ],
            ),
            if (item.error != null) ...[
              const SizedBox(height: 6),
              Text(
                item.error!,
                style: Theme.of(context)
                    .textTheme
                    .bodySmall
                    ?.copyWith(color: scheme.error),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
