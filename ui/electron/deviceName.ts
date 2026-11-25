import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";

const GENERIC_VALUES = new Set([
  "",
  "none",
  "n/a",
  "na",
  "unknown",
  "invalid",
  "empty",
  "default string",
  "system product name",
  "system manufacturer",
  "system version",
  "to be filled by o.e.m.",
  "to be filled by o.e.m",
  "o.e.m.",
  "oem",
  "not specified",
  "not applicable",
  "sku",
  "0",
  "1.0",
  "standard pc",
  "veloce desktop",
]);

const VENDOR_ALIASES: Record<string, string> = {
  "asustek computer inc.": "ASUS",
  "asustek computer inc": "ASUS",
  "asus": "ASUS",
  "dell inc.": "Dell",
  "dell computer corporation": "Dell",
  "dell": "Dell",
  "hewlett-packard": "HP",
  "hp inc.": "HP",
  "hp": "HP",
  "lenovo": "Lenovo",
  "micro-star international co., ltd.": "MSI",
  "micro-star international co., ltd": "MSI",
  "msi": "MSI",
  "acer": "Acer",
  "acer inc.": "Acer",
  "apple inc.": "Apple",
  "apple": "Apple",
  "gigabyte technology co., ltd.": "Gigabyte",
  "gigabyte": "Gigabyte",
  "asrock": "ASRock",
  "framework": "Framework",
  "razer": "Razer",
  "samsung electronics co., ltd.": "Samsung",
  "samsung": "Samsung",
  "lg electronics": "LG",
  "huawei": "HUAWEI",
  "xiaomi": "Xiaomi",
  "microsoft corporation": "Microsoft",
  "system76": "System76",
  "tuxedo": "TUXEDO",
};

export function isGenericHardwareString(raw: string | null | undefined): boolean {
  const cleaned = (raw ?? "").replace(/\0/g, "").trim();
  if (!cleaned) return true;
  const lower = cleaned.toLowerCase();
  if (GENERIC_VALUES.has(lower)) return true;
  if (
    lower.includes("to be filled by") ||
    lower.includes("default string") ||
    lower.includes("system product") ||
    lower.startsWith("standard pc (") ||
    lower === "virtualbox" ||
    lower === "vmware virtual platform" ||
    lower === "vmware7,1" ||
    lower === "qemu" ||
    lower === "bochs" ||
    lower === "kvm"
  ) {
    return true;
  }
  return false;
}

