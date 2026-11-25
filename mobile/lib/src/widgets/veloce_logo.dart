import 'package:flutter/material.dart';

/// Minimal two-tone Swoop V falcon brand mark rendered natively with CustomPainter.
class VeloceLogoMark extends StatelessWidget {
  const VeloceLogoMark({super.key, this.size = 32});

  final double size;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: size,
      height: size,
      child: CustomPaint(painter: _VeloceSwoopVPainter()),
    );
  }
}

class _VeloceSwoopVPainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final sx = size.width / 512.0;
    final sy = size.height / 512.0;
    canvas.save();
    canvas.scale(sx, sy);

    final rect = RRect.fromRectAndRadius(
      const Rect.fromLTWH(8, 8, 496, 496),
      const Radius.circular(112),
    );
    final bgPaint = Paint()
      ..shader = const LinearGradient(
        begin: Alignment.topLeft,
        end: Alignment.bottomRight,
        colors: [Color(0xFF171C26), Color(0xFF0B0E14)],
      ).createShader(const Rect.fromLTWH(0, 0, 512, 512));
    canvas.drawRRect(rect, bgPaint);

    final borderPaint = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 6
      ..color = const Color(0x26FFFFFF);
    canvas.drawRRect(rect, borderPaint);

    final backWing = Path()
      ..moveTo(244, 382)
      ..cubicTo(296, 322, 350, 236, 412, 126)
      ..cubicTo(396, 222, 350, 310, 278, 378)
      ..cubicTo(264, 390, 250, 390, 244, 382)
      ..close();
    canvas.drawPath(backWing, Paint()..color = const Color(0xFFCBD5E1));

    final frontFalcon = Path()
      ..moveTo(96, 138)
      ..cubicTo(156, 150, 210, 194, 250, 266)
      ..cubicTo(266, 226, 292, 196, 324, 182)
      ..cubicTo(344, 174, 362, 178, 374, 194)
      ..cubicTo(358, 190, 344, 194, 334, 206)
      ..cubicTo(312, 246, 288, 312, 254, 378)
      ..cubicTo(248, 390, 232, 390, 226, 378)
      ..cubicTo(196, 322, 160, 268, 112, 222)
      ..cubicTo(142, 226, 166, 234, 188, 248)
      ..cubicTo(146, 206, 116, 170, 96, 138)
      ..close();
    canvas.drawPath(frontFalcon, Paint()..color = const Color(0xFFF8FAFC));

    canvas.restore();
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}
