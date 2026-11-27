import { contextBridge, ipcRenderer, webUtils } from "electron";

const veloceApi = {
  getStatus: () => ipcRenderer.invoke("veloce:getStatus"),
  getSettings: () => ipcRenderer.invoke("veloce:getSettings"),
  updateSettings: (settings: {
    deviceName?: string;
    downloadDir?: string;
    relayAddrs?: string[];
    parallelStreams?: number;
    theme?: string;
    runInBackground?: boolean;
    trustedPeers?: Array<{
      nodeId: string;
      shortId: string;
      deviceName: string;
      addedAt: number;
    }>;
  }) => ipcRenderer.invoke("veloce:updateSettings", settings),
  previewTheme: (theme: string) => ipcRenderer.invoke("veloce:previewTheme", theme),
  pickFiles: () => ipcRenderer.invoke("veloce:pickFiles"),
  pickFolders: () => ipcRenderer.invoke("veloce:pickFolders"),
  pickOutputDir: () => ipcRenderer.invoke("veloce:pickOutputDir"),
  getPathForFile: (file: File) => webUtils.getPathForFile(file),
  createTextClip: (text: string) => ipcRenderer.invoke("veloce:createTextClip", text),
  readTextClip: (filePath: string) => ipcRenderer.invoke("veloce:readTextClip", filePath),
  openPath: (filePath: string) => ipcRenderer.invoke("veloce:openPath", filePath),
  showInFolder: (filePath: string) => ipcRenderer.invoke("veloce:showInFolder", filePath),
  openExternalUrl: (url: string) => ipcRenderer.invoke("veloce:openExternalUrl", url),
  startSession: (args: {
    paths: string[];
    targetPeerNodeId?: string;
    relayHints?: string[];
  }) => ipcRenderer.invoke("veloce:startSession", args),
  joinSession: (args: {
    ticketCode: string;
    outputDir: string;
    parallelStreams?: number;
  }) => ipcRenderer.invoke("veloce:joinSession", args),
  parseTicket: (ticketCode: string) =>
    ipcRenderer.invoke("veloce:parseTicket", ticketCode),
  listLocalPeers: () => ipcRenderer.invoke("veloce:listLocalPeers"),
  acceptIncoming: (args: { transferId: string; outputDir: string }) =>
    ipcRenderer.invoke("veloce:acceptIncoming", args),
  rejectIncoming: (args: { transferId: string; reason?: string }) =>
    ipcRenderer.invoke("veloce:rejectIncoming", args),
  pauseTransfer: (transferId: string) =>
    ipcRenderer.invoke("veloce:pauseTransfer", transferId),
  resumeTransfer: (transferId: string) =>
    ipcRenderer.invoke("veloce:resumeTransfer", transferId),
  cancelTransfer: (transferId: string) =>
    ipcRenderer.invoke("veloce:cancelTransfer", transferId),
  onEvent: (listener: (event: unknown) => void) => {
    const wrapped = (_ipcEvent: Electron.IpcRendererEvent, payload: unknown) => {
      listener(payload);
    };
    ipcRenderer.on("veloce:event", wrapped);
    return () => {
      ipcRenderer.removeListener("veloce:event", wrapped);
    };
  },
};

contextBridge.exposeInMainWorld("veloce", veloceApi);