export function cleanVendorName(raw: string | null | undefined): string {
  const cleaned = (raw ?? "").replace(/\0/g, "").trim();
  if (isGenericHardwareString(cleaned)) return "";
  const aliased = VENDOR_ALIASES[cleaned.toLowerCase()];
  if (aliased) return aliased;

  const stripped = cleaned
    .replace(/\b(co\.?,?\s*ltd\.?|inc\.?|corp\.?|corporation|ltd\.?|llc|gmbh)\b\.?/gi, "")
    .replace(/,\s*$/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return isGenericHardwareString(stripped) ? "" : stripped;
}

export function formatHardwareModel(
  vendorRaw: string | null | undefined,
  modelRaw: string | null | undefined,
): string {
  const model = (modelRaw ?? "").replace(/\0/g, "").replace(/\s+/g, " ").trim();
  if (isGenericHardwareString(model)) return "";
  const vendor = cleanVendorName(vendorRaw);
  if (!vendor) return model.slice(0, 64);

  const lowerModel = model.toLowerCase();
  const lowerVendor = vendor.toLowerCase();
  if (
    lowerModel.startsWith(lowerVendor) ||
    lowerModel.includes(` ${lowerVendor} `) ||
    (lowerVendor === "apple" &&
      (lowerModel.startsWith("mac") || lowerModel.startsWith("imac"))) ||
    (lowerVendor === "microsoft" && lowerModel.startsWith("surface")) ||
    (lowerVendor === "lenovo" &&
      (lowerModel.startsWith("thinkpad") ||
        lowerModel.startsWith("ideapad") ||
        lowerModel.startsWith("legion") ||
        lowerModel.startsWith("yoga")))
  ) {
    return model.slice(0, 64);
  }
  return `${vendor} ${model}`.slice(0, 64);
}

function readTrimmedFile(filePath: string): string {
  try {
    return fs.readFileSync(filePath, "utf8").replace(/\0/g, "").trim();
  } catch {
    return "";
  }
}

function detectLinuxModel(): string {
  const dmiBase = "/sys/devices/virtual/dmi/id";
  const vendor = readTrimmedFile(`${dmiBase}/sys_vendor`);
  const productName = readTrimmedFile(`${dmiBase}/product_name`);
  const productVersion = readTrimmedFile(`${dmiBase}/product_version`);
  const productFamily = readTrimmedFile(`${dmiBase}/product_family`);
  const boardName = readTrimmedFile(`${dmiBase}/board_name`);

  // Lenovo laptops put the human-readable model ("ThinkPad X1 Carbon Gen 11")
  // in product_version while product_name holds an opaque machine type ("21HM002GUS").
  const isLenovo = cleanVendorName(vendor).toLowerCase() === "lenovo";
  const candidates = isLenovo
    ? [productVersion, productFamily, productName, boardName]
    : [productName, boardName, productFamily, productVersion];

  for (const candidate of candidates) {
    const formatted = formatHardwareModel(vendor, candidate);
    if (formatted) return formatted;
  }

  // ARM laptops / SBCs (Raspberry Pi, Apple Silicon Linux, Chromebooks)
  const dtModel = readTrimmedFile("/proc/device-tree/model");
  if (!isGenericHardwareString(dtModel)) {
    return dtModel.slice(0, 64);
  }

  return "";
}

function mapMacHwModel(hwModel: string): string {
  const id = hwModel.trim();
  if (!id) return "";
  if (id.startsWith("MacBookPro")) return "MacBook Pro";
  if (id.startsWith("MacBookAir")) return "MacBook Air";
  if (id.startsWith("MacBook")) return "MacBook";
  if (id.startsWith("Macmini")) return "Mac mini";
  if (id.startsWith("MacStudio")) return "Mac Studio";
  if (id.startsWith("MacPro")) return "Mac Pro";
  if (id.startsWith("iMac")) return "iMac";
  if (id.startsWith("Mac")) return "Mac";
  return id;
}

function detectMacModel(): string {
  try {
    const out = execFileSync("ioreg", ["-rd1", "-c", "IOPlatformExpertDevice"], {
      encoding: "utf8",
      timeout: 1500,
      stdio: ["ignore", "pipe", "ignore"],
    });
    const match = out.match(/"product-name"\s*=\s*<"([^"]+)">/);
    if (match?.[1] && !isGenericHardwareString(match[1])) {
      return match[1].trim().slice(0, 64);
    }
  } catch {
    // Fall through to sysctl.
  }

  try {
    const hwModel = execFileSync("sysctl", ["-n", "hw.model"], {
      encoding: "utf8",
      timeout: 1500,
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    const mapped = mapMacHwModel(hwModel);
    if (mapped && !isGenericHardwareString(mapped)) {
      return mapped.slice(0, 64);
    }
  } catch {
    // Fall through to hostname.
  }

  return "";
}

function queryWindowsRegValue(key: string, valueName: string): string {
  try {
    const out = execFileSync("reg", ["query", key, "/v", valueName], {
      encoding: "utf8",
      timeout: 1500,
      windowsHide: true,
      stdio: ["ignore", "pipe", "ignore"],
    });
    const regex = new RegExp(`${valueName}\\s+REG_\\w+\\s+(.+)`, "i");
    const match = out.match(regex);
    return match?.[1]?.trim() ?? "";
  } catch {
    return "";
  }
}

function detectWindowsModel(): string {
  const biosKey = "HKLM\\HARDWARE\\DESCRIPTION\\System\\BIOS";
  const vendor = queryWindowsRegValue(biosKey, "SystemManufacturer");
  const product = queryWindowsRegValue(biosKey, "SystemProductName");
  const family = queryWindowsRegValue(biosKey, "SystemFamily");
  const board = queryWindowsRegValue(biosKey, "BaseBoardProduct");

  const isLenovo = cleanVendorName(vendor).toLowerCase() === "lenovo";
  const candidates = isLenovo
    ? [family, product, board]
    : [product, family, board];

  for (const candidate of candidates) {
    const formatted = formatHardwareModel(vendor, candidate);
    if (formatted) return formatted;
  }
  return "";
}

export function cleanHostnameFallback(): string {
  const host = (os.hostname() || "").replace(/\.local$/i, "").trim();
  if (host && host.toLowerCase() !== "localhost") {
    return host.slice(0, 64);
  }
  switch (process.platform) {
    case "darwin":
      return "Mac";
    case "win32":
      return "Windows PC";
    default:
      return "Linux PC";
  }
}

/**
 * Detects the real laptop/desktop hardware model name on Linux, macOS, and Windows,
 * falling back cleanly to the machine hostname when hardware DMI info is generic or unavailable.
 */
export function detectHardwareModelName(): string {
  let detected = "";
  if (process.platform === "linux") {
    detected = detectLinuxModel();
  } else if (process.platform === "darwin") {
    detected = detectMacModel();
  } else if (process.platform === "win32") {
    detected = detectWindowsModel();
  }
  return detected || cleanHostnameFallback();
}
