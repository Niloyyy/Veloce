import {
  ArrowDownLeft,
  ArrowUpRight,
  Moon,
  Shield,
  SlidersHorizontal,
  Sun,
  Zap,
} from "lucide-react";
import { useEffect, useState } from "react";
import { IncomingOfferModal } from "./components/IncomingOfferModal";
import { ReceiveView } from "./components/ReceiveView";
import { SendView } from "./components/SendView";
import { SettingsModal } from "./components/SettingsModal";
import { TransferCard } from "./components/TransferCard";
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
  const [transfers, setTransfers] = useState<Record<string, TransferRecord>>({});
  const [pendingOffer, setPendingOffer] = useState<IncomingOfferEvent | null>(null);
  const [busy, setBusy] = useState(false);
  const [bannerError, setBannerError] = useState<string | null>(null);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

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
                (evt.direction === "send" ? `Sending #${evt.transferId}` : `Receiving #${evt.transferId}`),
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
              label: existing?.label ?? `Transfer #${evt.transferId}`,
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
              error: evt.error,
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
      setActiveSession(res);
      if (targetPeerNodeId) {
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
      setBannerError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleJoinSession = async (ticketCode: string) => {
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
    } catch (err) {
      setBannerError(err instanceof Error ? err.message : String(err));
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
    try {
      await window.veloce?.resumeTransfer(transferId);
    } catch (err) {
      setBannerError(err instanceof Error ? err.message : String(err));
    }
  };

  const handleCancelTransfer = async (transferId: string) => {
    await window.veloce?.cancelTransfer(transferId);
  };

  const transferList = Object.values(transfers).reverse();

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">
            <Zap size={18} />
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
      {bannerError && <div className="alert-banner">{bannerError}</div>}

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
              busy={busy}
              onStartSession={handleStartSession}
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
              <span className="muted-xs">{transferList.length} session(s)</span>
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
