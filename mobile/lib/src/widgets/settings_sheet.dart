import 'package:flutter/material.dart';

import '../services/platform_service.dart';
import '../state/veloce_controller.dart';

class SettingsSheet extends StatefulWidget {
  const SettingsSheet({
    super.key,
    required this.settings,
    required this.onSave,
    this.onPickSaveFolder,
  });

  final VeloceSettings settings;
  final ValueChanged<VeloceSettings> onSave;
  final Future<PickedSaveFolder?> Function()? onPickSaveFolder;

  @override
  State<SettingsSheet> createState() => _SettingsSheetState();
}

class _SettingsSheetState extends State<SettingsSheet> {
  late final TextEditingController _nameController;
  late final TextEditingController _relayController;
  late bool _exportToDownloads;
  late ThemeMode _themeMode;
  late String _customSaveUri;
  late String _customSaveLabel;
  bool _pickingFolder = false;

  @override
  void initState() {
    super.initState();
    _nameController = TextEditingController(text: widget.settings.deviceName);
    _relayController = TextEditingController(text: widget.settings.relayHost);
    _exportToDownloads = widget.settings.exportToDownloads;
    _themeMode = widget.settings.themeMode;
    _customSaveUri = widget.settings.customSaveUri;
    _customSaveLabel = widget.settings.customSaveLabel;
  }

  @override
  void dispose() {
    _nameController.dispose();
    _relayController.dispose();
    super.dispose();
  }

  Future<void> _handlePickFolder() async {
    final picker = widget.onPickSaveFolder;
    if (picker == null || _pickingFolder) return;
    setState(() => _pickingFolder = true);
    try {
      final picked = await picker();
      if (picked != null && mounted) {
        setState(() {
          _customSaveUri = picked.uri;
          _customSaveLabel = picked.displayPath;
        });
      }
    } finally {
      if (mounted) {
        setState(() => _pickingFolder = false);
      }
    }
  }

  String get _previewSaveLabel => widget.settings
      .copyWith(
        exportToDownloads: _exportToDownloads,
        customSaveUri: _customSaveUri,
        customSaveLabel: _customSaveLabel,
      )
      .effectiveSaveLabel;

  @override
  Widget build(BuildContext context) {
    final inset = MediaQuery.of(context).viewInsets.bottom;
    final hasCustom = _customSaveUri.trim().isNotEmpty;
    return SafeArea(
      child: SingleChildScrollView(
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
            const SizedBox(height: 14),
            Container(
              width: double.infinity,
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: Theme.of(context).colorScheme.surfaceContainerHighest,
                borderRadius: BorderRadius.circular(12),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      const Icon(Icons.folder_outlined, size: 18),
                      const SizedBox(width: 8),
                      Text(
                        'Save Destination Folder',
                        style: Theme.of(context).textTheme.titleSmall,
                      ),
                    ],
                  ),
                  const SizedBox(height: 6),
                  Text(
                    _previewSaveLabel,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                  const SizedBox(height: 10),
                  Row(
                    children: [
                      Expanded(
                        child: OutlinedButton.icon(
                          onPressed: _pickingFolder || widget.onPickSaveFolder == null
                              ? null
                              : _handlePickFolder,
                          icon: const Icon(Icons.folder_open_rounded, size: 18),
                          label: Text(_pickingFolder ? 'Opening...' : 'Choose Folder'),
                        ),
                      ),
                      if (hasCustom) ...[
                        const SizedBox(width: 8),
                        IconButton.outlined(
                          tooltip: 'Reset to default Downloads folder',
                          onPressed: () {
                            setState(() {
                              _customSaveUri = '';
                              _customSaveLabel = '';
                            });
                          },
                          icon: const Icon(Icons.restart_alt_rounded, size: 19),
                        ),
                      ],
                    ],
                  ),
                ],
              ),
            ),
            const SizedBox(height: 8),
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              title: const Text('Save to Downloads / Files'),
              subtitle: Text(
                hasCustom
                    ? 'Overridden by your custom save folder above'
                    : 'Export verified transfers to shared Downloads folder',
              ),
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
                      customSaveUri: _customSaveUri,
                      customSaveLabel: _customSaveLabel,
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
