import { app, BrowserWindow, dialog, ipcMain } from "electron";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

interface NativeClient {
  nodeInfoJson(): string;
  onEvent(cb: (eventJson: string) => void): void;
  startSession(
    paths: string[],
    targetPeerNodeId?: string | null,
    relayHints?: string[] | null,
  ): Promise<string>;
  joinSession(
    ticketCode: string,
    outputDir: string,
    parallelStreams?: number | null,
  ): Promise<string>;
  listLocalPeers(): string;
  acceptIncomingTransfer(transferId: string, outputDir: string): void;
  rejectIncomingTransfer(transferId: string, reason?: string | null): void;
  pauseTransfer(transferId: string): boolean;
  resumeTransfer(transferId: string): Promise<boolean>;
  cancelTransfer(transferId: string): boolean;
  shutdown(): void;
}

interface NativeAddon {
  VeloceNativeClient: {
    create(config: {
      deviceName: string;
      bindAddr?: string;
      enableMdns?: boolean;
      parallelStreams?: number;
      relayAddrs?: string[];
      forceRelay?: boolean;
    }): Promise<NativeClient>;
  };
  parseTicket(ticketCode: string): string;
}

let mainWindow: BrowserWindow | null = null;
let nativeAddon: NativeAddon | null = null;
let client: NativeClient | null = null;
let initError: string | null = null;

function resolveNativeAddonPath(): string {
  const candidates = [
    path.join(__dirname, "..", "native", "veloce_core.node"),
    path.join(process.resourcesPath || "", "native", "veloce_core.node"),
  ];
  for (const candidate of candidates) {
    if (candidate && fs.existsSync(candidate)) {
      return candidate;
    }
  }
  return candidates[0];
}

async function initNativeEngine(): Promise<void> {
  const addonPath = resolveNativeAddonPath();
  if (!fs.existsSync(addonPath)) {
    initError = `Native Rust addon not found at ${addonPath}. Run "npm run build:native" in /ui first.`;
    return;
  }

  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const loaded = require(addonPath) as NativeAddon;
    nativeAddon = loaded;

    const relayEnv = (process.env.VELOCE_RELAY_ADDRS || "")
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);

    const created = await loaded.VeloceNativeClient.create({
      deviceName: os.hostname() || "Veloce Desktop",
      enableMdns: true,
      parallelStreams: 8,
      relayAddrs: relayEnv.length > 0 ? relayEnv : undefined,
    });

    created.onEvent((eventJson: string) => {
      if (mainWindow && !mainWindow.isDestroyed()) {
        try {
          mainWindow.webContents.send("veloce:event", JSON.parse(eventJson));
        } catch {
          // Ignore malformed event payloads.
        }
      }
    });

    client = created;
  } catch (err) {
    initError = err instanceof Error ? err.message : String(err);
  }
}

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1080,
    height: 760,
    minWidth: 820,
    minHeight: 620,
    title: "Veloce — P2P File Transfer",
    backgroundColor: "#0c0f14",
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });

  const devUrl = process.env.VITE_DEV_SERVER_URL;
  if (devUrl) {
    void mainWindow.loadURL(devUrl);
  } else {
    void mainWindow.loadFile(path.join(__dirname, "..", "dist", "index.html"));
  }

  mainWindow.on("closed", () => {
    mainWindow = null;
  });
}

function registerIpcHandlers(): void {
  ipcMain.handle("veloce:getStatus", () => {
    if (!client) {
      return { ready: false, error: initError ?? "Engine initializing", nodeInfo: null };
    }
    return {
      ready: true,
      error: null,
      nodeInfo: JSON.parse(client.nodeInfoJson()),
      defaultOutputDir: path.join(os.homedir(), "Downloads", "Veloce"),
    };
  });

  ipcMain.handle("veloce:pickFiles", async () => {
    if (!mainWindow) return [];
    const res = await dialog.showOpenDialog(mainWindow, {
      title: "Select files to send",
      properties: ["openFile", "multiSelections"],
    });
    return res.canceled ? [] : res.filePaths;
  });

  ipcMain.handle("veloce:pickFolders", async () => {
    if (!mainWindow) return [];
    const res = await dialog.showOpenDialog(mainWindow, {
      title: "Select folders to send",
      properties: ["openDirectory", "multiSelections"],
    });
    return res.canceled ? [] : res.filePaths;
  });

  ipcMain.handle("veloce:pickOutputDir", async () => {
    if (!mainWindow) return null;
    const res = await dialog.showOpenDialog(mainWindow, {
      title: "Choose destination folder",
      properties: ["openDirectory", "createDirectory"],
    });
    return res.canceled ? null : res.filePaths[0] ?? null;
  });

  ipcMain.handle(
    "veloce:startSession",
    async (
      _event,
      args: { paths: string[]; targetPeerNodeId?: string; relayHints?: string[] },
    ) => {
      if (!client) throw new Error(initError ?? "Engine not ready");
      const json = await client.startSession(
        args.paths,
        args.targetPeerNodeId ?? null,
        args.relayHints ?? null,
      );
      return JSON.parse(json);
    },
  );

  ipcMain.handle(
    "veloce:joinSession",
    async (
      _event,
      args: { ticketCode: string; outputDir: string; parallelStreams?: number },
    ) => {
      if (!client) throw new Error(initError ?? "Engine not ready");
      const json = await client.joinSession(
        args.ticketCode,
        args.outputDir,
        args.parallelStreams ?? 8,
      );
      return JSON.parse(json);
    },
  );

  ipcMain.handle("veloce:parseTicket", (_event, ticketCode: string) => {
    if (!nativeAddon) throw new Error(initError ?? "Native addon not loaded");
    return JSON.parse(nativeAddon.parseTicket(ticketCode));
  });

  ipcMain.handle("veloce:listLocalPeers", () => {
    if (!client) return [];
    return JSON.parse(client.listLocalPeers());
  });

  ipcMain.handle(
    "veloce:acceptIncoming",
    (_event, args: { transferId: string; outputDir: string }) => {
      if (!client) throw new Error(initError ?? "Engine not ready");
      client.acceptIncomingTransfer(args.transferId, args.outputDir);
      return { ok: true };
    },
  );

  ipcMain.handle(
    "veloce:rejectIncoming",
    (_event, args: { transferId: string; reason?: string }) => {
      if (!client) throw new Error(initError ?? "Engine not ready");
      client.rejectIncomingTransfer(args.transferId, args.reason ?? null);
      return { ok: true };
    },
  );

  ipcMain.handle("veloce:pauseTransfer", (_event, transferId: string) => {
    if (!client) return false;
    return client.pauseTransfer(transferId);
  });

  ipcMain.handle("veloce:resumeTransfer", async (_event, transferId: string) => {
    if (!client) return false;
    return client.resumeTransfer(transferId);
  });

  ipcMain.handle("veloce:cancelTransfer", (_event, transferId: string) => {
    if (!client) return false;
    return client.cancelTransfer(transferId);
  });
}

app.whenReady().then(async () => {
  await initNativeEngine();
  registerIpcHandlers();
  createWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on("window-all-closed", () => {
  if (client) {
    client.shutdown();
    client = null;
  }
  if (process.platform !== "darwin") {
    app.quit();
  }
});
