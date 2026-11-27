import { Check, Copy, QrCode, Square } from "lucide-react";
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
          width: 184,
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
          <QrCode size={16} className="accent-icon" />
          <h3>Share Code Ready</h3>
        </div>
        <div className="ticket-header-actions">
          <span className="muted-xs">
            {session.fileCount} {session.fileCount === 1 ? "file" : "files"} •{" "}
            {formatBytes(session.totalSize)}
          </span>
          <button
            className="btn-secondary btn-sm"
            type="button"
            title="Stop sharing this code"
            onClick={onStopSharing}
          >
            <Square size={12} />
            <span>Stop Sharing</span>
          </button>
        </div>
      </div>

      <div className="ticket-body">
        <div className="qr-frame">
          {qrDataUrl ? (
            <img src={qrDataUrl} alt="Share QR Code" width={144} height={144} />
          ) : (
            <div className="qr-placeholder">Generating QR...</div>
          )}
        </div>

        <div className="ticket-meta">
          <p className="muted-sm">
            Copy this code and send it to the recipient (or scan the QR code in the Veloce mobile
            app). Keep Veloce open or in your system tray until the download finishes.
          </p>
          <div className="ticket-code-box">
            <code className="ticket-code-text">{session.ticketCode}</code>
            <button className="btn-primary btn-sm" onClick={handleCopy} type="button">
              {copied ? <Check size={14} /> : <Copy size={14} />}
              <span>{copied ? "Copied!" : "Copy Code"}</span>
            </button>
          </div>
          {!hasRelay && (
            <span className="field-hint">
              Works directly across most home &amp; mobile networks. If a strict mobile carrier
              blocks direct connections, add a Relay Server under Settings → Advanced.
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
