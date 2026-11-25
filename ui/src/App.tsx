import {
  ArrowDownLeft,
  ArrowUpRight,
  Moon,
  Shield,
  SlidersHorizontal,
  Sun,
  Trash2,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { IncomingOfferModal } from "./components/IncomingOfferModal";
import { ReceiveView } from "./components/ReceiveView";
import { SendView } from "./components/SendView";
import { SettingsModal } from "./components/SettingsModal";
import { TransferCard } from "./components/TransferCard";
import { VeloceLogo } from "./components/VeloceLogo";
import type {
  DesktopSettings,
  EngineEvent,
  EngineStatus,
  IncomingOfferEvent,
  PeerDto,
  StartSessionResult,
  TransferRecord,
} from "./types/veloce";

type ActiveTab = "send" | "receive";
type ThemeMode = "dark" | "light";

const HISTORY_STORAGE_KEY = "veloce.desktopTransferHistory";

function humanizeError(raw: unknown): string {
  const msg = raw instanceof Error ? raw.message : String(raw);
  if (msg.includes("could not reach peer directly or via any configured relay")) {
    return (
      "Could not reach sender over LAN or Direct P2P. Ensure the sender still has Veloce open, " +
      "or configure a Veloce Relay server (host:port) in Settings for cellular / strict NAT networks."
    );
  }
  return msg;
}

function loadSavedHistory(): Record<string, TransferRecord> {
  try {
    const raw = localStorage.getItem(HISTORY_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as TransferRecord[];
    if (!Array.isArray(parsed)) return {};
    const out: Record<string, TransferRecord> = {};
    for (const item of parsed) {
      if (item && typeof item.transferId === "string" && item.transferId.length > 0) {
        out[item.transferId] = {
          ...item,
          status: item.status === "active" ? "cancelled" : item.status,
          speedBytesPerSec: 0,
          etaSeconds: 0,
        };
      }
    }
    return out;
  } catch {
    return {};
  }
}

function persistHistory(records: Record<string, TransferRecord>): void {
  try {
    const finished = Object.values(records)
      .filter(
        (r) =>
          r.status === "completed" || r.status === "failed" || r.status === "cancelled",
      )
      .slice(-100);
    localStorage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(finished));
  } catch {
    // Ignore storage quota errors.
  }
}

export function App() {
  const [tab, setTab] = useState<ActiveTab>("send");
  const [theme, setTheme] = useState<ThemeMode>("dark");
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [settings, setSettings] = useState<DesktopSettings>({
    deviceName: "Veloce Desktop",
    downloadDir: "",
    relayAddrs: [],
    parallelStreams: 8,
    theme: "dark",
  });
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [savingSettings, setSavingSettings] = useState(false);
  const [peers, setPeers] = useState<PeerDto[]>([]);
  const [outputDir, setOutputDir] = useState<string>("");
  const [activeSession, setActiveSession] = useState<StartSessionResult | null>(null);
  const [transfers, setTransfers] = useState<Record<string, TransferRecord>>(() =>
    loadSavedHistory(),
  );
  const [pendingOffer, setPendingOffer] = useState<IncomingOfferEvent | null>(null);
  const [busy, setBusy] = useState(false);
  const [bannerError, setBannerError] = useState<string | null>(null);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    persistHistory(transfers);
  }, [transfers]);

  useEffect(() => {
    if (!window.veloce) {
      setStatus({
        ready: false,
        error: "Electron preload bridge unavailable (running outside Electron).",
        nodeInfo: null,
      });
      return;
    }

    let unsub: (() => void) | undefined;

    (async () => {
      const s = await window.veloce!.getStatus();
      setStatus(s);
      if (s.settings) {
        setSettings(s.settings);
        setTheme(s.settings.theme);
        setOutputDir(s.settings.downloadDir);
      } else if (s.defaultOutputDir) {
        setOutputDir(s.defaultOutputDir);
      }

      const initialPeers = await window.veloce!.listLocalPeers();
      setPeers(initialPeers);

      unsub = window.veloce!.onEvent((evt: EngineEvent) => {
        handleEngineEvent(evt);
      });
    })();

    return () => {
      unsub?.();
    };
  }, []);

  const handleEngineEvent = (evt: EngineEvent) => {
    switch (evt.type) {
      case "PeerDiscovered":
        setPeers((prev) => {
          const rest = prev.filter((p) => p.nodeId !== evt.peer.nodeId);
          return [...rest, evt.peer];
        });
        break;
      case "PeerLost":
        setPeers((prev) => prev.filter((p) => p.nodeId !== evt.nodeId));
        break;
      case "IncomingOffer":
        setPendingOffer(evt);
        break;
      case "ConnectionTypeChanged":
        setTransfers((prev) => {
          const existing = prev[evt.transferId];
          if (!existing) return prev;
          return {
            ...prev,
            [evt.transferId]: { ...existing, connectionType: evt.connectionType },
          };
        });
        break;
      case "TransferProgress":
        setTransfers((prev) => {
          const existing = prev[evt.transferId];
          const history = [...(existing?.speedHistory ?? []), evt.speedBytesPerSec].slice(-24);
          return {
            ...prev,
            [evt.transferId]: {
              transferId: evt.transferId,
              label:
                existing?.label ??
                (evt.direction === "send"
                  ? `Sending #${evt.transferId}`
                  : `Receiving #${evt.transferId}`),
              direction: evt.direction,
              status: "active",
              bytesTransferred: evt.bytesTransferred,
              totalBytes: evt.totalBytes,
              chunksVerified: evt.chunksVerified,
              totalChunks: evt.totalChunks,
              speedBytesPerSec: evt.speedBytesPerSec,
              speedHistory: history,
              etaSeconds: evt.etaSeconds,
              connectionType: evt.connectionType,
            },
          };
        });
        break;
      case "TransferPaused":
        setTransfers((prev) => {
          const existing = prev[evt.transferId];
          if (!existing) return prev;
          return {
            ...prev,
            [evt.transferId]: {
              ...existing,
              status: "paused",
              speedBytesPerSec: 0,
              etaSeconds: 0,
            },
          };
        });
        break;
      case "TransferResumed":
        setTransfers((prev) => {
          const existing = prev[evt.transferId];
          if (!existing) return prev;
          return {
            ...prev,
            [evt.transferId]: {
              ...existing,
              status: "active",
              error: undefined,
            },
          };
        });
        break;
      case "TransferCompleted":
        setTransfers((prev) => {
          const existing = prev[evt.transferId];
          return {
            ...prev,
            [evt.transferId]: {
              transferId: evt.transferId,
              label:
                existing?.label ??
                (evt.direction === "send"
                  ? `Sent #${evt.transferId}`
                  : `Received #${evt.transferId}`),
              direction: evt.direction,
              status: "completed",
              bytesTransferred: evt.totalBytes,
              totalBytes: evt.totalBytes,
              chunksVerified: evt.totalChunks,
              totalChunks: evt.totalChunks,
              speedBytesPerSec: evt.averageSpeedBytesPerSec,
              speedHistory: [
                ...(existing?.speedHistory ?? []),
                evt.averageSpeedBytesPerSec,
              ].slice(-24),
              etaSeconds: 0,
              connectionType: existing?.connectionType ?? "LAN",
              outputPaths: evt.outputPaths,
            },
          };
        });
        break;
      case "TransferFailed":
        setTransfers((prev) => {
          const existing = prev[evt.transferId];
          const isCancel = evt.error.toLowerCase().includes("cancel");
          const friendly = humanizeError(evt.error);
          return {
            ...prev,
            [evt.transferId]: {
              transferId: evt.transferId,
              label: existing?.label ?? `Transfer #${evt.transferId}`,
              direction: evt.direction,
              status: isCancel ? "cancelled" : "failed",
              bytesTransferred: existing?.bytesTransferred ?? 0,
              totalBytes: existing?.totalBytes ?? 0,
              chunksVerified: existing?.chunksVerified ?? 0,
              totalChunks: existing?.totalChunks ?? 0,
              speedBytesPerSec: 0,
              speedHistory: existing?.speedHistory ?? [],
              etaSeconds: 0,
              connectionType: existing?.connectionType ?? "LAN",
              error: friendly,
            },
          };
        });
        break;
    }
  };

  const handleStartSession = async (paths: string[], targetPeerNodeId?: string) => {
    setBusy(true);
    setBannerError(null);
    try {
      const res = await window.veloce!.startSession({
        paths,
        targetPeerNodeId,
        relayHints: settings.relayAddrs.length > 0 ? settings.relayAddrs : undefined,
      });
      if (!targetPeerNodeId) {
        setActiveSession(res);
      } else {
        const label =
          res.fileCount === 1
            ? res.files[0]?.relativePath ?? "File"
            : `${res.fileCount} files`;
        setTransfers((prev) => ({
          ...prev,
          [res.transferId]: {
            transferId: res.transferId,
            label,
            direction: "send",
            status: "active",
            bytesTransferred: 0,
            totalBytes: res.totalSize,
            chunksVerified: 0,
            totalChunks: res.totalChunks,
            speedBytesPerSec: 0,
            speedHistory: [],
            etaSeconds: 0,
            connectionType: "LAN",
          },
        }));
      }
    } catch (err) {
      setBannerError(humanizeError(err));
    } finally {
      setBusy(false);
    }
  };

  const handleStopSharing = async () => {
    if (!activeSession) return;
    const tid = activeSession.transferId;
    setActiveSession(null);
    try {
      await window.veloce?.cancelTransfer(tid);
    } catch {
      // Ignore cancellation errors.
    }
  };

  const handleJoinSession = async (ticketCode: string): Promise<boolean> => {
    setBusy(true);
    setBannerError(null);
    try {
      const res = await window.veloce!.joinSession({
        ticketCode,
        outputDir,
        parallelStreams: settings.parallelStreams,
      });
      setTransfers((prev) => ({
        ...prev,
        [res.transferId]: {
          transferId: res.transferId,
          label: `${res.fileCount} item(s) via Global Ticket`,
          direction: "receive",
          status: "active",
          bytesTransferred: 0,
          totalBytes: res.totalSize,
          chunksVerified: 0,
          totalChunks: 0,
          speedBytesPerSec: 0,
          speedHistory: [],
          etaSeconds: 0,
          connectionType: "Direct-P2P",
        },
      }));
      return true;
    } catch (err) {
      setBannerError(humanizeError(err));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const handleChooseOutputDir = async () => {
    const dir = await window.veloce?.pickOutputDir();
    if (dir) setOutputDir(dir);
  };

  const handleToggleTheme = () => {
    const nextTheme: ThemeMode = theme === "dark" ? "light" : "dark";
    setTheme(nextTheme);
    setSettings((prev) => ({ ...prev, theme: nextTheme }));
    void window.veloce?.updateSettings({ theme: nextTheme });
  };

  const handleSaveSettings = async (next: DesktopSettings) => {
    if (!window.veloce) return;
    setSavingSettings(true);
    setBannerError(null);
    try {
      const result = await window.veloce.updateSettings(next);
      setSettings(result.settings);
      setStatus(result.status);
      setOutputDir(result.settings.downloadDir);
      setActiveSession(null);
      const refreshedPeers = await window.veloce.listLocalPeers();
      setPeers(refreshedPeers);
    } finally {
      setSavingSettings(false);
    }
  };

  const handleAcceptOffer = async () => {
    if (!pendingOffer || !outputDir) return;
    const offer = pendingOffer;
    setPendingOffer(null);
    setTransfers((prev) => ({
      ...prev,
      [offer.transferId]: {
        transferId: offer.transferId,
        label: `From ${offer.senderName} (${offer.fileCount} item(s))`,
        direction: "receive",
        status: "active",
        bytesTransferred: 0,
        totalBytes: offer.totalSize,
        chunksVerified: 0,
        totalChunks: 0,
        speedBytesPerSec: 0,
        speedHistory: [],
        etaSeconds: 0,
        connectionType: offer.connectionType,
      },
    }));
    await window.veloce?.acceptIncoming({ transferId: offer.transferId, outputDir });
  };

  const handleRejectOffer = async () => {
    if (!pendingOffer) return;
    const offer = pendingOffer;
    setPendingOffer(null);
    await window.veloce?.rejectIncoming({ transferId: offer.transferId });
  };

  const handlePauseTransfer = async (transferId: string) => {
    await window.veloce?.pauseTransfer(transferId);
  };

  const handleResumeTransfer = async (transferId: string) => {
    setBannerError(null);
    try {
      const ok = await window.veloce?.resumeTransfer(transferId);
      if (ok) {
        setTransfers((prev) => {
          const existing = prev[transferId];
          if (!existing) return prev;
          return {
            ...prev,
            [transferId]: { ...existing, status: "active", error: undefined },
          };
        });
      }
    } catch (err) {
      setBannerError(humanizeError(err));
    }
  };

  const handleCancelTransfer = async (transferId: string) => {
    await window.veloce?.cancelTransfer(transferId);
    if (activeSession?.transferId === transferId) {
      setActiveSession(null);
    }
    setTransfers((prev) => {
      const existing = prev[transferId];
      if (!existing) return prev;
      return {
        ...prev,
        [transferId]: {
          ...existing,
          status: "cancelled",
          speedBytesPerSec: 0,
          etaSeconds: 0,
        },
      };
    });
  };

  const handleRemoveTransfer = async (transferId: string) => {
    const existing = transfers[transferId];
    if (existing && (existing.status === "active" || existing.status === "paused")) {
      await window.veloce?.cancelTransfer(transferId);
    }
    if (activeSession?.transferId === transferId) {
      setActiveSession(null);
    }
    setTransfers((prev) => {
      const next = { ...prev };
      delete next[transferId];
      return next;
    });
  };

  const handleClearHistory = () => {
    setTransfers((prev) => {
      const next: Record<string, TransferRecord> = {};
      for (const [id, tr] of Object.entries(prev)) {
        if (tr.status === "active" || tr.status === "paused") {
          next[id] = tr;
        }
      }
      return next;
    });
  };

  const transferList = Object.values(transfers).reverse();
  const hasFinishedTransfers = transferList.some(
    (tr) => tr.status === "completed" || tr.status === "failed" || tr.status === "cancelled",
  );

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">
            <VeloceLogo size={36} />
          </div>
          <div>
            <h1>Veloce</h1>
            <span className="muted-xs">QUIC (BBR) • BLAKE3 Merkle • E2EE P2P</span>
          </div>
        </div>

        <div className="topbar-right">
          {status?.nodeInfo && (
            <div className="node-chip" title={`Full Node ID: ${status.nodeInfo.nodeId}`}>
              <Shield size={13} />
              <span>{status.nodeInfo.deviceName}</span>
              <code>{status.nodeInfo.shortId}</code>
            </div>
          )}
          <button
            className="icon-btn"
            type="button"
            aria-label="Toggle dark or light theme"
            title="Toggle theme"
            onClick={handleToggleTheme}
          >
            {theme === "dark" ? <Sun size={16} /> : <Moon size={16} />}
          </button>
          <button
            className="icon-btn"
            type="button"
            aria-label="Open settings"
            title="Settings"
            onClick={() => setSettingsOpen(true)}
          >
            <SlidersHorizontal size={16} />
          </button>
        </div>
      </header>

      {status && !status.ready && status.error && (
        <div className="alert-banner">{status.error}</div>
      )}
      {bannerError && (
        <div className="alert-banner alert-banner-dismissible">
          <span>{bannerError}</span>
          <button
            className="icon-btn"
            type="button"
            aria-label="Dismiss error"
            onClick={() => setBannerError(null)}
          >
            <X size={14} />
          </button>
        </div>
      )}

      <nav className="tab-bar" role="tablist">
        <button
          role="tab"
          aria-selected={tab === "send"}
          type="button"
          className={`tab-btn ${tab === "send" ? "tab-active" : ""}`}
          onClick={() => setTab("send")}
        >
          <ArrowUpRight size={16} />
          <span>Send</span>
        </button>
        <button
          role="tab"
          aria-selected={tab === "receive"}
          type="button"
          className={`tab-btn ${tab === "receive" ? "tab-active" : ""}`}
          onClick={() => setTab("receive")}
        >
          <ArrowDownLeft size={16} />
          <span>Receive</span>
        </button>
      </nav>

      <main className="main-grid">
        <section className="primary-column">
          {tab === "send" ? (
            <SendView
              peers={peers}
              activeSession={activeSession}
              hasRelay={settings.relayAddrs.length > 0}
              busy={busy}
              onStartSession={handleStartSession}
              onStopSharing={handleStopSharing}
            />
          ) : (
            <ReceiveView
              outputDir={outputDir}
              busy={busy}
              onChangeOutputDir={handleChooseOutputDir}
              onJoinSession={handleJoinSession}
            />
          )}
        </section>

        <aside className="telemetry-column">
          <div className="panel telemetry-panel">
            <div className="panel-header">
              <h3>Live Transfers</h3>
              <div className="panel-header-actions">
                <span className="muted-xs">{transferList.length} session(s)</span>
                {hasFinishedTransfers && (
                  <button
                    className="btn-secondary btn-sm"
                    type="button"
                    title="Clear completed, failed, and cancelled transfers"
                    onClick={handleClearHistory}
                  >
                    <Trash2 size={13} />
                    <span>Clear History</span>
                  </button>
                )}
              </div>
            </div>

            {transferList.length === 0 ? (
              <p className="muted-sm empty-telemetry">
                Active transfers will stream real-time MB/s throughput, ETA, BLAKE3 chunk
                verification, and connection path badges here.
              </p>
            ) : (
              <div className="transfer-list">
                {transferList.map((tr) => (
                  <TransferCard
                    key={tr.transferId}
                    transfer={tr}
                    onPause={handlePauseTransfer}
                    onResume={handleResumeTransfer}
                    onCancel={handleCancelTransfer}
                    onRemove={handleRemoveTransfer}
                  />
                ))}
              </div>
            )}
          </div>
        </aside>
      </main>

      {pendingOffer && (
        <IncomingOfferModal
          offer={pendingOffer}
          outputDir={outputDir}
          onChangeOutputDir={handleChooseOutputDir}
          onAccept={handleAcceptOffer}
          onReject={handleRejectOffer}
        />
      )}

      {settingsOpen && (
        <SettingsModal
          settings={settings}
          saving={savingSettings}
          onBrowseDownloadDir={async () => (await window.veloce?.pickOutputDir()) ?? null}
          onSave={handleSaveSettings}
          onClose={() => setSettingsOpen(false)}
        />
      )}
    </div>
  );
}
