import { FolderOpen, MessageSquareText, ShieldCheck } from "lucide-react";
import { useState } from "react";
import type { IncomingOfferEvent } from "../types/veloce";
import { formatBytes } from "../utils/format";
import { isTextClipPath } from "../utils/textClip";
import { ConnectionBadge } from "./ConnectionBadge";

interface IncomingOfferModalProps {
  offer: IncomingOfferEvent;
  outputDir: string;
  onChangeOutputDir: () => void;
  onAccept: (trustSender: boolean) => void;
  onReject: () => void;
}

export function IncomingOfferModal({
  offer,
  outputDir,
  onChangeOutputDir,
  onAccept,
  onReject,
}: IncomingOfferModalProps) {
  const [trustSender, setTrustSender] = useState(false);
  const isTextClip =
    offer.fileCount === 1 &&
    offer.files.length === 1 &&
    isTextClipPath(offer.files[0].relativePath);

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true">
      <div className="modal-card">
        <div className="modal-header">
          <div className="modal-title-stack">
            <span className="eyebrow">
              {isTextClip ? "Incoming Text / Link" : "Incoming Files"}
            </span>
            <h2>{offer.senderName}</h2>
          </div>
          <ConnectionBadge type={offer.connectionType} />
        </div>

        <div className="offer-summary">
          {isTextClip ? (
            <span className="panel-title-inline">
              <MessageSquareText size={15} />
              <strong>Text / Link</strong> • {formatBytes(offer.totalSize)}
            </span>
          ) : (
            <span>
              <strong>
                {offer.fileCount} {offer.fileCount === 1 ? "file" : "files"}
              </strong>{" "}
              • <strong>{formatBytes(offer.totalSize)}</strong>
            </span>
          )}
          <span className="merkle-pill">
            <ShieldCheck size={13} />
            Encrypted
          </span>
        </div>

        {!isTextClip && (
          <>
            <ul className="offer-file-list">
              {offer.files.slice(0, 6).map((f) => (
                <li key={f.relativePath}>
                  <span className="truncate">{f.relativePath}</span>
                  <span className="muted-xs">{formatBytes(f.size)}</span>
                </li>
              ))}
              {offer.files.length > 6 && (
                <li className="muted-xs">+ {offer.files.length - 6} more</li>
              )}
            </ul>

            <div className="dir-picker-row">
              <span className="muted-xs truncate">Save to: {outputDir}</span>
              <button className="btn-secondary btn-sm" type="button" onClick={onChangeOutputDir}>
                <FolderOpen size={14} />
                <span>Change</span>
              </button>
            </div>
          </>
        )}

        <label className="trust-checkbox-row">
          <input
            type="checkbox"
            checked={trustSender}
            onChange={(e) => setTrustSender(e.target.checked)}
          />
          <span>
            Always accept files automatically from <strong>{offer.senderName}</strong>
          </span>
        </label>

        <div className="modal-actions">
          <button className="btn-secondary" type="button" onClick={onReject}>
            Decline
          </button>
          <button className="btn-primary" type="button" onClick={() => onAccept(trustSender)}>
            {isTextClip ? "Receive Text" : "Accept"}
          </button>
        </div>
      </div>
    </div>
  );
}
