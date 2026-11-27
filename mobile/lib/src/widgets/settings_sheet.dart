import 'package:flutter/material.dart';

import '../services/platform_service.dart';
import '../state/veloce_controller.dart';
import '../theme/veloce_themes.dart';

class SettingsSheet extends StatefulWidget {
  const SettingsSheet({
    super.key,
    required this.settings,
    required this.onSave,
    this.onPreviewTheme,
    this.onPickSaveFolder,
    this.detectedDeviceName = '',
    this.trustedPeers = const [],
    this.onUntrustPeer,
  });

  final VeloceSettings settings;
  final ValueChanged<VeloceSettings> onSave;
  final ValueChanged<VeloceThemeId>? onPreviewTheme;
  final Future<PickedSaveFolder?> Function()? onPickSaveFolder;
  final String detectedDeviceName;
  final List<TrustedPeer> trustedPeers;
  final ValueChanged<String>? onUntrustPeer;

  @override
  State<SettingsSheet> createState() => _SettingsSheetState();
}

class _SettingsSheetState extends State<SettingsSheet> {
  late final TextEditingController _nameController;
  late final TextEditingController _relayController;
  late bool _exportToDownloads;
  late VeloceThemeId _themeId;
  late String _customSaveUri;
  late String _customSaveLabel;
  bool _pickingFolder = false;

  @override
  void initState() {
    super.initState();
    _nameController = TextEditingController(text: widget.settings.deviceName);
    _relayController = TextEditingController(text: widget.settings.relayHost);
    _exportToDownloads = widget.settings.exportToDownloads;
    _themeId = widget.settings.effectiveThemeId;
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
              maxLength: 64,
              onChanged: (_) => setState(() {}),
              decoration: InputDecoration(
                labelText: 'Device Name',
                hintText: widget.detectedDeviceName.isNotEmpty
                    ? widget.detectedDeviceName
                    : 'Shown to nearby devices',
                helperText: 'Shown to nearby phones and computers',
                border: const OutlineInputBorder(),
                counterText: '',
                suffixIcon: widget.detectedDeviceName.isNotEmpty &&
                        _nameController.text.trim() != widget.detectedDeviceName
                    ? IconButton(
                        tooltip: 'Reset to ${widget.detectedDeviceName}',
                        icon: const Icon(Icons.restart_alt_rounded, size: 20),
                        onPressed: () {
                          setState(() {
                            _nameController.text = widget.detectedDeviceName;
                          });
                        },
                      )
                    : null,
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
                    ? 'Using your custom folder selected above'
                    : 'Automatically save received files to Downloads',
              ),
              value: _exportToDownloads,
              onChanged: (v) => setState(() => _exportToDownloads = v),
            ),
            const SizedBox(height: 8),
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                Text('Theme', style: Theme.of(context).textTheme.titleSmall),
                Text(
                  'Tap to preview live',
                  style: Theme.of(context).textTheme.labelSmall,
                ),
              ],
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: VeloceThemeId.values.map((theme) {
                final selected = _themeId == theme;
                return ChoiceChip(
                  selected: selected,
                  showCheckmark: false,
                  avatar: Container(
                    width: 18,
                    height: 18,
                    decoration: BoxDecoration(
                      color: theme.scaffoldColor,
                      shape: BoxShape.circle,
                      border: Border.all(color: theme.accentColor, width: 2.5),
                    ),
                    child: Center(
                      child: Container(
                        width: 6,
                        height: 6,
                        decoration: BoxDecoration(
                          color: theme.accentColor,
                          shape: BoxShape.circle,
                        ),
                      ),
                    ),
                  ),
                  label: Text(theme.label),
                  onSelected: (_) {
                    setState(() => _themeId = theme);
                    widget.onPreviewTheme?.call(theme);
                  },
                );
              }).toList(growable: false),
            ),
            const SizedBox(height: 16),
            Text(
              'Trusted Devices',
              style: Theme.of(context).textTheme.titleSmall,
            ),
            const SizedBox(height: 6),
            if (widget.trustedPeers.isEmpty)
              Text(
                'None yet. Tap the shield icon on a nearby device to accept its files automatically.',
                style: Theme.of(context).textTheme.bodySmall,
              )
            else
              Column(
                children: widget.trustedPeers.map((peer) {
                  return Container(
                    margin: const EdgeInsets.only(bottom: 6),
                    padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                    decoration: BoxDecoration(
                      color: Theme.of(context).colorScheme.surfaceContainerHighest,
                      borderRadius: BorderRadius.circular(10),
                    ),
                    child: Row(
                      children: [
                        const Icon(
                          Icons.verified_user_rounded,
                          size: 16,
                          color: Color(0xFF10B981),
                        ),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            peer.deviceName,
                            overflow: TextOverflow.ellipsis,
                            style: Theme.of(context).textTheme.bodySmall,
                          ),
                        ),
                        if (widget.onUntrustPeer != null)
                          IconButton(
                            tooltip: 'Remove ${peer.deviceName}',
                            icon: const Icon(Icons.delete_outline_rounded, size: 18),
                            visualDensity: VisualDensity.compact,
                            onPressed: () => widget.onUntrustPeer!(peer.nodeId),
                          ),
                      ],
                    ),
                  );
                }).toList(growable: false),
              ),
            const SizedBox(height: 8),
            Theme(
              data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
              child: ExpansionTile(
                tilePadding: EdgeInsets.zero,
                childrenPadding: const EdgeInsets.only(bottom: 8),
                initiallyExpanded: _relayController.text.trim().isNotEmpty,
                title: Text(
                  'Advanced Network Options',
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        fontWeight: FontWeight.w600,
                      ),
                ),
                children: [
                  TextField(
                    controller: _relayController,
                    decoration: const InputDecoration(
                      labelText: 'Relay Server (optional)',
                      hintText: 'relay.example.com:7878',
                      helperText:
                          'Only needed if two devices on strict mobile networks cannot connect directly.',
                      helperMaxLines: 2,
                      border: OutlineInputBorder(),
                    ),
                  ),
                ],
              ),
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
                      themeId: _themeId,
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
