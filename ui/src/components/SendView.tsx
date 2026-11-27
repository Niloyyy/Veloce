import {
  ClipboardPaste,
  FileText,
  FileUp,
  FolderPlus,
  Globe,
  Laptop,
  MessageSquareText,
  QrCode,
  Send,
  Shield,
  ShieldCheck,
  Trash2,
  UploadCloud,
  X,
} from "lucide-react";
import { useState } from "react";
import type { PeerDto, StartSessionResult } from "../types/veloce";
import { QrCodeCard } from "./QrCodeCard";

interface SendViewProps {
  peers: PeerDto[];
  selectedPaths: string[];
  trustedNodeIds: ReadonlySet<string>;
  activeSession: StartSessionResult | null;
  hasRelay: boolean;
  busy: boolean;
  onAddPaths: (paths: string[]) => void;
  onRemovePath: (path: string) => void;
  onClearPaths: () => void;
  onStartSession: (paths: string[], targetPeerNodeId?: string) => Promise<void>;
  onSendText: (text: string, targetPeerNodeId?: string) => Promise<void>;
  onToggleTrustPeer: (peer: PeerDto) => void;
  onStopSharing: () => void;
}

function fileBaseName(filePath: string): string {
  const parts = filePath.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? filePath;
}

function parentFolderHint(filePath: string): string {
  const parts = filePath.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 1) return "";
  return parts.slice(-3, -1).join("/");
}

