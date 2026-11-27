import {
  CheckCircle2,
  ClipboardPaste,
  Download,
  FolderOpen,
  Globe,
  Wifi,
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
      } catch {
        if (!cancelled) {
          setPreview(null);
          setParseError("That share code looks incomplete. Copy the full veloce1… code and try again.");
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
      <div className="panel lan-listening-banner">
        <span className="pulse-dot" />
        <Wifi size={18} className="accent-icon" />
        <div className="lan-banner-body">
          <strong>
            Visible on Wi-Fi{deviceName ? ` as "${deviceName}"` : ""}
          </strong>
          <p className="muted-xs">
            Devices on the same Wi-Fi can send to you directly — no code needed.
          </p>
        </div>
        <button className="btn-secondary btn-sm" type="button" onClick={onChangeOutputDir}>
          <FolderOpen size={14} />
          <span>Save Folder</span>
        </button>
      </div>

      <form className="panel receive-panel" onSubmit={handleSubmit}>
        <div className="panel-header">
          <h3 className="panel-title-inline">
            <Globe size={16} className="accent-icon" />
            <span>Receive with a Share Code</span>
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
              title="Paste share code from clipboard"
            >
              <ClipboardPaste size={13} />
              <span>Paste Code</span>
            </button>
          </div>
        </div>

        <p className="muted-sm receive-subtitle">
          Receiving from someone on another network? Ask them to click{" "}
          <strong>Create Share Code</strong> in Send, then paste their <code>veloce1…</code> code
          here. Interrupted downloads automatically resume where they left off.
        </p>

        <textarea
          id="ticket-input"
          className="ticket-textarea"
          rows={2}
          aria-label="Share code"
          placeholder="Paste veloce1… share code here"
          value={ticketInput}
          onChange={(e) => setTicketInput(e.target.value)}
        />

        {parseError && <p className="inline-error">{parseError}</p>}

        {preview && (
          <div className="ticket-preview">
            <div className="preview-row">
              <span className="merkle-pill">
                <CheckCircle2 size={13} />
                Ready to Download
              </span>
              <strong>
                {preview.fileCount} {preview.fileCount === 1 ? "file" : "files"} •{" "}
                {formatBytes(preview.totalSize)}
              </strong>
            </div>
          </div>
        )}

        <div className="dir-picker-row">
          <span className="muted-xs truncate" title={outputDir}>
            Saving to: {outputDir || "Not selected"}
          </span>
          <button className="btn-secondary btn-sm" type="button" onClick={onChangeOutputDir}>
            <FolderOpen size={14} />
            <span>Change</span>
          </button>
        </div>

        <button
          className="btn-primary btn-full"
          type="submit"
          disabled={busy || !preview || !outputDir}
        >
          <Download size={16} />
          <span>{busy ? "Connecting..." : "Download Files"}</span>
        </button>
      </form>
    </div>
  );
}
