import {
  ArrowDownLeft,
  ArrowUpRight,
  CheckCircle2,
  Clock,
  Pause,
  Play,
  RotateCcw,
  ShieldCheck,
  Trash2,
  X,
  XCircle,
} from "lucide-react";
import type { TransferRecord } from "../types/veloce";
import { formatBytes, formatDurationMs, formatEta, formatSpeed } from "../utils/format";
import { ConnectionBadge } from "./ConnectionBadge";

interface TransferCardProps {
  transfer: TransferRecord;
  onPause: (transferId: string) => void;
  onResume: (transferId: string) => void;
  onCancel: (transferId: string) => void;
  onRemove: (transferId: string) => void;
}

function SpeedSparkline({ samples }: { samples: number[] }) {
  if (samples.length < 2) return null;
  const max = Math.max(...samples, 1);
  const w = 120;
  const h = 28;
  const points = samples
    .map((val, idx) => {
      const x = (idx / (samples.length - 1)) * w;
      const y = h - (val / max) * (h - 4) - 2;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");

  return (
    <svg className="sparkline" width={w} height={h} viewBox={`0 0 ${w} ${h}`} aria-hidden="true">
      <polyline fill="none" stroke="currentColor" strokeWidth="2" points={points} />
    </svg>
  );
}

export function TransferCard({
  transfer,
  onPause,
  onResume,
  onCancel,
  onRemove,
}: TransferCardProps) {
  const pct =
    transfer.totalBytes > 0
      ? Math.min(100, (transfer.bytesTransferred / transfer.totalBytes) * 100)
      : transfer.status === "completed"
        ? 100
        : 0;

  return (
    <div className={`transfer-card status-${transfer.status}`}>
      <div className="transfer-row-top">
        <div className="transfer-identity">
          <span className={`dir-icon dir-${transfer.direction}`}>
            {transfer.direction === "send" ? (
              <ArrowUpRight size={15} />
            ) : (
              <ArrowDownLeft size={15} />
            )}
          </span>
          <div className="transfer-titles">
            <strong className="transfer-label">{transfer.label}</strong>
            <span className="muted-xs">
              {formatBytes(transfer.bytesTransferred)} / {formatBytes(transfer.totalBytes)}
              {transfer.status === "paused" ? " • Paused (resume state saved)" : ""}
              {transfer.status === "cancelled" ? " • Cancelled" : ""}
            </span>
          </div>
        </div>

        <div className="transfer-actions">
          <ConnectionBadge type={transfer.connectionType} />
          {transfer.status === "active" && (
            <>
              <button
                className="icon-btn"
                title="Pause transfer"
                type="button"
                onClick={() => onPause(transfer.transferId)}
              >
                <Pause size={14} />
              </button>
              <button
                className="icon-btn"
                title="Cancel transfer"
                type="button"
                onClick={() => onCancel(transfer.transferId)}
              >
                <X size={15} />
              </button>
            </>
          )}
          {transfer.status === "paused" && (
            <>
              <button
                className="icon-btn icon-btn-resume"
                title="Resume transfer from verified chunks"
                type="button"
                onClick={() => onResume(transfer.transferId)}
              >
                <Play size={14} />
              </button>
              <button
                className="icon-btn"
                title="Cancel transfer"
                type="button"
                onClick={() => onCancel(transfer.transferId)}
              >
                <X size={15} />
              </button>
            </>
          )}
          {transfer.status === "completed" && (
            <>
              <span className="status-icon ok" title="Completed and Merkle verified">
                <CheckCircle2 size={17} />
              </span>
              <button
                className="icon-btn"
                title="Remove from history"
                type="button"
                onClick={() => onRemove(transfer.transferId)}
              >
                <Trash2 size={14} />
              </button>
            </>
          )}
          {(transfer.status === "failed" || transfer.status === "cancelled") && (
            <>
              <span className="status-icon err" title={transfer.error ?? "Cancelled"}>
                <XCircle size={17} />
              </span>
              {transfer.status === "failed" && transfer.direction === "receive" && (
                <button
                  className="icon-btn icon-btn-resume"
                  title="Retry transfer"
                  type="button"
                  onClick={() => onResume(transfer.transferId)}
                >
                  <RotateCcw size={14} />
                </button>
              )}
              <button
                className="icon-btn"
                title="Remove from history"
                type="button"
                onClick={() => onRemove(transfer.transferId)}
              >
                <Trash2 size={14} />
              </button>
            </>
          )}
        </div>
      </div>

      <div className="progress-track">
        <div
          className={`progress-fill ${transfer.status === "paused" ? "progress-paused" : ""}`}
          style={{ width: `${pct.toFixed(1)}%` }}
        />
      </div>

      <div className="transfer-telemetry">
        <div className="telemetry-metrics">
          <span>
            <strong>
              {transfer.status === "paused"
                ? "Paused"
                : transfer.status === "cancelled"
                  ? "Cancelled"
                  : formatSpeed(transfer.speedBytesPerSec)}
            </strong>
          </span>
          {transfer.status === "completed" ? (
            <span className="duration-chip" title="Time taken from when chunks started transferring">
              <Clock size={13} />
              {transfer.elapsedMs && transfer.elapsedMs > 0
                ? `Took ${formatDurationMs(transfer.elapsedMs)}`
                : "Done"}
            </span>
          ) : (
            <>
              {transfer.elapsedMs && transfer.elapsedMs > 0 ? (
                <span className="duration-chip" title="Elapsed chunk transfer time">
                  <Clock size={13} />
                  {formatDurationMs(transfer.elapsedMs)}
                </span>
              ) : null}
              <span>
                ETA:{" "}
                {transfer.status === "paused" || transfer.status === "cancelled"
                  ? "—"
                  : formatEta(transfer.etaSeconds)}
              </span>
            </>
          )}
          <span className="chunk-status">
            <ShieldCheck size={13} />
            {transfer.chunksVerified} / {transfer.totalChunks} chunks verified
          </span>
        </div>
        <SpeedSparkline samples={transfer.speedHistory} />
      </div>

      {transfer.error && <p className="transfer-error">{transfer.error}</p>}
    </div>
  );
}
