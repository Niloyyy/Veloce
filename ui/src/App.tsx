import {
  ArrowDownLeft,
  ArrowUpRight,
  Palette,
  Pencil,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
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
  ThemeId,
  TransferRecord,
  TrustedPeer,
} from "./types/veloce";
import { formatTextClipLabel, isTextClipPath } from "./utils/textClip";
import { nextThemeId, themeLabel } from "./utils/themes";

type ActiveTab = "send" | "receive";

const HISTORY_STORAGE_KEY = "veloce.desktopTransferHistory";

function humanizeError(raw: unknown): string {
  const msg = raw instanceof Error ? raw.message : String(raw);
  if (msg.includes("could not reach peer directly or via any configured relay")) {
    return (
      "Could not connect to sender. Make sure the sender still has Veloce open, " +
      "or add a Relay Server under Settings → Advanced for strict mobile networks."
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
  const [theme, setTheme] = useState<ThemeId>("dark");
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [settings, setSettings] = useState<DesktopSettings>({
    deviceName: "",
    downloadDir: "",
    relayAddrs: [],
    parallelStreams: 8,
    theme: "dark",
    runInBackground: true,
    trustedPeers: [],
  });
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [savingSettings, setSavingSettings] = useState(false);
  const [peers, setPeers] = useState<PeerDto[]>([]);
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
  const [outputDir, setOutputDir] = useState<string>("");
  const [activeSession, setActiveSession] = useState<StartSessionResult | null>(null);
  const [transfers, setTransfers] = useState<Record<string, TransferRecord>>(() =>
    loadSavedHistory(),
  );
  const [pendingOffer, setPendingOffer] = useState<IncomingOfferEvent | null>(null);
  const [busy, setBusy] = useState(false);
  const [bannerError, setBannerError] = useState<string | null>(null);

  const trustedPeersRef = useRef<TrustedPeer[]>(settings.trustedPeers);
  const outputDirRef = useRef<string>(outputDir);
  const pendingProgressRef = useRef<
    Map<string, Extract<EngineEvent, { type: "TransferProgress" }>>
  >(new Map());
  const progressRafRef = useRef<number | null>(null);

  useEffect(() => {
    trustedPeersRef.current = settings.trustedPeers;
  }, [settings.trustedPeers]);

  useEffect(() => {
    outputDirRef.current = outputDir;
  }, [outputDir]);

  useEffect(() => {
    return () => {
      if (progressRafRef.current !== null) {
        cancelAnimationFrame(progressRafRef.current);
      }
    };
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    void window.veloce?.previewTheme?.(theme);
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
      case "IncomingOffer": {
        const isTrusted = trustedPeersRef.current.some(
          (p) => p.nodeId === evt.senderNodeId,
        );
        const isTextClip =
          evt.fileCount === 1 &&
          evt.files.length === 1 &&
          isTextClipPath(evt.files[0].relativePath);
        const destDir = outputDirRef.current;

        if (isTrusted && destDir) {
          setTransfers((prev) => ({
            ...prev,
            [evt.transferId]: {
              transferId: evt.transferId,
              label: isTextClip
                ? `Text from ${evt.senderName}`
                : `From ${evt.senderName} (${evt.fileCount} ${evt.fileCount === 1 ? "file" : "files"})`,
              direction: "receive",
              status: "active",
              bytesTransferred: 0,
              totalBytes: evt.totalSize,
              chunksVerified: 0,
              totalChunks: 0,
              speedBytesPerSec: 0,
              speedHistory: [],
              etaSeconds: 0,
              connectionType: evt.connectionType,
            },
          }));
          void window.veloce?.acceptIncoming({
            transferId: evt.transferId,
            outputDir: destDir,
          });
        } else {
          setPendingOffer(evt);
        }
        break;
      }
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
        pendingProgressRef.current.set(evt.transferId, evt);
        if (progressRafRef.current === null) {
          progressRafRef.current = requestAnimationFrame(() => {
            progressRafRef.current = null;
            const batch = Array.from(pendingProgressRef.current.values());
            pendingProgressRef.current.clear();
            if (batch.length === 0) return;
            setTransfers((prev) => {
              const next = { ...prev };
              for (const p of batch) {
                const existing = next[p.transferId];
                const initialSkipped =
                  existing?.initialChunksVerified ??
                  (p.speedBytesPerSec === 0 && p.chunksVerified > 0
                    ? p.chunksVerified
                    : 0);
                const history =
                  p.speedBytesPerSec > 0
                    ? [...(existing?.speedHistory ?? []), p.speedBytesPerSec].slice(-24)
                    : (existing?.speedHistory ?? []);
                next[p.transferId] = {
                  transferId: p.transferId,
                  label:
                    existing?.label ??
                    (p.direction === "send" ? "Sending files" : "Receiving files"),
                  direction: p.direction,
                  status: "active",
                  bytesTransferred: p.bytesTransferred,
                  totalBytes: p.totalBytes,
                  chunksVerified: p.chunksVerified,
                  totalChunks: p.totalChunks,
                  initialChunksVerified:
                    initialSkipped > 0 ? initialSkipped : existing?.initialChunksVerified,
                  speedBytesPerSec: p.speedBytesPerSec,
                  speedHistory: history,
                  etaSeconds: p.etaSeconds,
                  elapsedMs: p.elapsedMs > 0 ? p.elapsedMs : existing?.elapsedMs,
                  connectionType: p.connectionType,
                  textContent: existing?.textContent,
                };
              }
              return next;
            });
          });
        }
        break;
      case "TransferPaused":
        pendingProgressRef.current.delete(evt.transferId);
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
      case "TransferCompleted": {
        pendingProgressRef.current.delete(evt.transferId);
        const isReceivedTextClip =
          evt.direction === "receive" &&
          evt.outputPaths.length === 1 &&
          isTextClipPath(evt.outputPaths[0]);

        setTransfers((prev) => {
          const existing = prev[evt.transferId];
          return {
            ...prev,
            [evt.transferId]: {
              transferId: evt.transferId,
              label:
                existing?.label ??
                (evt.direction === "send" ? "Sent files" : "Received files"),
              direction: evt.direction,
              status: "completed",
              bytesTransferred: evt.totalBytes,
              totalBytes: evt.totalBytes,
              chunksVerified: evt.totalChunks,
              totalChunks: evt.totalChunks,
              initialChunksVerified: existing?.initialChunksVerified,
              speedBytesPerSec: evt.averageSpeedBytesPerSec,
              speedHistory: [
                ...(existing?.speedHistory ?? []),
                evt.averageSpeedBytesPerSec,
              ].slice(-24),
              etaSeconds: 0,
              elapsedMs: evt.elapsedMs > 0 ? evt.elapsedMs : existing?.elapsedMs,
              connectionType: existing?.connectionType ?? "LAN",
              outputPaths: isReceivedTextClip ? [] : evt.outputPaths,
              textContent: existing?.textContent,
            },
          };
        });

        if (isReceivedTextClip) {
          const clipPath = evt.outputPaths[0];
          const tid = evt.transferId;
          void window.veloce?.readTextClip(clipPath).then((content) => {
            if (content == null) return;
            setTransfers((prev) => {
              const existing = prev[tid];
              if (!existing) return prev;
              return {
                ...prev,
                [tid]: {
                  ...existing,
                  label: formatTextClipLabel(content, "Received"),
                  textContent: content,
                  outputPaths: [],
                },
              };
            });
          });
        }
        break;
      }
      case "TransferFailed":
        pendingProgressRef.current.delete(evt.transferId);
        setTransfers((prev) => {
          const existing = prev[evt.transferId];
          const isCancel = evt.error.toLowerCase().includes("cancel");
          const friendly = humanizeError(evt.error);
          return {
            ...prev,
            [evt.transferId]: {
              transferId: evt.transferId,
              label: existing?.label ?? "Transfer",
              direction: evt.direction,
              status: isCancel ? "cancelled" : "failed",
              bytesTransferred: existing?.bytesTransferred ?? 0,
              totalBytes: existing?.totalBytes ?? 0,
              chunksVerified: existing?.chunksVerified ?? 0,
              totalChunks: existing?.totalChunks ?? 0,
              initialChunksVerified: existing?.initialChunksVerified,
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

  const handleStopSharing = useCallback(async () => {
    setActiveSession((prev) => {
      if (prev) {
        void window.veloce?.cancelTransfer(prev.transferId);
      }
      return null;
    });
  }, []);

  const handleAddSelectedPaths = useCallback(
    (incoming: string[]) => {
      if (incoming.length === 0) return;
      setSelectedPaths((prev) => {
        const merged = [...prev];
        let changed = false;
        for (const p of incoming) {
          if (p && !merged.includes(p)) {
            merged.push(p);
            changed = true;
          }
        }
        if (changed) {
          void handleStopSharing();
        }
        return changed ? merged : prev;
      });
    },
    [handleStopSharing],
  );

  const handleRemoveSelectedPath = useCallback(
    (targetPath: string) => {
      setSelectedPaths((prev) => {
        const next = prev.filter((p) => p !== targetPath);
        if (next.length !== prev.length) {
          void handleStopSharing();
        }
        return next;
      });
    },
    [handleStopSharing],
  );

  const handleClearSelectedPaths = useCallback(() => {
    setSelectedPaths([]);
    void handleStopSharing();
  }, [handleStopSharing]);

  const handleStartSession = async (paths: string[], targetPeerNodeId?: string) => {
    if (paths.length === 0) return;
    setBusy(true);
    setBannerError(null);
    try {
      if (!targetPeerNodeId && activeSession) {
        await window.veloce?.cancelTransfer(activeSession.transferId);
      }
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

  const handleSendText = async (text: string, targetPeerNodeId?: string) => {
    const trimmed = text.trim();
    if (!trimmed || !window.veloce) return;
    setBusy(true);
    setBannerError(null);
    try {
      const clip = await window.veloce.createTextClip(trimmed);
      const res = await window.veloce.startSession({
        paths: [clip.path],
        targetPeerNodeId,
        relayHints: settings.relayAddrs.length > 0 ? settings.relayAddrs : undefined,
      });
      if (!targetPeerNodeId) {
        setActiveSession(res);
      } else {
        setTransfers((prev) => ({
          ...prev,
          [res.transferId]: {
            transferId: res.transferId,
            label: formatTextClipLabel(trimmed),
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
            textContent: trimmed,
          },
        }));
      }
    } catch (err) {
      setBannerError(humanizeError(err));
    } finally {
      setBusy(false);
    }
  };

  const handleToggleTrustPeer = async (peer: {
    nodeId: string;
    shortId: string;
    deviceName: string;
  }) => {
    const exists = settings.trustedPeers.some((p) => p.nodeId === peer.nodeId);
    const nextTrusted: TrustedPeer[] = exists
      ? settings.trustedPeers.filter((p) => p.nodeId !== peer.nodeId)
      : [
          ...settings.trustedPeers,
          {
            nodeId: peer.nodeId,
            shortId: peer.shortId,
            deviceName: peer.deviceName,
            addedAt: Date.now(),
          },
        ];
    setSettings((prev) => ({ ...prev, trustedPeers: nextTrusted }));
    try {
      const res = await window.veloce?.updateSettings({ trustedPeers: nextTrusted });
      if (res?.settings) {
        setSettings(res.settings);
      }
    } catch (err) {
      setBannerError(humanizeError(err));
    }
  };

  const handleOpenFile = useCallback(async (filePath: string) => {
    const err = await window.veloce?.openPath(filePath);
    if (err) {
      setBannerError(`Could not open file: ${err}`);
    }
  }, []);

  const handleShowInFolder = useCallback(async (filePath: string) => {
    const ok = await window.veloce?.showInFolder(filePath);
    if (ok === false) {
      setBannerError("File no longer exists at its saved location.");
    }
  }, []);

  const handleOpenUrl = useCallback(async (url: string) => {
    await window.veloce?.openExternalUrl(url);
  }, []);

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
          label: `${res.fileCount} ${res.fileCount === 1 ? "file" : "files"}`,
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
    const nextTheme: ThemeId = nextThemeId(theme);
    setTheme(nextTheme);
    setSettings((prev) => ({ ...prev, theme: nextTheme }));
    void window.veloce?.updateSettings({ theme: nextTheme });
  };

  const handleSaveSettings = async (next: DesktopSettings) => {
    if (!window.veloce) return;
    setSavingSettings(true);
    setBannerError(null);
    try {
      const engineChanged =
        next.deviceName !== settings.deviceName ||
        next.parallelStreams !== settings.parallelStreams ||
        JSON.stringify(next.relayAddrs) !== JSON.stringify(settings.relayAddrs);
      const result = await window.veloce.updateSettings(next);
      setSettings(result.settings);
      setTheme(result.settings.theme);
      setStatus(result.status);
      setOutputDir(result.settings.downloadDir);
      if (engineChanged) {
        setActiveSession(null);
      }
      const refreshedPeers = await window.veloce.listLocalPeers();
      setPeers(refreshedPeers);
    } finally {
      setSavingSettings(false);
    }
  };

  const handleAcceptOffer = async (trustSender = false) => {
    if (!pendingOffer || !outputDir) return;
    const offer = pendingOffer;
    setPendingOffer(null);
    if (
      trustSender &&
      !settings.trustedPeers.some((p) => p.nodeId === offer.senderNodeId)
    ) {
      void handleToggleTrustPeer({
        nodeId: offer.senderNodeId,
        shortId: offer.senderShortId,
        deviceName: offer.senderName,
      });
    }
    const isTextClip =
      offer.fileCount === 1 &&
      offer.files.length === 1 &&
      isTextClipPath(offer.files[0].relativePath);
    setTransfers((prev) => ({
      ...prev,
      [offer.transferId]: {
        transferId: offer.transferId,
        label: isTextClip
          ? `Text from ${offer.senderName}`
          : `From ${offer.senderName} (${offer.fileCount} ${offer.fileCount === 1 ? "file" : "files"})`,
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

  const handlePauseTransfer = useCallback(async (transferId: string) => {
    await window.veloce?.pauseTransfer(transferId);
  }, []);

  const handleResumeTransfer = useCallback(async (transferId: string) => {
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
  }, []);

  const handleCancelTransfer = useCallback(async (transferId: string) => {
    await window.veloce?.cancelTransfer(transferId);
    setActiveSession((prev) => (prev?.transferId === transferId ? null : prev));
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
  }, []);

  const handleRemoveTransfer = useCallback(async (transferId: string) => {
    setTransfers((prev) => {
      const existing = prev[transferId];
      if (existing && (existing.status === "active" || existing.status === "paused")) {
        void window.veloce?.cancelTransfer(transferId);
      }
      const next = { ...prev };
      delete next[transferId];
      return next;
    });
    setActiveSession((prev) => (prev?.transferId === transferId ? null : prev));
  }, []);

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
  const activeCount = transferList.filter((tr) => tr.status === "active").length;
  const hasFinishedTransfers = transferList.some(
    (tr) => tr.status === "completed" || tr.status === "failed" || tr.status === "cancelled",
  );
  const trustedNodeIds = new Set(settings.trustedPeers.map((p) => p.nodeId));

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">
            <VeloceLogo size={32} />
          </div>
          <h1>Veloce</h1>
        </div>

        <nav className="tab-bar" role="tablist" aria-label="Transfer mode">
          <button
            role="tab"
            aria-selected={tab === "send"}
            type="button"
            className={`tab-btn ${tab === "send" ? "tab-active" : ""}`}
            onClick={() => setTab("send")}
          >
            <ArrowUpRight size={15} />
            <span>Send</span>
          </button>
          <button
            role="tab"
            aria-selected={tab === "receive"}
            type="button"
            className={`tab-btn ${tab === "receive" ? "tab-active" : ""}`}
            onClick={() => setTab("receive")}
          >
            <ArrowDownLeft size={15} />
            <span>Receive</span>
          </button>
        </nav>

        <div className="topbar-right">
          {status?.nodeInfo && (
            <button
              type="button"
              className="node-chip node-chip-btn"
              title={`Visible on Wi-Fi as "${status.nodeInfo.deviceName}" (click to rename)`}
              onClick={() => setSettingsOpen(true)}
            >
              <span className="online-dot" />
              <span>{status.nodeInfo.deviceName}</span>
              <Pencil size={11} className="node-chip-edit" />
            </button>
          )}
          <button
            className="icon-btn"
            type="button"
            aria-label={`Switch theme (current: ${themeLabel(theme)})`}
            title={`Theme: ${themeLabel(theme)}`}
            onClick={handleToggleTheme}
          >
            <Palette size={16} />
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

      <main className="main-grid">
        <section className="primary-column">
          {tab === "send" ? (
            <SendView
              peers={peers}
              selectedPaths={selectedPaths}
              trustedNodeIds={trustedNodeIds}
              activeSession={activeSession}
              hasRelay={settings.relayAddrs.length > 0}
              busy={busy}
              onAddPaths={handleAddSelectedPaths}
              onRemovePath={handleRemoveSelectedPath}
              onClearPaths={handleClearSelectedPaths}
              onStartSession={handleStartSession}
              onSendText={handleSendText}
              onToggleTrustPeer={handleToggleTrustPeer}
              onStopSharing={handleStopSharing}
            />
          ) : (
            <ReceiveView
              deviceName={status?.nodeInfo?.deviceName}
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
              <div className="panel-title-inline">
                <h3>Activity</h3>
                {activeCount > 0 && (
                  <span className="count-badge">{activeCount} active</span>
                )}
              </div>
              <div className="panel-header-actions">
                {hasFinishedTransfers && (
                  <button
                    className="btn-secondary btn-sm"
                    type="button"
                    title="Clear completed, failed, and cancelled transfers"
                    onClick={handleClearHistory}
                  >
                    <Trash2 size={13} />
                    <span>Clear</span>
                  </button>
                )}
              </div>
            </div>

            {transferList.length === 0 ? (
              <div className="empty-telemetry-box">
                <p className="muted-sm">No recent transfers</p>
                <span className="muted-xs">
                  Files and links you send or receive will appear here with live speed and one-click
                  open.
                </span>
              </div>
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
                    onOpenFile={handleOpenFile}
                    onShowInFolder={handleShowInFolder}
                    onOpenUrl={handleOpenUrl}
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
          nodeId={status?.nodeInfo?.nodeId}
          detectedDeviceName={status?.detectedDeviceName}
          saving={savingSettings}
          onPreviewTheme={(nextTheme) => setTheme(nextTheme)}
          onBrowseDownloadDir={async () => (await window.veloce?.pickOutputDir()) ?? null}
          onSave={handleSaveSettings}
          onClose={() => {
            setTheme(settings.theme);
            setSettingsOpen(false);
          }}
        />
      )}
    </div>
  );
}
