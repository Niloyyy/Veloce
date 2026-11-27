import {
  ChevronDown,
  FolderOpen,
  RotateCcw,
  ShieldCheck,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import { useState } from "react";
import type { DesktopSettings, ThemeId, TrustedPeer } from "../types/veloce";
import { THEME_OPTIONS } from "../utils/themes";

interface SettingsModalProps {
  settings: DesktopSettings;
  nodeId?: string;
  detectedDeviceName?: string;
  saving: boolean;
  onPreviewTheme: (theme: ThemeId) => void;
  onBrowseDownloadDir: () => Promise<string | null>;
  onSave: (next: DesktopSettings) => Promise<void>;
  onClose: () => void;
}

export function SettingsModal({
  settings,
  nodeId,
  detectedDeviceName,
  saving,
  onPreviewTheme,
  onBrowseDownloadDir,
  onSave,
  onClose,
}: SettingsModalProps) {
  const [deviceName, setDeviceName] = useState(settings.deviceName);
  const [downloadDir, setDownloadDir] = useState(settings.downloadDir);
  const [relayText, setRelayText] = useState(settings.relayAddrs.join(", "));
  const [parallelStreams, setParallelStreams] = useState(settings.parallelStreams);
  const [selectedTheme, setSelectedTheme] = useState<ThemeId>(settings.theme);
  const [runInBackground, setRunInBackground] = useState<boolean>(
    settings.runInBackground ?? true,
  );
  const [trustedPeers, setTrustedPeers] = useState<TrustedPeer[]>(settings.trustedPeers ?? []);
  const [advancedOpen, setAdvancedOpen] = useState(settings.relayAddrs.length > 0);
  const [error, setError] = useState<string | null>(null);

  const handleBrowse = async () => {
    const picked = await onBrowseDownloadDir();
    if (picked) setDownloadDir(picked);
  };

  const handleSelectTheme = (nextTheme: ThemeId) => {
    setSelectedTheme(nextTheme);
    onPreviewTheme(nextTheme);
  };

  const handleCancel = () => {
    onPreviewTheme(settings.theme);
    onClose();
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const trimmedName = deviceName.trim();
    if (!trimmedName) {
      setError("Device name cannot be empty.");
      return;
    }
    const relayAddrs = relayText
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);

    setError(null);
    try {
      await onSave({
        deviceName: trimmedName,
        downloadDir: downloadDir.trim() || settings.downloadDir,
        relayAddrs,
        parallelStreams,
        theme: selectedTheme,
        runInBackground,
        trustedPeers,
      });
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div
      className="modal-backdrop"
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-modal-title"
    >
      <div className="modal-card modal-card-wide">
        <div className="modal-header">
          <div className="modal-title-stack">
            <h2 id="settings-modal-title" className="settings-title-row">
              <SlidersHorizontal size={18} />
              <span>Settings</span>
            </h2>
          </div>
          <button
            className="icon-btn"
            type="button"
            aria-label="Close settings"
            onClick={handleCancel}
            disabled={saving}
          >
            <X size={16} />
          </button>
        </div>

        <form className="settings-form" onSubmit={handleSubmit}>
          <div className="field-group">
            <div className="field-label-row">
              <label className="input-label" htmlFor="veloce-device-name">
                Device Name
              </label>
              {detectedDeviceName && deviceName.trim() !== detectedDeviceName && (
                <button
                  type="button"
                  className="btn-link-reset"
                  onClick={() => setDeviceName(detectedDeviceName)}
                  disabled={saving}
                  title={`Reset to ${detectedDeviceName}`}
                >
                  <RotateCcw size={12} />
                  <span>Reset</span>
                </button>
              )}
            </div>
            <input
              id="veloce-device-name"
              className="text-input"
              type="text"
              maxLength={64}
              value={deviceName}
              onChange={(e) => setDeviceName(e.target.value)}
              placeholder={detectedDeviceName || "My Computer"}
            />
            <span className="field-hint">Shown to nearby phones and computers.</span>
          </div>

          <div className="field-group">
            <label className="input-label" htmlFor="veloce-download-dir">
              Save Folder
            </label>
            <div className="dir-picker-row dir-picker-compact">
              <span id="veloce-download-dir" className="muted-xs truncate" title={downloadDir}>
                {downloadDir}
              </span>
              <button
                className="btn-secondary btn-sm"
                type="button"
                onClick={handleBrowse}
                disabled={saving}
              >
                <FolderOpen size={14} />
                <span>Browse</span>
              </button>
            </div>
          </div>

          <div className="field-group">
            <div className="field-label-row">
              <span className="input-label">Theme</span>
              <span className="muted-xs">Click to preview live</span>
            </div>
            <div className="theme-picker-grid">
              {THEME_OPTIONS.map((opt) => (
                <button
                  key={opt.id}
                  type="button"
                  className={`theme-swatch-btn ${selectedTheme === opt.id ? "active" : ""}`}
                  onClick={() => handleSelectTheme(opt.id)}
                >
                  <span
                    className="theme-swatch-dot"
                    style={{
                      backgroundColor: opt.bg,
                      borderColor: opt.accent,
                    }}
                  >
                    <span style={{ backgroundColor: opt.accent }} />
                  </span>
                  <span>{opt.label}</span>
                </button>
              ))}
            </div>
          </div>

          <label className="trust-checkbox-row" htmlFor="veloce-bg-mode">
            <input
              id="veloce-bg-mode"
              type="checkbox"
              checked={runInBackground}
              onChange={(e) => setRunInBackground(e.target.checked)}
              disabled={saving}
            />
            <div>
              <strong>Keep running in background</strong>
              <div className="field-hint">
                Stay in the system tray when closed so you can receive files anytime.
              </div>
            </div>
          </label>

          <div className="field-group">
            <span className="input-label">Trusted Devices</span>
            {trustedPeers.length === 0 ? (
              <span className="field-hint">
                None yet. Click the shield icon on a nearby device to accept its files
                automatically without a prompt.
              </span>
            ) : (
              <ul className="trusted-list">
                {trustedPeers.map((peer) => (
                  <li key={peer.nodeId} className="trusted-list-row">
                    <div className="trusted-list-info">
                      <ShieldCheck size={14} className="trusted-icon" />
                      <strong>{peer.deviceName}</strong>
                    </div>
                    <button
                      type="button"
                      className="icon-btn"
                      title={`Remove ${peer.deviceName}`}
                      onClick={() =>
                        setTrustedPeers((prev) =>
                          prev.filter((p) => p.nodeId !== peer.nodeId),
                        )
                      }
                    >
                      <Trash2 size={13} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>

          <div className="advanced-section">
            <button
              type="button"
              className="advanced-toggle-btn"
              onClick={() => setAdvancedOpen((v) => !v)}
              aria-expanded={advancedOpen}
            >
              <span>Advanced Network Options</span>
              <ChevronDown
                size={15}
                className={`chevron-icon ${advancedOpen ? "chevron-open" : ""}`}
              />
            </button>

            {advancedOpen && (
              <div className="advanced-body">
                <div className="field-group">
                  <label className="input-label" htmlFor="veloce-relay-addrs">
                    Relay Server (optional)
                  </label>
                  <input
                    id="veloce-relay-addrs"
                    className="text-input text-input-mono"
                    type="text"
                    value={relayText}
                    onChange={(e) => setRelayText(e.target.value)}
                    placeholder="relay.example.com:7878"
                  />
                  <span className="field-hint">
                    Only needed if two devices on strict mobile networks cannot connect directly.
                  </span>
                </div>

                <div className="field-group">
                  <div className="range-header">
                    <label className="input-label" htmlFor="veloce-parallel-streams">
                      Parallel Connections
                    </label>
                    <span className="range-value">{parallelStreams}</span>
                  </div>
                  <input
                    id="veloce-parallel-streams"
                    className="range-input"
                    type="range"
                    min={1}
                    max={32}
                    step={1}
                    value={parallelStreams}
                    onChange={(e) => setParallelStreams(Number(e.target.value))}
                  />
                </div>

                {nodeId && (
                  <div className="field-hint">
                    Device Key ID: <code>{nodeId.slice(0, 16)}…</code>
                  </div>
                )}
              </div>
            )}
          </div>

          {error && <div className="inline-error">{error}</div>}

          <div className="modal-actions">
            <button
              className="btn-secondary"
              type="button"
              onClick={handleCancel}
              disabled={saving}
            >
              Cancel
            </button>
            <button className="btn-primary" type="submit" disabled={saving}>
              {saving ? "Saving..." : "Save & Apply"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
