import { Download, FolderOpen, Radio, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import type { TicketPreview } from "../types/veloce";
import { formatBytes } from "../utils/format";

interface ReceiveViewProps {
  outputDir: string;
  busy: boolean;
  onChangeOutputDir: () => void;
  onJoinSession: (ticketCode: string) => Promise<boolean>;
}

export function ReceiveView({
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
    }, 180);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [ticketInput]);

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
          <h3>Join Global Session</h3>
          <span className="muted-xs">Resumable • Out-of-order parallel QUIC streams</span>
        </div>

        <label className="input-label" htmlFor="ticket-input">
          Paste Session Code (<code>veloce1...</code>)
        </label>
        <textarea
          id="ticket-input"
          className="ticket-textarea"
          rows={3}
          placeholder="veloce1..."
          value={ticketInput}
          onChange={(e) => setTicketInput(e.target.value)}
        />

        {parseError && <p className="inline-error">{parseError}</p>}

        {preview && (
          <div className="ticket-preview">
            <div className="preview-row">
              <span className="merkle-pill">
                <ShieldCheck size={13} />
                Valid Ticket
              </span>
              <strong>
                {preview.fileCount} file(s) • {formatBytes(preview.totalSize)}
              </strong>
            </div>
            <div className="preview-meta muted-xs">
              <span>Sender: {preview.senderShortId}</span>
              <span>•</span>
              <span>
                {preview.directAddrs.length} direct hint(s), {preview.relayHints.length} relay(s)
              </span>
            </div>
          </div>
        )}

        <div className="dir-picker-row">
          <span className="muted-xs truncate">Destination: {outputDir || "Not selected"}</span>
          <button className="btn-secondary btn-sm" type="button" onClick={onChangeOutputDir}>
            <FolderOpen size={14} />
            <span>Choose Folder</span>
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
          <strong>Listening for Incoming LAN Transfers</strong>
          <p className="muted-xs">
            Nearby devices on your network can discover this machine automatically. Incoming offers
            require your explicit approval before any data is written.
          </p>
        </div>
      </div>
    </div>
  );
}
