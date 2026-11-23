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

    return ListView.builder(
      padding: const EdgeInsets.all(16),
      itemCount: items.length,
      itemBuilder: (context, idx) {
        final item = items[idx];
        return TransferCard(
          item: item,
          onPause: () => controller.pauseTransfer(item.transferId),
          onResume: () => controller.resumeTransfer(item.transferId),
          onCancel: () => controller.cancelTransfer(item.transferId),
        );
      },
    );
  }
}
