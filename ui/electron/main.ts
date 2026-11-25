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

export type DesktopThemeId = "dark" | "light" | "midnight" | "nord" | "emerald" | "sunset";

const VALID_THEMES: ReadonlySet<string> = new Set([
  "dark",
  "light",
  "midnight",
  "nord",
  "emerald",
  "sunset",
]);

const THEME_BG_COLORS: Record<DesktopThemeId, string> = {
  dark: "#0c0f14",
  light: "#f4f6fa",
  midnight: "#000000",
  nord: "#1b222c",
  emerald: "#09130f",
  sunset: "#140e12",
};

export interface DesktopSettings {
  deviceName: string;
  downloadDir: string;
  relayAddrs: string[];
  parallelStreams: number;
  theme: DesktopThemeId;
}

let mainWindow: BrowserWindow | null = null;
let nativeAddon: NativeAddon | null = null;
let client: NativeClient | null = null;
let initError: string | null = null;
let currentSettings: DesktopSettings = defaultSettings();

function defaultSettings(): DesktopSettings {
  const envRelays = (process.env.VELOCE_RELAY_ADDRS || "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);

  return {
    deviceName: os.hostname() || "Veloce Desktop",
    downloadDir: path.join(os.homedir(), "Downloads", "Veloce"),
    relayAddrs: envRelays,
    parallelStreams: 8,
    theme: "dark",
  };
}

function settingsFilePath(): string {
  return path.join(app.getPath("userData"), "veloce-settings.json");
}

function sanitizeSettings(raw: Partial<DesktopSettings>, fallback: DesktopSettings): DesktopSettings {
  const deviceName =
    typeof raw.deviceName === "string" && raw.deviceName.trim().length > 0
      ? raw.deviceName.trim().slice(0, 64)
      : fallback.deviceName;
  const downloadDir =
    typeof raw.downloadDir === "string" && raw.downloadDir.trim().length > 0
      ? raw.downloadDir.trim()
      : fallback.downloadDir;
  const relayAddrs = Array.isArray(raw.relayAddrs)
    ? raw.relayAddrs
        .filter((s): s is string => typeof s === "string")
        .map((s) => s.trim())
        .filter(Boolean)
    : fallback.relayAddrs;
  const rawStreams =
    typeof raw.parallelStreams === "number" && Number.isFinite(raw.parallelStreams)
      ? Math.round(raw.parallelStreams)
      : fallback.parallelStreams;
  const parallelStreams = Math.min(32, Math.max(1, rawStreams));
  const theme: DesktopThemeId =
    typeof raw.theme === "string" && VALID_THEMES.has(raw.theme)
      ? (raw.theme as DesktopThemeId)
      : fallback.theme;

  return {
    deviceName,
    downloadDir,
    relayAddrs,
    parallelStreams,
    theme,
  };
}

function loadPersistedSettings(): DesktopSettings {
  const fallback = defaultSettings();
  try {
    const file = settingsFilePath();
    if (!fs.existsSync(file)) {
      return fallback;
    }
    const parsed = JSON.parse(fs.readFileSync(file, "utf8")) as Partial<DesktopSettings>;
    return sanitizeSettings(parsed, fallback);
  } catch {
    return fallback;
  }
}

function savePersistedSettings(next: DesktopSettings): void {
  const file = settingsFilePath();
  const dir = path.dirname(file);
  fs.mkdirSync(dir, { recursive: true });
  const tmp = `${file}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(next, null, 2), "utf8");
  fs.renameSync(tmp, file);
}

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

function buildStatusPayload() {
  if (!client) {
    return {
      ready: false,
      error: initError ?? "Engine initializing",
      nodeInfo: null,
      defaultOutputDir: currentSettings.downloadDir,
      settings: currentSettings,
    };
  }
  return {
    ready: true,
    error: null,
    nodeInfo: JSON.parse(client.nodeInfoJson()),
    defaultOutputDir: currentSettings.downloadDir,
    settings: currentSettings,
  };
}

async function initNativeEngine(): Promise<void> {
  if (client) {
    try {
      client.shutdown();
    } catch {
      // Ignore shutdown errors during restart.
    }
    client = null;
  }
  initError = null;

  const addonPath = resolveNativeAddonPath();
  if (!fs.existsSync(addonPath)) {
    initError = `Native Rust addon not found at ${addonPath}. Run "npm run build:native" in /ui first.`;
    return;
  }

  try {
    if (!nativeAddon) {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      nativeAddon = require(addonPath) as NativeAddon;
    }

    const created = await nativeAddon.VeloceNativeClient.create({
      deviceName: currentSettings.deviceName,
      enableMdns: true,
      parallelStreams: currentSettings.parallelStreams,
      relayAddrs:
        currentSettings.relayAddrs.length > 0 ? currentSettings.relayAddrs : undefined,
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

function resolveAppIconPath(): string | undefined {
  const candidates = [
    path.join(__dirname, "..", "build", "icon.png"),
    path.join(process.resourcesPath || "", "build", "icon.png"),
  ];
  for (const candidate of candidates) {
    if (candidate && fs.existsSync(candidate)) {
      return candidate;
    }
  }
  return undefined;
}

function createWindow(): void {
  const iconPath = resolveAppIconPath();
  mainWindow = new BrowserWindow({
    width: 1080,
    height: 760,
    minWidth: 820,
    minHeight: 620,
    title: "Veloce — P2P File Transfer",
    icon: iconPath,
    backgroundColor: THEME_BG_COLORS[currentSettings.theme] ?? "#0c0f14",
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
  ipcMain.handle("veloce:getStatus", () => buildStatusPayload());

  ipcMain.handle("veloce:getSettings", () => currentSettings);

  ipcMain.handle(
    "veloce:updateSettings",
    async (_event, partial: Partial<DesktopSettings>) => {
      const prev = currentSettings;
      const next = sanitizeSettings(partial, prev);
      const needsRestart =
        next.deviceName !== prev.deviceName ||
        next.parallelStreams !== prev.parallelStreams ||
        JSON.stringify(next.relayAddrs) !== JSON.stringify(prev.relayAddrs);

      currentSettings = next;
      try {
        savePersistedSettings(next);
      } catch (err) {
        throw new Error(
          `Failed to persist settings: ${err instanceof Error ? err.message : String(err)}`,
        );
      }

      if (needsRestart) {
        await initNativeEngine();
      }

      return {
        settings: currentSettings,
        status: buildStatusPayload(),
      };
    },
  );

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
      const relayHints =
        args.relayHints && args.relayHints.length > 0
          ? args.relayHints
          : currentSettings.relayAddrs.length > 0
            ? currentSettings.relayAddrs
            : null;
      const json = await client.startSession(
        args.paths,
        args.targetPeerNodeId ?? null,
        relayHints,
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
        args.outputDir || currentSettings.downloadDir,
        args.parallelStreams ?? currentSettings.parallelStreams,
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
      client.acceptIncomingTransfer(
        args.transferId,
        args.outputDir || currentSettings.downloadDir,
      );
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
  currentSettings = loadPersistedSettings();
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