export function SendView({
  peers,
  selectedPaths,
  trustedNodeIds,
  activeSession,
  hasRelay,
  busy,
  onAddPaths,
  onRemovePath,
  onClearPaths,
  onStartSession,
  onSendText,
  onToggleTrustPeer,
  onStopSharing,
}: SendViewProps) {
  const [quickText, setQuickText] = useState("");
  const [dragging, setDragging] = useState(false);

  const handleSelectFiles = async () => {
    const picked = (await window.veloce?.pickFiles()) ?? [];
    if (picked.length > 0) {
      onAddPaths(picked);
    }
  };

  const handleSelectFolders = async () => {
    const picked = (await window.veloce?.pickFolders()) ?? [];
    if (picked.length > 0) {
      onAddPaths(picked);
    }
  };

  const handleDrop = (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setDragging(false);
    const files = Array.from(e.dataTransfer.files);
    const paths = files
      .map((f) => window.veloce?.getPathForFile(f) ?? "")
      .filter((p) => p.length > 0);
    if (paths.length > 0) {
      onAddPaths(paths);
    }
  };

  const handleSendToLanPeer = async (peer: PeerDto) => {
    let paths = selectedPaths;
    if (paths.length === 0) {
      paths = (await window.veloce?.pickFiles()) ?? [];
      if (paths.length === 0) return;
      onAddPaths(paths);
    }
    await onStartSession(paths, peer.nodeId);
  };

  const handleCreateGlobalCode = async () => {
    let paths = selectedPaths;
    if (paths.length === 0) {
      paths = (await window.veloce?.pickFiles()) ?? [];
      if (paths.length === 0) return;
      onAddPaths(paths);
    }
    await onStartSession(paths);
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
        className={`dropzone ${dragging ? "dropzone-active" : ""} ${selectedPaths.length > 0 ? "dropzone-has-files" : ""}`}
        onDragOver={(e) => {
          e.preventDefault();
          setDragging(true);
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={handleDrop}
      >
        {selectedPaths.length === 0 ? (
          <>
            <div className="dropzone-icon">
              <UploadCloud size={26} />
            </div>
            <h2>Drop files or folders to send</h2>
            <p className="muted-sm">
              Zero-buffer streaming • Missing-chunk auto-resume • End-to-end encrypted P2P
            </p>
            <div className="dropzone-buttons">
              <button
                className="btn-primary"
                type="button"
                disabled={busy}
                onClick={handleSelectFiles}
              >
                <FileUp size={16} />
                <span>Select Files</span>
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
          </>
        ) : (
          <div className="staged-files-box">
            <div className="staged-header">
              <div className="staged-title-group">
                <strong>
                  {selectedPaths.length}{" "}
                  {selectedPaths.length === 1 ? "item ready" : "items ready"}
                </strong>
                <span className="muted-xs">Remove any item with × or add more</span>
              </div>
              <div className="staged-header-actions">
                <button
                  className="btn-secondary btn-sm"
                  type="button"
                  disabled={busy}
                  onClick={handleSelectFiles}
                >
                  <FileUp size={13} />
                  <span>+ Files</span>
                </button>
                <button
                  className="btn-secondary btn-sm"
                  type="button"
                  disabled={busy}
                  onClick={handleSelectFolders}
                >
                  <FolderPlus size={13} />
                  <span>+ Folder</span>
                </button>
                <button
                  className="btn-ghost-danger btn-sm"
                  type="button"
                  disabled={busy}
                  onClick={onClearPaths}
                  title="Unselect all items"
                >
                  <Trash2 size={13} />
                  <span>Clear All</span>
                </button>
              </div>
            </div>

            <ul className="staged-file-list">
              {selectedPaths.map((p) => {
                const name = fileBaseName(p);
                const folder = parentFolderHint(p);
                return (
                  <li key={p} className="staged-file-item" title={p}>
                    <FileText size={15} className="staged-file-icon" />
                    <div className="staged-file-text">
                      <span className="staged-file-name">{name}</span>
                      {folder && <span className="muted-xs">{folder}</span>}
                    </div>
                    <button
                      type="button"
                      className="file-remove-btn"
                      aria-label={`Remove ${name}`}
                      title={`Unselect ${name}`}
                      onClick={() => onRemovePath(p)}
                    >
                      <X size={14} />
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        )}
      </div>

      <section className="panel">
        <div className="panel-header">
          <div className="panel-title-inline">
            <span className="pulse-dot" />
            <h3>Nearby Devices (Same Wi-Fi / LAN)</h3>
          </div>
          <span className="muted-xs">
            {peers.length > 0
              ? `${peers.length} online • live ~1s sync`
              : "Auto-updating within ~1s"}
          </span>
        </div>

        {peers.length === 0 ? (
          <div className="empty-state">
            <Laptop size={20} />
            <p className="muted-sm">
              Open Veloce on another device on the same Wi-Fi or Ethernet — it appears here within
              a second for instant 1-click transfer.
            </p>
          </div>
        ) : (
          <div className="peers-grid">
            {peers.map((peer) => {
              const isTrusted = trustedNodeIds.has(peer.nodeId);
              return (
                <div key={peer.nodeId} className="peer-card peer-card-row">
                  <div className="peer-avatar">
                    <Laptop size={17} />
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
                      <span>
                        {selectedPaths.length > 0
                          ? `Send (${selectedPaths.length})`
                          : "Send"}
                      </span>
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </section>

      {activeSession ? (
        <QrCodeCard
          session={activeSession}
          hasRelay={hasRelay}
          onStopSharing={onStopSharing}
        />
      ) : (
        <section className="panel global-share-panel">
          <div className="panel-header">
            <h3 className="panel-title-inline">
              <Globe size={16} className="accent-icon" />
              <span>Global Transfer (Send Anywhere Over Internet)</span>
            </h3>
            <button
              className="btn-primary btn-sm"
              type="button"
              disabled={busy}
              onClick={handleCreateGlobalCode}
            >
              <QrCode size={14} />
              <span>
                {busy
                  ? "Preparing..."
                  : selectedPaths.length > 0
                    ? `Generate Global Code (${selectedPaths.length})`
                    : "Pick Files & Generate Code"}
              </span>
            </button>
          </div>

          <div className="how-it-works-grid">
            <div className="step-chip">
              <span className="step-num">1</span>
              <div>
                <strong>Generate Code</strong>
                <span className="muted-xs">Creates a one-time encrypted ticket &amp; QR</span>
              </div>
            </div>
            <div className="step-chip">
              <span className="step-num">2</span>
              <div>
                <strong>Share With Receiver</strong>
                <span className="muted-xs">Send the veloce1… code or scan QR on phone</span>
              </div>
            </div>
            <div className="step-chip">
              <span className="step-num">3</span>
              <div>
                <strong>Direct Stream</strong>
                <span className="muted-xs">Receiver pastes in Receive • resumes missing chunks</span>
              </div>
            </div>
          </div>
        </section>
      )}

      <section className="panel quick-text-panel">
        <div className="panel-header">
          <h3 className="panel-title-inline">
            <MessageSquareText size={15} />
            <span>Quick Text / Link</span>
          </h3>
          <div className="panel-header-actions">
            {hasQuickText && (
              <button
                className="btn-secondary btn-sm"
                type="button"
                disabled={busy}
                onClick={() => setQuickText("")}
              >
                <X size={13} />
                <span>Clear</span>
              </button>
            )}
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
        </div>
        <textarea
          className="text-input quick-text-input"
          rows={2}
          value={quickText}
          onChange={(e) => setQuickText(e.target.value)}
          placeholder="Paste a link, Wi-Fi password, or snippet to beam over encrypted P2P..."
        />
        <div className="quick-text-actions">
          <span className="muted-xs">
            {hasQuickText
              ? "Click Text on a nearby device above, or generate a Global QR / Code."
              : "Instant encrypted clipboard beam across LAN or Global Code."}
          </span>
          <button
            className="btn-secondary btn-sm"
            type="button"
            disabled={busy || !hasQuickText}
            onClick={() => handleBeamText()}
          >
            <QrCode size={13} />
            <span>Create Text QR / Code</span>
          </button>
        </div>
      </section>
    </div>
  );
}
