import {
  ClipboardPaste,
  Download,
  FolderOpen,
  Globe,
  Radio,
  ShieldCheck,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import type { TicketPreview } from "../types/veloce";
import { formatBytes } from "../utils/format";

interface ReceiveViewProps {
  deviceName?: string;
  outputDir: string;
  busy: boolean;
  onChangeOutputDir: () => void;
  onJoinSession: (ticketCode: string) => Promise<boolean>;
}

export function ReceiveView({
  deviceName,
  outputDir,
  busy,
  onChangeOutputDir,
  onJoinSession,
}: ReceiveViewProps) {
  const [ticketInput, setTicketInput] = useState("");
  const [preview, setPreview] = useState<TicketPreview | null>(null);
  const [parseError, setParseError] = useState<string | null>(null);

  useEffect(() => {
    const trimmed = ticketInput.trim();
    if (!trimmed) {
      setPreview(null);
      setParseError(null);
      return;
    }

    let cancelled = false;
    const timer = setTimeout(async () => {
      try {
        const parsed = await window.veloce?.parseTicket(trimmed);
        if (!cancelled && parsed) {
          setPreview(parsed);
          setParseError(null);
        }
      } catch (err) {
        if (!cancelled) {
          setPreview(null);
          setParseError(err instanceof Error ? err.message : "Invalid session code");
        }
      }
    }, 150);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [ticketInput]);

  const handlePaste = async () => {
    try {
      const text = (await navigator.clipboard.readText())?.trim();
      if (text) {
        setTicketInput(text);
      }
    } catch {
      // Ignore clipboard permission errors.
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!ticketInput.trim() || !preview) return;
    const ok = await onJoinSession(ticketInput.trim());
    if (ok) {
      setTicketInput("");
      setPreview(null);
    }
  };

  return (
    <div className="view-stack">
      <form className="panel receive-panel" onSubmit={handleSubmit}>
        <div className="panel-header">
          <h3 className="panel-title-inline">
            <Globe size={16} className="accent-icon" />
            <span>Receive Globally (From Anywhere)</span>
          </h3>
          <div className="panel-header-actions">
            {ticketInput.trim().length > 0 && (
              <button
                className="btn-secondary btn-sm"
                type="button"
                onClick={() => setTicketInput("")}
              >
                <X size={13} />
                <span>Clear</span>
              </button>
            )}
            <button
              className="btn-secondary btn-sm"
              type="button"
              onClick={handlePaste}
              title="Paste session code from clipboard"
            >
              <ClipboardPaste size={13} />
              <span>Paste Code</span>
            </button>
          </div>
        </div>

        <div className="how-it-works-grid receive-steps">
          <div className="step-chip">
            <span className="step-num">1</span>
            <div>
              <strong>Sender Generates Code</strong>
              <span className="muted-xs">Sender clicks Generate Global Code in Send</span>
            </div>
          </div>
          <div className="step-chip">
            <span className="step-num">2</span>
            <div>
              <strong>Paste veloce1… Code</strong>
              <span className="muted-xs">Verifies BLAKE3 Merkle root before downloading</span>
            </div>
          </div>
          <div className="step-chip">
            <span className="step-num">3</span>
            <div>
              <strong>Seamless Auto-Resume</strong>
              <span className="muted-xs">Skips any chunks already received earlier</span>
            </div>
          </div>
        </div>

        <label className="input-label" htmlFor="ticket-input">
          Session Code (<code>veloce1...</code>)
        </label>
        <textarea
          id="ticket-input"
          className="ticket-textarea"
          rows={3}
          placeholder="Paste veloce1... code from the sender here"
          value={ticketInput}
          onChange={(e) => setTicketInput(e.target.value)}
        />

        {parseError && <p className="inline-error">{parseError}</p>}

        {preview && (
          <div className="ticket-preview">
            <div className="preview-row">
              <span className="merkle-pill">
                <ShieldCheck size={13} />
                Verified Ticket
              </span>
              <strong>
                {preview.fileCount} item(s) • {formatBytes(preview.totalSize)}
              </strong>
            </div>
            <div className="preview-meta muted-xs">
              <span>Sender: {preview.senderShortId}</span>
              <span>•</span>
              <span>
                {preview.directAddrs.length} direct endpoint(s), {preview.relayHints.length} relay(s)
              </span>
            </div>
          </div>
        )}

        <div className="dir-picker-row">
          <span className="muted-xs truncate" title={outputDir}>
            Save to: {outputDir || "Not selected"}
          </span>
          <button className="btn-secondary btn-sm" type="button" onClick={onChangeOutputDir}>
            <FolderOpen size={14} />
            <span>Change Folder</span>
          </button>
        </div>

        <button
          className="btn-primary btn-full"
          type="submit"
          disabled={busy || !preview || !outputDir}
        >
          <Download size={16} />
          <span>{busy ? "Connecting..." : "Download & Verify Files"}</span>
        </button>
      </form>

      <div className="panel lan-listening-banner">
        <div className="pulse-dot" />
        <Radio size={18} />
        <div>
          <strong>
            Ready on Nearby Wi-Fi / LAN{deviceName ? ` as "${deviceName}"` : ""}
          </strong>
          <p className="muted-xs">
            No code needed on the same Wi-Fi — select this device on the sender&apos;s Nearby list.
            Works in the background from your system tray.
          </p>
        </div>
      </div>
    </div>
  );
}
