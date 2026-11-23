import 'package:flutter/material.dart';

class ConnectionBadge extends StatelessWidget {
  const ConnectionBadge({super.key, required this.connectionType});

  final String connectionType;

  @override
  Widget build(BuildContext context) {
    final (IconData icon, String label, Color color) = switch (connectionType) {
      'LAN' => (Icons.wifi_rounded, 'LAN', const Color(0xFF10B981)),
      'Direct-P2P' => (Icons.public_rounded, 'Direct P2P (UDP)', const Color(0xFF38BDF8)),
      _ => (Icons.cell_tower_rounded, 'Relayed', const Color(0xFFF59E0B)),
    };

    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 4),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.14),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 13, color: color),
          const SizedBox(width: 5),
          Text(
            label,
            style: TextStyle(
              color: color,
              fontSize: 11.5,
              fontWeight: FontWeight.w700,
            ),
          ),
        ],
      ),
    );
  }
}
