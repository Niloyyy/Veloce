import { FileUp, FolderPlus, Laptop, Send, UploadCloud } from "lucide-react";
import { useState } from "react";
import type { PeerDto, StartSessionResult } from "../types/veloce";
import { QrCodeCard } from "./QrCodeCard";

interface SendViewProps {
  peers: PeerDto[];
  activeSession: StartSessionResult | null;
  hasRelay: boolean;
  busy: boolean;
  onStartSession: (paths: string[], targetPeerNodeId?: string) => Promise<void>;
  onStopSharing: () => void;
}

export function SendView({
  peers,
  activeSession,
  hasRelay,
  busy,
  onStartSession,
  onStopSharing,
}: SendViewProps) {
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
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
          Instant BLAKE3 Merkle chunking • Multi-stream QUIC (BBR) • Line-speed LAN & Global P2P
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
            {peers.map((peer) => (
              <button
                key={peer.nodeId}
                type="button"
                className="peer-card"
                disabled={busy}
                onClick={() => handleSendToLanPeer(peer)}
              >
                <div className="peer-avatar">
                  <Laptop size={18} />
                </div>
                <div className="peer-info">
                  <strong>{peer.deviceName}</strong>
                  <span className="muted-xs">
                    {peer.shortId} • {peer.addresses[0] ?? "LAN"}
                  </span>
                </div>
                <span className="peer-send-tag">
                  <Send size={13} />
                  Send
                </span>
              </button>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
