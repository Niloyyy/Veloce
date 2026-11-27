import { FolderOpen, RotateCcw, ShieldCheck, SlidersHorizontal, Trash2, X } from "lucide-react";
import { useState } from "react";
import type { DesktopSettings, ThemeId, TrustedPeer } from "../types/veloce";
import { THEME_OPTIONS } from "../utils/themes";

interface SettingsModalProps {
  settings: DesktopSettings;
  detectedDeviceName?: string;
  saving: boolean;
  onPreviewTheme: (theme: ThemeId) => void;
  onBrowseDownloadDir: () => Promise<string | null>;
  onSave: (next: DesktopSettings) => Promise<void>;
  onClose: () => void;
}

export function SettingsModal({
  settings,
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
            <span className="eyebrow">Node &amp; Transfer Preferences</span>
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

          <label className="trust-checkbox-row" htmlFor="veloce-bg-mode">
            <input
              id="veloce-bg-mode"
              type="checkbox"
              checked={runInBackground}
              onChange={(e) => setRunInBackground(e.target.checked)}
              disabled={saving}
            />
            <div>
              <strong>Keep running in background (System Tray)</strong>
              <div className="field-hint">
                Closing the window hides Veloce to the system tray so nearby devices &amp; global
                transfers stay reachable anytime.
              </div>
            </div>
          </label>

          <div className="field-group">
            <div className="field-label-row">
              <span className="input-label">Appearance Theme</span>
              <span className="muted-xs">Live preview enabled</span>
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

          <div className="field-group">
            <label className="input-label" htmlFor="veloce-relay-addrs">
              Relay Addresses (optional host:port)
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
              Native IPv6, UPnP/NAT-PMP, STUN, and Tailscale/ZeroTier work free with no relay; add
              a self-hosted veloce-relay only when both peers are on IPv4-only symmetric CGNAT.
            </span>
          </div>

          <div className="field-group">
            <div className="range-header">
              <label className="input-label" htmlFor="veloce-parallel-streams">
                Parallel Streams
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
              Recommended: 8 for LAN/WAN, 4 for low-power links, 16–32 for high-latency fiber.
            </span>
          </div>

          <div className="field-group">
            <span className="input-label">Trusted Devices (Auto-Accept)</span>
            {trustedPeers.length === 0 ? (
              <span className="field-hint">
                No trusted devices yet. Click the shield icon on a nearby device or check
                &ldquo;Trust&rdquo; on an incoming offer to auto-accept transfers silently.
              </span>
            ) : (
              <ul className="trusted-list">
                {trustedPeers.map((peer) => (
                  <li key={peer.nodeId} className="trusted-list-row">
                    <div className="trusted-list-info">
                      <ShieldCheck size={14} className="trusted-icon" />
                      <strong>{peer.deviceName}</strong>
                      <code>{peer.shortId}</code>
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
              {saving ? "Applying..." : "Save & Apply"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
