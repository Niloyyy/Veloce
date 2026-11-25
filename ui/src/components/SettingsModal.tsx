import { FolderOpen, RotateCcw, SlidersHorizontal, X } from "lucide-react";
import { useState } from "react";
import type { DesktopSettings, ThemeId } from "../types/veloce";
import { THEME_OPTIONS } from "../utils/themes";

interface SettingsModalProps {
  settings: DesktopSettings;
  detectedDeviceName?: string;
  saving: boolean;
  onBrowseDownloadDir: () => Promise<string | null>;
  onSave: (next: DesktopSettings) => Promise<void>;
  onClose: () => void;
}

export function SettingsModal({
  settings,
  detectedDeviceName,
  saving,
  onBrowseDownloadDir,
  onSave,
  onClose,
}: SettingsModalProps) {
  const [deviceName, setDeviceName] = useState(settings.deviceName);
  const [downloadDir, setDownloadDir] = useState(settings.downloadDir);
  const [relayText, setRelayText] = useState(settings.relayAddrs.join(", "));
  const [parallelStreams, setParallelStreams] = useState(settings.parallelStreams);
  const [selectedTheme, setSelectedTheme] = useState<ThemeId>(settings.theme);
  const [error, setError] = useState<string | null>(null);

  const handleBrowse = async () => {
    const picked = await onBrowseDownloadDir();
    if (picked) setDownloadDir(picked);
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
          <div>
            <span className="eyebrow">Node & Transfer Preferences</span>
            <h2 id="settings-modal-title" className="settings-title-row">
              <SlidersHorizontal size={18} />
              <span>Settings</span>
            </h2>
          </div>
          <button
            className="icon-btn"
            type="button"
            aria-label="Close settings"
            onClick={onClose}
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
                  title={`Reset to detected model (${detectedDeviceName})`}
                >
                  <RotateCcw size={12} />
                  <span>Reset to model</span>
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
              placeholder={detectedDeviceName || "Advertised over mDNS and TLS handshake"}
            />
            {detectedDeviceName && (
              <span className="field-hint">
                Shown to nearby peers &amp; transfer offers • Detected model: {detectedDeviceName}
              </span>
            )}
          </div>

          <div className="field-group">
            <label className="input-label" htmlFor="veloce-download-dir">
              Default Download Directory
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
            <label className="input-label" htmlFor="veloce-relay-addrs">
              Relay Addresses (host:port)
            </label>
            <input
              id="veloce-relay-addrs"
              className="text-input text-input-mono"
              type="text"
              value={relayText}
              onChange={(e) => setRelayText(e.target.value)}
              placeholder="relay.example.com:7878, 203.0.113.10:7878"
            />
            <span className="field-hint">
              Optional comma-separated host:port hints. Native IPv6, UPnP/NAT-PMP, STUN, and
              Tailscale/ZeroTier work free with no relay; add a self-hosted veloce-relay only when
              both peers are on IPv4-only symmetric CGNAT.
            </span>
          </div>

          <div className="field-group">
            <div className="range-header">
              <label className="input-label" htmlFor="veloce-parallel-streams">
                Parallel QUIC Streams
              </label>
              <span className="range-value">{parallelStreams} streams</span>
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
            <span className="field-hint">
              Recommended: 8 for desktop LAN/WAN, 4 for low-power links, 16–32 for high-BDP fiber.
            </span>
          </div>

          <div className="field-group">
            <span className="input-label">Appearance Theme</span>
            <div className="theme-picker-grid">
              {THEME_OPTIONS.map((opt) => (
                <button
                  key={opt.id}
                  type="button"
                  className={`theme-swatch-btn ${selectedTheme === opt.id ? "active" : ""}`}
                  onClick={() => setSelectedTheme(opt.id)}
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

          {error && <div className="inline-error">{error}</div>}

          <div className="modal-actions">
            <button
              className="btn-secondary"
              type="button"
              onClick={onClose}
              disabled={saving}
            >
              Cancel
            </button>
            <button className="btn-primary" type="submit" disabled={saving}>
              {saving ? "Applying..." : "Save & Apply"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
