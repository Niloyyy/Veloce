import { app, BrowserWindow, dialog, ipcMain, shell } from "electron";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { detectHardwareModelName } from "./deviceName";

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
      identityKeyPath?: string;
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

export interface TrustedPeer {
  nodeId: string;
  shortId: string;
  deviceName: string;
  addedAt: number;
}

export interface DesktopSettings {
  deviceName: string;
  downloadDir: string;
  relayAddrs: string[];
  parallelStreams: number;
  theme: DesktopThemeId;
  trustedPeers: TrustedPeer[];
}

const TEXT_CLIP_FILENAME = "veloce-clip-message.txt";
const MAX_TEXT_CLIP_BYTES = 256 * 1024;

interface PersistedDesktopSettings extends DesktopSettings {
  deviceNameCustom?: boolean;
}

const DETECTED_DEVICE_NAME = detectHardwareModelName();

let mainWindow: BrowserWindow | null = null;
let nativeAddon: NativeAddon | null = null;
let client: NativeClient | null = null;
let initError: string | null = null;
let currentSettings: DesktopSettings = defaultSettings();
let deviceNameCustom = false;

function defaultSettings(): DesktopSettings {
  const envRelays = (process.env.VELOCE_RELAY_ADDRS || "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);

  return {
    deviceName: DETECTED_DEVICE_NAME,
    downloadDir: path.join(os.homedir(), "Downloads", "Veloce"),
    relayAddrs: envRelays,
    parallelStreams: 8,
    theme: "dark",
    trustedPeers: [],
  };
}

function identityKeyFilePath(): string {
  return path.join(app.getPath("userData"), "veloce-identity.key");
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

  const trustedPeers: TrustedPeer[] = Array.isArray(raw.trustedPeers)
    ? Array.from(
        new Map(
          raw.trustedPeers
            .filter(
              (p): p is TrustedPeer =>
                Boolean(p) &&
                typeof p.nodeId === "string" &&
                p.nodeId.trim().length > 0 &&
                typeof p.deviceName === "string",
            )
            .map((p) => {
              const nodeId = p.nodeId.trim();
              const shortId =
                typeof p.shortId === "string" && p.shortId.trim().length > 0
                  ? p.shortId.trim()
                  : nodeId.slice(0, 8);
              const name = p.deviceName.trim() || shortId;
              const addedAt =
                typeof p.addedAt === "number" && Number.isFinite(p.addedAt)
                  ? p.addedAt
                  : Date.now();
              return [nodeId, { nodeId, shortId, deviceName: name, addedAt }] as const;
            }),
        ).values(),
      )
    : fallback.trustedPeers;

  return {
    deviceName,
    downloadDir,
    relayAddrs,
    parallelStreams,
    theme,
    trustedPeers,
  };
}

function loadPersistedSettings(): DesktopSettings {
  const fallback = defaultSettings();
  try {
    const file = settingsFilePath();
    if (!fs.existsSync(file)) {
      deviceNameCustom = false;
      return fallback;
    }
    const parsed = JSON.parse(fs.readFileSync(file, "utf8")) as Partial<PersistedDesktopSettings>;
    const rawSavedName = typeof parsed.deviceName === "string" ? parsed.deviceName.trim() : "";
    const host = (os.hostname() || "").trim();
    const looksLikeLegacyAutoName =
      !rawSavedName ||
      rawSavedName.toLowerCase() === "veloce desktop" ||
      (parsed.deviceNameCustom === undefined &&
        host.length > 0 &&
        rawSavedName.toLowerCase() === host.toLowerCase());

    if (parsed.deviceNameCustom === true) {
      deviceNameCustom = true;
    } else if (looksLikeLegacyAutoName) {
      deviceNameCustom = false;
      parsed.deviceName = fallback.deviceName;
    } else {
      deviceNameCustom = rawSavedName !== DETECTED_DEVICE_NAME;
    }

    return sanitizeSettings(parsed, fallback);
  } catch {
    deviceNameCustom = false;
    return fallback;
  }
}

function savePersistedSettings(next: DesktopSettings): void {
  const file = settingsFilePath();
  const dir = path.dirname(file);
  fs.mkdirSync(dir, { recursive: true });
  const tmp = `${file}.tmp`;
  const payload: PersistedDesktopSettings = {
    ...next,
    deviceNameCustom,
  };
  fs.writeFileSync(tmp, JSON.stringify(payload, null, 2), "utf8");
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
      detectedDeviceName: DETECTED_DEVICE_NAME,
      settings: currentSettings,
    };
  }
  return {
    ready: true,
    error: null,
    nodeInfo: JSON.parse(client.nodeInfoJson()),
    defaultOutputDir: currentSettings.downloadDir,
    detectedDeviceName: DETECTED_DEVICE_NAME,
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
      identityKeyPath: identityKeyFilePath(),
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
      if (typeof partial.deviceName === "string") {
        deviceNameCustom = next.deviceName !== DETECTED_DEVICE_NAME;
      }
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

  ipcMain.handle("veloce:createTextClip", (_event, text: string) => {
    if (typeof text !== "string" || text.trim().length === 0) {
      throw new Error("Text message cannot be empty");
    }
    const buf = Buffer.from(text, "utf8");
    if (buf.byteLength > MAX_TEXT_CLIP_BYTES) {
      throw new Error("Quick text clip exceeds 256 KB limit");
    }
    const clipDir = path.join(
      os.tmpdir(),
      "veloce-clips",
      crypto.randomBytes(8).toString("hex"),
    );
    fs.mkdirSync(clipDir, { recursive: true });
    const filePath = path.join(clipDir, TEXT_CLIP_FILENAME);
    fs.writeFileSync(filePath, buf);
    return { path: filePath, bytes: buf.byteLength };
  });

  ipcMain.handle("veloce:readTextClip", (_event, filePath: string) => {
    if (typeof filePath !== "string" || filePath.trim().length === 0) {
      return null;
    }
    const base = path.basename(filePath).toLowerCase();
    if (base !== TEXT_CLIP_FILENAME) {
      return null;
    }
    try {
      const stat = fs.statSync(filePath);
      if (!stat.isFile() || stat.size > MAX_TEXT_CLIP_BYTES) {
        return null;
      }
      const content = fs.readFileSync(filePath, "utf8");
      try {
        fs.unlinkSync(filePath);
      } catch {
        // Keep best-effort cleanup non-fatal.
      }
      return content;
    } catch {
      return null;
    }
  });

  ipcMain.handle("veloce:openPath", async (_event, filePath: string) => {
    if (typeof filePath !== "string" || filePath.trim().length === 0) {
      return "File path is empty";
    }
    if (!fs.existsSync(filePath)) {
      return "File no longer exists at saved location";
    }
    const err = await shell.openPath(filePath);
    return err || null;
  });

  ipcMain.handle("veloce:showInFolder", (_event, filePath: string) => {
    if (typeof filePath !== "string" || filePath.trim().length === 0) {
      return false;
    }
    if (!fs.existsSync(filePath)) {
      return false;
    }
    shell.showItemInFolder(filePath);
    return true;
  });

  ipcMain.handle("veloce:openExternalUrl", async (_event, rawUrl: string) => {
    if (typeof rawUrl !== "string" || rawUrl.trim().length === 0) {
      return false;
    }
    try {
      const parsed = new URL(rawUrl.trim());
      if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
        return false;
      }
      await shell.openExternal(parsed.toString());
      return true;
    } catch {
      return false;
    }
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
