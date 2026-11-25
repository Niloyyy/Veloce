import {
  ClipboardPaste,
  FileUp,
  FolderPlus,
  Laptop,
  MessageSquareText,
  QrCode,
  Send,
  Shield,
  ShieldCheck,
  UploadCloud,
} from "lucide-react";
import { useState } from "react";
import type { PeerDto, StartSessionResult } from "../types/veloce";
import { QrCodeCard } from "./QrCodeCard";

interface SendViewProps {
  peers: PeerDto[];
  trustedNodeIds: ReadonlySet<string>;
  activeSession: StartSessionResult | null;
  hasRelay: boolean;
  busy: boolean;
  onStartSession: (paths: string[], targetPeerNodeId?: string) => Promise<void>;
  onSendText: (text: string, targetPeerNodeId?: string) => Promise<void>;
  onToggleTrustPeer: (peer: PeerDto) => void;
  onStopSharing: () => void;
}

export function SendView({
  peers,
  trustedNodeIds,
  activeSession,
  hasRelay,
  busy,
  onStartSession,
  onSendText,
  onToggleTrustPeer,
  onStopSharing,
}: SendViewProps) {
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
  const [quickText, setQuickText] = useState("");
  const [dragging, setDragging] = useState(false);

  const handleSelectFiles = async () => {
    const picked = (await window.veloce?.pickFiles()) ?? [];
    if (picked.length > 0) {
      setSelectedPaths(picked);
      await onStartSession(picked);
    }
  };

  const handleSelectFolders = async () => {
    const picked = (await window.veloce?.pickFolders()) ?? [];
    if (picked.length > 0) {
      setSelectedPaths(picked);
      await onStartSession(picked);
    }
  };

  const handleDrop = async (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setDragging(false);
    const files = Array.from(e.dataTransfer.files);
    const paths = files
      .map((f) => window.veloce?.getPathForFile(f) ?? "")
      .filter((p) => p.length > 0);
    if (paths.length > 0) {
      setSelectedPaths(paths);
      await onStartSession(paths);
    }
  };

  const handleSendToLanPeer = async (peer: PeerDto) => {
    let paths = selectedPaths;
    if (paths.length === 0) {
      paths = (await window.veloce?.pickFiles()) ?? [];
      if (paths.length === 0) return;
      setSelectedPaths(paths);
    }
    await onStartSession(paths, peer.nodeId);
  };

  const handlePasteClipboard = async () => {
    try {
      const text = await navigator.clipboard.readText();
      if (text) setQuickText(text);
    } catch {
      // Ignore clipboard permission denials.
    }
  };

  const handleBeamText = async (targetPeerNodeId?: string) => {
    const trimmed = quickText.trim();
    if (!trimmed) return;
    await onSendText(trimmed, targetPeerNodeId);
    if (targetPeerNodeId) {
      setQuickText("");
    }
  };

  const hasQuickText = quickText.trim().length > 0;

  return (
    <div className="view-stack">
      <div
        className={`dropzone ${dragging ? "dropzone-active" : ""}`}
        onDragOver={(e) => {
          e.preventDefault();
          setDragging(true);
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={handleDrop}
      >
        <div className="dropzone-icon">
          <UploadCloud size={28} />
        </div>
        <h2>Drop files or folders to share</h2>
        <p className="muted-sm">
          Instant BLAKE3 Merkle chunking • Multi-stream QUIC (BBR) • Line-speed LAN &amp; Global P2P
        </p>

        <div className="dropzone-buttons">
          <button
            className="btn-primary"
            type="button"
            disabled={busy}
            onClick={handleSelectFiles}
          >
            <FileUp size={16} />
            <span>{busy ? "Hashing..." : "Select Files"}</span>
          </button>
          <button
            className="btn-secondary"
            type="button"
            disabled={busy}
            onClick={handleSelectFolders}
          >
            <FolderPlus size={16} />
            <span>Select Folder</span>
          </button>
        </div>

        {selectedPaths.length > 0 && (
          <div className="selected-pill-row">
            {selectedPaths.slice(0, 3).map((p) => (
              <span className="file-pill" key={p}>
                {p.split(/[\\/]/).pop()}
              </span>
            ))}
            {selectedPaths.length > 3 && (
              <span className="file-pill">+{selectedPaths.length - 3} more</span>
            )}
          </div>
        )}
      </div>

      <section className="panel quick-text-panel">
        <div className="panel-header">
          <h3 className="panel-title-inline">
            <MessageSquareText size={16} />
            <span>Quick Text &amp; Link Beam</span>
          </h3>
          <button
            className="btn-secondary btn-sm"
            type="button"
            disabled={busy}
            onClick={handlePasteClipboard}
            title="Paste from clipboard"
          >
            <ClipboardPaste size={13} />
            <span>Paste</span>
          </button>
        </div>
        <textarea
          className="text-input quick-text-input"
          rows={2}
          value={quickText}
          onChange={(e) => setQuickText(e.target.value)}
          placeholder="Paste a URL, Wi-Fi password, code snippet, or note to beam over E2EE QUIC..."
        />
        <div className="quick-text-actions">
          <span className="muted-xs">
            {hasQuickText
              ? "Click Text on any nearby device below, or create a QR / Ticket."
              : "Works across LAN push and Global QR / Session Tickets."}
          </span>
          <button
            className="btn-secondary btn-sm"
            type="button"
            disabled={busy || !hasQuickText}
            onClick={() => handleBeamText()}
          >
            <QrCode size={13} />
            <span>Share via QR / Ticket</span>
          </button>
        </div>
      </section>

      {activeSession && (
        <QrCodeCard
          session={activeSession}
          hasRelay={hasRelay}
          onStopSharing={onStopSharing}
        />
      )}

      <section className="panel">
        <div className="panel-header">
          <h3>Nearby LAN Devices (mDNS)</h3>
          <span className="muted-xs">
            {peers.length > 0
              ? `${peers.length} device(s) online on local subnet`
              : "Scanning local subnet via _veloce._udp.local..."}
          </span>
        </div>

        {peers.length === 0 ? (
          <div className="empty-state">
            <Laptop size={22} />
            <p className="muted-sm">
              Open Veloce on another device on the same Wi-Fi or Ethernet network to send directly
              at line speed without internet relays.
            </p>
          </div>
        ) : (
          <div className="peers-grid">
            {peers.map((peer) => {
              const isTrusted = trustedNodeIds.has(peer.nodeId);
              return (
                <div key={peer.nodeId} className="peer-card peer-card-row">
                  <div className="peer-avatar">
                    <Laptop size={18} />
                  </div>
                  <div className="peer-info">
                    <div className="peer-name-row">
                      <strong>{peer.deviceName}</strong>
                      {isTrusted && (
                        <span
                          className="trusted-pill"
                          title="Trusted Ed25519 identity — incoming offers auto-accept"
                        >
                          <ShieldCheck size={11} />
                          Trusted
                        </span>
                      )}
                    </div>
                    <span className="muted-xs">
                      {peer.shortId} • {peer.addresses[0] ?? "LAN"}
                    </span>
                  </div>
                  <div className="peer-card-actions">
                    <button
                      type="button"
                      className={`icon-btn ${isTrusted ? "icon-btn-trusted" : ""}`}
                      title={
                        isTrusted
                          ? `Remove ${peer.deviceName} from Trusted Devices`
                          : `Trust ${peer.deviceName} (auto-accept incoming transfers)`
                      }
                      onClick={() => onToggleTrustPeer(peer)}
                    >
                      {isTrusted ? <ShieldCheck size={15} /> : <Shield size={15} />}
                    </button>
                    {hasQuickText && (
                      <button
                        type="button"
                        className="btn-secondary btn-sm"
                        disabled={busy}
                        onClick={() => handleBeamText(peer.nodeId)}
                        title={`Send quick text to ${peer.deviceName}`}
                      >
                        <MessageSquareText size={13} />
                        <span>Text</span>
                      </button>
                    )}
                    <button
                      type="button"
                      className="btn-primary btn-sm"
                      disabled={busy}
                      onClick={() => handleSendToLanPeer(peer)}
                    >
                      <Send size={13} />
                      <span>Send</span>
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </section>
    </div>
  );
}
