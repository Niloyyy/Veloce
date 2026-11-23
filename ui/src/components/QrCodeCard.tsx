import { Check, Copy, QrCode, ShieldCheck } from "lucide-react";
import QRCode from "qrcode";
import { useEffect, useState } from "react";
import type { StartSessionResult } from "../types/veloce";
import { formatBytes } from "../utils/format";

interface QrCodeCardProps {
  session: StartSessionResult;
}

export function QrCodeCard({ session }: QrCodeCardProps) {
  const [qrDataUrl, setQrDataUrl] = useState<string>("");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let active = true;
    QRCode.toDataURL(session.qrUri, {
      width: 200,
      margin: 1,
      color: {
        dark: "#0f172a",
        light: "#ffffff",
      },
    })
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
        <span className="merkle-pill">
          <ShieldCheck size={13} />
          BLAKE3 Verified ({session.totalChunks} chunks)
        </span>
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
            Share this ephemeral code or scan the QR code from anywhere in the world. Veloce tries
            LAN first, punches UDP NATs directly, or falls back to an end-to-end encrypted relay.
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
        </div>
      </div>
    </div>
  );
}
