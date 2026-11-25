import 'package:flutter/material.dart';

import '../state/veloce_controller.dart';

class SettingsSheet extends StatefulWidget {
  const SettingsSheet({
    super.key,
    required this.settings,
    required this.onSave,
  });

  final VeloceSettings settings;
  final ValueChanged<VeloceSettings> onSave;

  @override
  State<SettingsSheet> createState() => _SettingsSheetState();
}

class _SettingsSheetState extends State<SettingsSheet> {
  late final TextEditingController _nameController;
  late final TextEditingController _relayController;
  late bool _exportToDownloads;
  late ThemeMode _themeMode;

  @override
  void initState() {
    super.initState();
    _nameController = TextEditingController(text: widget.settings.deviceName);
    _relayController = TextEditingController(text: widget.settings.relayHost);
    _exportToDownloads = widget.settings.exportToDownloads;
    _themeMode = widget.settings.themeMode;
  }

  @override
  void dispose() {
    _nameController.dispose();
    _relayController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final inset = MediaQuery.of(context).viewInsets.bottom;
    return SafeArea(
      child: Padding(
        padding: EdgeInsets.fromLTRB(20, 12, 20, 20 + inset),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Settings', style: Theme.of(context).textTheme.titleLarge),
            const SizedBox(height: 16),
            TextField(
              controller: _nameController,
              decoration: const InputDecoration(
                labelText: 'Device Name',
                hintText: 'Advertised via mDNS and handshake',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 14),
            TextField(
              controller: _relayController,
              decoration: const InputDecoration(
                labelText: 'Relay Server (optional host:port)',
                hintText: 'relay.example.com:7878',
                helperText:
                    'Optional: IPv6, UPnP/NAT-PMP, STUN & Tailscale work free out of the box. '
                    'Add a self-hosted relay only for double-CGNAT IPv4-only cellular links.',
                helperMaxLines: 3,
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 10),
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              title: const Text('Save to Downloads / Files'),
              subtitle: const Text('Export verified transfers to shared Downloads folder'),
              value: _exportToDownloads,
              onChanged: (v) => setState(() => _exportToDownloads = v),
            ),
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              title: const Text('Dark Mode'),
              value: _themeMode == ThemeMode.dark,
              onChanged: (dark) =>
                  setState(() => _themeMode = dark ? ThemeMode.dark : ThemeMode.light),
            ),
            const SizedBox(height: 12),
            SizedBox(
              width: double.infinity,
              child: FilledButton(
                onPressed: () {
                  final name = _nameController.text.trim();
                  widget.onSave(
                    VeloceSettings(
                      deviceName: name.isEmpty ? widget.settings.deviceName : name,
                      relayHost: _relayController.text.trim(),
                      exportToDownloads: _exportToDownloads,
                      themeMode: _themeMode,
                    ),
                  );
                  Navigator.of(context).pop();
                },
                child: const Text('Save & Apply'),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
