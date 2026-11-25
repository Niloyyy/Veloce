export const VELOCE_TEXT_CLIP_FILENAME = "veloce-clip-message.txt";
export const MAX_TEXT_CLIP_BYTES = 256 * 1024;

export function isTextClipPath(filePath: string): boolean {
  const base = filePath.split(/[\\/]/).pop()?.trim().toLowerCase() ?? "";
  return base === VELOCE_TEXT_CLIP_FILENAME;
}

export function extractFirstHttpUrl(text: string): string | null {
  const match = text.match(/https?:\/\/[^\s<>"{}|\\^`[\]]+/i);
  if (!match) return null;
  const candidate = match[0].replace(/[),.;!?]+$/, "");
  try {
    const parsed = new URL(candidate);
    if (parsed.protocol === "http:" || parsed.protocol === "https:") {
      return parsed.toString();
    }
  } catch {
    return null;
  }
  return null;
}

export function formatTextClipLabel(text: string, prefix?: string): string {
  const singleLine = text.replace(/\s+/g, " ").trim();
  const short =
    singleLine.length > 42 ? `${singleLine.slice(0, 42)}…` : singleLine;
  const kind = extractFirstHttpUrl(singleLine) ? "Link" : "Text";
  if (prefix) return `${prefix} ${kind}: ${short || "(empty)"}`;
  return `${kind}: ${short || "(empty)"}`;
}
