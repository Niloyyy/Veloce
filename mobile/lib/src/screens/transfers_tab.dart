import 'package:flutter/material.dart';

import '../state/veloce_controller.dart';
import '../widgets/transfer_card.dart';

class TransfersTab extends StatelessWidget {
  const TransfersTab({super.key, required this.controller});

  final VeloceController controller;

  @override
  Widget build(BuildContext context) {
    final items = controller.transfers;
    if (items.isEmpty) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(
                Icons.swap_vert_rounded,
                size: 48,
                color: Theme.of(context).colorScheme.outline,
              ),
              const SizedBox(height: 12),
              Text(
                'No Active Transfers',
                style: Theme.of(context).textTheme.titleMedium,
              ),
              const SizedBox(height: 6),
              Text(
                'Live MB/s speed, ETA, BLAKE3 chunk verification, and connection badges '
                'will stream here during transfers.',
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ],
          ),
        ),
      );
    }

    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 10, 16, 0),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                '${items.length} transfer(s)',
                style: Theme.of(context).textTheme.labelMedium,
              ),
              if (controller.hasCompletedTransfers)
                TextButton.icon(
                  onPressed: controller.clearCompletedTransfers,
                  icon: const Icon(Icons.delete_sweep_outlined, size: 18),
                  label: const Text('Clear History'),
                ),
            ],
          ),
        ),
        Expanded(
          child: ListView.builder(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
            itemCount: items.length,
            itemBuilder: (context, idx) {
              final item = items[idx];
              final card = TransferCard(
                item: item,
                onPause: () => controller.pauseTransfer(item.transferId),
                onResume: () => controller.resumeTransfer(item.transferId),
                onCancel: () => controller.cancelTransfer(item.transferId),
                onRemove: () => controller.removeTransfer(item.transferId),
                onRetry: () => controller.resumeTransfer(item.transferId),
              );
              if (!item.isTerminal) {
                return card;
              }
              return Dismissible(
                key: ValueKey('transfer-${item.transferId}'),
                direction: DismissDirection.horizontal,
                onDismissed: (_) => controller.removeTransfer(item.transferId),
                background: Container(
                  margin: const EdgeInsets.only(bottom: 12),
                  padding: const EdgeInsets.symmetric(horizontal: 20),
                  alignment: Alignment.centerRight,
                  decoration: BoxDecoration(
                    color: Theme.of(context).colorScheme.errorContainer,
                    borderRadius: BorderRadius.circular(12),
                  ),
                  child: Icon(
                    Icons.delete_outline_rounded,
                    color: Theme.of(context).colorScheme.onErrorContainer,
                  ),
                ),
                child: card,
              );
            },
          ),
        ),
      ],
    );
  }
}
