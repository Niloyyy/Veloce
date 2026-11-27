import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../state/veloce_controller.dart';
import '../utils/formatters.dart';
import '../utils/text_clip.dart';
import 'connection_badge.dart';

class TransferCard extends StatelessWidget {
  const TransferCard({
    super.key,
    required this.item,
    required this.onPause,
    required this.onResume,
    required this.onCancel,
    required this.onRemove,
    this.onRetry,
    this.onOpenFile,
    this.onOpenFolder,
    this.onOpenUrl,
  });

  final TransferItem item;
  final VoidCallback onPause;
  final VoidCallback onResume;
  final VoidCallback onCancel;
  final VoidCallback onRemove;
  final VoidCallback? onRetry;
  final VoidCallback? onOpenFile;
  final VoidCallback? onOpenFolder;
  final ValueChanged<String>? onOpenUrl;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final isSend = item.direction == 'send';
    final textClip = item.textContent;
    final hasTextClip = textClip != null && textClip.isNotEmpty;
    final detectedUrl = hasTextClip ? extractFirstHttpUrl(textClip) : null;
    final showSavedFileActions = !hasTextClip &&
        item.outputPaths.isNotEmpty &&
        item.status == TransferStatus.completed &&
        item.direction == 'receive';

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
                    : item.status == TransferStatus.failed
                        ? scheme.error
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
                            : item.status == TransferStatus.cancelled
                                ? 'Cancelled'
                                : formatSpeed(item.speedBytesPerSec),
                        style: Theme.of(context)
                            .textTheme
                            .labelMedium
                            ?.copyWith(fontWeight: FontWeight.w700),
                      ),
                      if (item.status == TransferStatus.completed)
                        Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            const Icon(Icons.timer_outlined, size: 13),
                            const SizedBox(width: 4),
                            Text(
                              item.elapsedMs > 0
                                  ? 'Took ${formatDurationMs(item.elapsedMs)}'
                                  : 'Done',
                              style: Theme.of(context).textTheme.labelSmall,
                            ),
                          ],
                        )
                      else ...[
                        if (item.elapsedMs > 0)
                          Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              const Icon(Icons.timer_outlined, size: 13),
                              const SizedBox(width: 4),
                              Text(
                                formatDurationMs(item.elapsedMs),
                                style: Theme.of(context).textTheme.labelSmall,
                              ),
                            ],
                          ),
                        Text(
                          'ETA: ${formatEta(item.etaSeconds)}',
                          style: Theme.of(context).textTheme.labelSmall,
                        ),
                      ],
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
                            '${item.chunksVerified}/${item.totalChunks} chunks'
                            '${item.initialChunksVerified > 0 ? ' (${item.initialChunksVerified} resumed)' : ''}',
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
                ] else ...[
                  if (item.status == TransferStatus.completed)
                    const Padding(
                      padding: EdgeInsets.only(right: 4),
                      child: Icon(
                        Icons.check_circle_rounded,
                        color: Color(0xFF10B981),
                        size: 20,
                      ),
                    )
                  else
                    Padding(
                      padding: const EdgeInsets.only(right: 4),
                      child: Icon(
                        Icons.error_outline_rounded,
                        color: scheme.error,
                        size: 20,
                      ),
                    ),
                  if (item.status == TransferStatus.failed &&
                      item.direction == 'receive' &&
                      onRetry != null)
                    IconButton(
                      tooltip: 'Retry transfer',
                      icon: const Icon(Icons.refresh_rounded, size: 20),
                      onPressed: onRetry,
                    ),
                  IconButton(
                    tooltip: 'Remove from history',
                    icon: const Icon(Icons.delete_outline_rounded, size: 20),
                    onPressed: onRemove,
                  ),
                ],
              ],
            ),
            if (hasTextClip) ...[
              const SizedBox(height: 8),
              Container(
                width: double.infinity,
                padding: const EdgeInsets.all(10),
                decoration: BoxDecoration(
                  color: scheme.surfaceContainerHighest,
                  borderRadius: BorderRadius.circular(10),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    SelectableText(
                      textClip,
                      maxLines: 6,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                            fontFamily: 'monospace',
                          ),
                    ),
                    const SizedBox(height: 8),
                    Wrap(
                      spacing: 8,
                      runSpacing: 6,
                      children: [
                        OutlinedButton.icon(
                          onPressed: () async {
                            await Clipboard.setData(ClipboardData(text: textClip));
                            if (context.mounted) {
                              ScaffoldMessenger.of(context).showSnackBar(
                                const SnackBar(content: Text('Copied to clipboard')),
                              );
                            }
                          },
                          icon: const Icon(Icons.copy_rounded, size: 15),
                          label: const Text('Copy Text'),
                          style: OutlinedButton.styleFrom(
                            visualDensity: VisualDensity.compact,
                          ),
                        ),
                        if (detectedUrl != null && onOpenUrl != null)
                          FilledButton.tonalIcon(
                            onPressed: () => onOpenUrl!(detectedUrl),
                            icon: const Icon(Icons.open_in_new_rounded, size: 15),
                            label: const Text('Open Link'),
                            style: FilledButton.styleFrom(
                              visualDensity: VisualDensity.compact,
                            ),
                          ),
                      ],
                    ),
                  ],
                ),
              ),
            ],
            if (showSavedFileActions) ...[
              const SizedBox(height: 8),
              Text(
                'Saved: ${item.outputPaths.first}',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const SizedBox(height: 6),
              Wrap(
                spacing: 8,
                runSpacing: 6,
                children: [
                  if (onOpenFile != null)
                    FilledButton.tonalIcon(
                      onPressed: onOpenFile,
                      icon: const Icon(Icons.open_in_new_rounded, size: 15),
                      label: const Text('Open'),
                      style: FilledButton.styleFrom(
                        visualDensity: VisualDensity.compact,
                      ),
                    ),
                  if (onOpenFolder != null)
                    OutlinedButton.icon(
                      onPressed: onOpenFolder,
                      icon: const Icon(Icons.folder_open_rounded, size: 15),
                      label: const Text('Folder'),
                      style: OutlinedButton.styleFrom(
                        visualDensity: VisualDensity.compact,
                      ),
                    ),
                ],
              ),
            ] else if (!hasTextClip &&
                item.outputPaths.isNotEmpty &&
                item.status == TransferStatus.completed) ...[
              const SizedBox(height: 6),
              Text(
                'Saved: ${item.outputPaths.first}',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ],
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
