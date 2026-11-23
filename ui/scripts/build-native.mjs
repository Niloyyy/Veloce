import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const uiRoot = path.resolve(__dirname, "..");
const repoRoot = path.resolve(uiRoot, "..");
const isRelease = process.argv.includes("--release");
const isOffline =
  process.argv.includes("--offline") || process.env.CARGO_NET_OFFLINE === "true";
const profile = isRelease ? "release" : "debug";

const cargoArgs = ["build", "-p", "core-ffi", "--features", "napi-addon"];
if (isOffline) {
  cargoArgs.push("--offline");
}
if (isRelease) {
  cargoArgs.push("--release");
}

console.log(`[veloce] Building core-ffi (${profile})...`);
execFileSync("cargo", cargoArgs, { cwd: repoRoot, stdio: "inherit" });

const libName =
  process.platform === "win32"
    ? "core_ffi.dll"
    : process.platform === "darwin"
      ? "libcore_ffi.dylib"
      : "libcore_ffi.so";

const src = path.join(repoRoot, "target", profile, libName);
const outDir = path.join(uiRoot, "native");
const dst = path.join(outDir, "veloce_core.node");

fs.mkdirSync(outDir, { recursive: true });
fs.copyFileSync(src, dst);
console.log(`[veloce] Installed native addon -> ${dst}`);
