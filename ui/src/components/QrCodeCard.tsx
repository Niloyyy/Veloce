import { Check, Copy, QrCode, ShieldCheck, Square } from "lucide-react";
import { useEffect, useState } from "react";
import type { StartSessionResult } from "../types/veloce";
import { formatBytes } from "../utils/format";

interface QrCodeCardProps {
  session: StartSessionResult;
  hasRelay: boolean;
  onStopSharing: () => void;
}

export function QrCodeCard({ session, hasRelay, onStopSharing }: QrCodeCardProps) {
  const [qrDataUrl, setQrDataUrl] = useState<string>("");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let active = true;
    import("qrcode")
      .then((mod) =>
        mod.toDataURL(session.qrUri, {
          width: 200,
          margin: 1,
          color: {
            dark: "#0f172a",
            light: "#ffffff",
          },
        }),
      )
      .then((url) => {
        if (active) setQrDataUrl(url);
      })
      .catch(() => {
        if (active) setQrDataUrl("");
      });
    return () => {
      active = false;
    };
  }, [session.qrUri]);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(session.ticketCode);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      // Clipboard API unavailable.
    }
  };

  return (
    <div className="ticket-card">
      <div className="ticket-header">
        <div className="ticket-title-row">
          <QrCode size={16} />
          <h3>Global Session Ticket</h3>
        </div>
        <div className="ticket-header-actions">
          <span className="merkle-pill">
            <ShieldCheck size={13} />
            BLAKE3 ({session.totalChunks} chunks)
          </span>
          <button
            className="btn-secondary btn-sm"
            type="button"
            title="Stop hosting this ticket"
            onClick={onStopSharing}
          >
            <Square size={12} />
            <span>Stop</span>
          </button>
        </div>
      </div>

      <div className="ticket-body">
        <div className="qr-frame">
          {qrDataUrl ? (
            <img src={qrDataUrl} alt="Global Session QR Code" width={164} height={164} />
          ) : (
            <div className="qr-placeholder">Generating QR...</div>
          )}
        </div>

        <div className="ticket-meta">
          <p className="muted-sm">
            Share this ephemeral code or scan the QR code on Veloce Mobile. Veloce connects over
            LAN, STUN Direct P2P, or your configured end-to-end encrypted relay.
          </p>
          <div className="ticket-stats">
            <span>{session.fileCount} file(s)</span>
            <span>•</span>
            <span>{formatBytes(session.totalSize)}</span>
            <span>•</span>
            <span>{formatBytes(session.chunkSize)} chunks</span>
          </div>
          <div className="ticket-code-box">
            <code className="ticket-code-text">{session.ticketCode}</code>
            <button className="btn-secondary btn-sm" onClick={handleCopy} type="button">
              {copied ? <Check size={14} /> : <Copy size={14} />}
              <span>{copied ? "Copied" : "Copy"}</span>
            </button>
          </div>
          {!hasRelay && (
            <span className="field-hint">
              Tip: Sending across cellular CGNAT or symmetric NAT? Add a{" "}
              <code>veloce-relay</code> host:port in Settings.
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
