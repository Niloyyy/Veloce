export type ConnectionType = "LAN" | "Direct-P2P" | "Relayed";
export type ThemeId = "dark" | "light" | "midnight" | "nord" | "emerald" | "sunset";

export interface NodeInfo {
  nodeId: string;
  shortId: string;
  deviceName: string;
  listenAddr: string;
  listenPort: number;
}

export interface DesktopSettings {
  deviceName: string;
  downloadDir: string;
  relayAddrs: string[];
  parallelStreams: number;
  theme: ThemeId;
}

export interface EngineStatus {
  ready: boolean;
  error: string | null;
  nodeInfo: NodeInfo | null;
  defaultOutputDir?: string;
  detectedDeviceName?: string;
  settings?: DesktopSettings;
}

export interface UpdateSettingsResult {
  settings: DesktopSettings;
  status: EngineStatus;
}

export interface PeerDto {
  nodeId: string;
  shortId: string;
  deviceName: string;
  addresses: string[];
  port: number;
}

export interface FileSummary {
  relativePath: string;
  size: number;
}

export interface StartSessionResult {
  transferId: string;
  ticketCode: string;
  qrUri: string;
  totalSize: number;
  totalChunks: number;
  chunkSize: number;
  fileCount: number;
  files: FileSummary[];
}

export interface TicketPreview {
  transferId: string;
  senderNodeId: string;
  senderShortId: string;
  totalSize: number;
  fileCount: number;
  directAddrs: string[];
  relayHints: string[];
  merkleRootHex: string;
}

export interface JoinSessionResult {
  transferId: string;
  totalSize: number;
  fileCount: number;
}

export interface IncomingOfferEvent {
  type: "IncomingOffer";
  transferId: string;
  senderNodeId: string;
  senderShortId: string;
  senderName: string;
  fileCount: number;
  totalSize: number;
  files: FileSummary[];
  connectionType: ConnectionType;
}

export type EngineEvent =
  | {
      type: "TransferProgress";
      transferId: string;
      direction: "send" | "receive";
      bytesTransferred: number;
      totalBytes: number;
      chunksVerified: number;
      totalChunks: number;
      speedBytesPerSec: number;
      etaSeconds: number;
      elapsedMs: number;
      connectionType: ConnectionType;
    }
  | {
      type: "PeerDiscovered";
      peer: PeerDto;
    }
  | {
      type: "PeerLost";
      nodeId: string;
    }
  | IncomingOfferEvent
  | {
      type: "ConnectionTypeChanged";
      transferId: string;
      peerNodeId: string;
      connectionType: ConnectionType;
    }
  | {
      type: "TransferPaused";
      transferId: string;
      direction: "send" | "receive";
    }
  | {
      type: "TransferResumed";
      transferId: string;
      direction: "send" | "receive";
    }
  | {
      type: "TransferCompleted";
      transferId: string;
      direction: "send" | "receive";
      totalBytes: number;
      totalChunks: number;
      elapsedMs: number;
      averageSpeedBytesPerSec: number;
      outputPaths: string[];
    }
  | {
      type: "TransferFailed";
      transferId: string;
      direction: "send" | "receive";
      error: string;
    };

export interface TransferRecord {
  transferId: string;
  label: string;
  direction: "send" | "receive";
  status: "active" | "paused" | "completed" | "failed" | "cancelled";
  bytesTransferred: number;
  totalBytes: number;
  chunksVerified: number;
  totalChunks: number;
  speedBytesPerSec: number;
  speedHistory: number[];
  etaSeconds: number;
  elapsedMs?: number;
  connectionType: ConnectionType;
  error?: string;
  outputPaths?: string[];
}

export interface VeloceBridgeApi {
  getStatus: () => Promise<EngineStatus>;
  getSettings: () => Promise<DesktopSettings>;
  updateSettings: (
    settings: Partial<DesktopSettings>,
  ) => Promise<UpdateSettingsResult>;
  pickFiles: () => Promise<string[]>;
  pickFolders: () => Promise<string[]>;
  pickOutputDir: () => Promise<string | null>;
  getPathForFile: (file: File) => string;
  startSession: (args: {
    paths: string[];
    targetPeerNodeId?: string;
    relayHints?: string[];
  }) => Promise<StartSessionResult>;
  joinSession: (args: {
    ticketCode: string;
    outputDir: string;
    parallelStreams?: number;
  }) => Promise<JoinSessionResult>;
  parseTicket: (ticketCode: string) => Promise<TicketPreview>;
  listLocalPeers: () => Promise<PeerDto[]>;
  acceptIncoming: (args: {
    transferId: string;
    outputDir: string;
  }) => Promise<{ ok: boolean }>;
  rejectIncoming: (args: {
    transferId: string;
    reason?: string;
  }) => Promise<{ ok: boolean }>;
  pauseTransfer: (transferId: string) => Promise<boolean>;
  resumeTransfer: (transferId: string) => Promise<boolean>;
  cancelTransfer: (transferId: string) => Promise<boolean>;
  onEvent: (listener: (event: EngineEvent) => void) => () => void;
}

declare global {
  interface Window {
    veloce?: VeloceBridgeApi;
  }
}
