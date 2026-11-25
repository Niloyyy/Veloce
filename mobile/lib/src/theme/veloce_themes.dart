import 'package:flutter/material.dart';

/// Curated Veloce UI themes shared across Mobile and Desktop.
enum VeloceThemeId {
  obsidian(
    label: 'Obsidian',
    brightness: Brightness.dark,
    seedColor: Color(0xFF3B82F6),
    scaffoldColor: Color(0xFF0C0F14),
    surfaceColor: Color(0xFF141922),
    accentColor: Color(0xFF3B82F6),
  ),
  daylight(
    label: 'Daylight',
    brightness: Brightness.light,
    seedColor: Color(0xFF2563EB),
    scaffoldColor: Color(0xFFF4F6FA),
    surfaceColor: Color(0xFFFFFFFF),
    accentColor: Color(0xFF2563EB),
  ),
  midnight(
    label: 'Midnight OLED',
    brightness: Brightness.dark,
    seedColor: Color(0xFF38BDF8),
    scaffoldColor: Color(0xFF000000),
    surfaceColor: Color(0xFF0B0E14),
    accentColor: Color(0xFF38BDF8),
  ),
  nord(
    label: 'Nord Frost',
    brightness: Brightness.dark,
    seedColor: Color(0xFF5EEAD4),
    scaffoldColor: Color(0xFF1B222C),
    surfaceColor: Color(0xFF242E3B),
    accentColor: Color(0xFF5EEAD4),
  ),
  emerald(
    label: 'Emerald',
    brightness: Brightness.dark,
    seedColor: Color(0xFF10B981),
    scaffoldColor: Color(0xFF09130F),
    surfaceColor: Color(0xFF102019),
    accentColor: Color(0xFF10B981),
  ),
  sunset(
    label: 'Sunset Ember',
    brightness: Brightness.dark,
    seedColor: Color(0xFFF97316),
    scaffoldColor: Color(0xFF140E12),
    surfaceColor: Color(0xFF1F161C),
    accentColor: Color(0xFFF97316),
  );

  const VeloceThemeId({
    required this.label,
    required this.brightness,
    required this.seedColor,
    required this.scaffoldColor,
    required this.surfaceColor,
    required this.accentColor,
  });

  final String label;
  final Brightness brightness;
  final Color seedColor;
  final Color scaffoldColor;
  final Color surfaceColor;
  final Color accentColor;

  bool get isDark => brightness == Brightness.dark;

  VeloceThemeId get next {
    final all = VeloceThemeId.values;
    return all[(index + 1) % all.length];
  }

  static VeloceThemeId parse(String? raw, {bool fallbackDark = true}) {
    final normalized = raw?.trim().toLowerCase() ?? '';
    if (normalized == 'dark' || normalized == 'obsidian') {
      return VeloceThemeId.obsidian;
    }
    if (normalized == 'light' || normalized == 'daylight') {
      return VeloceThemeId.daylight;
    }
    for (final id in VeloceThemeId.values) {
      if (id.name == normalized) return id;
    }
    return fallbackDark ? VeloceThemeId.obsidian : VeloceThemeId.daylight;
  }
}

ThemeData buildVeloceTheme(VeloceThemeId id) {
  final scheme = ColorScheme.fromSeed(
    seedColor: id.seedColor,
    brightness: id.brightness,
    surface: id.surfaceColor,
  );
  return ThemeData(
    useMaterial3: true,
    colorScheme: scheme,
    scaffoldBackgroundColor: id.scaffoldColor,
    appBarTheme: AppBarTheme(
      backgroundColor: id.scaffoldColor,
      surfaceTintColor: Colors.transparent,
    ),
    cardTheme: CardThemeData(
      color: id.surfaceColor,
    ),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: id.surfaceColor,
    ),
  );
}
