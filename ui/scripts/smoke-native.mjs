import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const nativePath = path.resolve(__dirname, "..", "native", "veloce_core.node");

const addon = require(nativePath);

async function main() {
  const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "veloce-napi-smoke-"));
  const srcDir = path.join(tmpRoot, "src");
  const dstDir = path.join(tmpRoot, "dst");
  fs.mkdirSync(srcDir, { recursive: true });
  fs.mkdirSync(dstDir, { recursive: true });

  const payload = Buffer.alloc(256 * 1024);
  for (let i = 0; i < payload.length; i++) {
    payload[i] = (i * 17) & 0xff;
  }
  const srcFile = path.join(srcDir, "veloce-napi-test.bin");
  fs.writeFileSync(srcFile, payload);

  const sender = await addon.VeloceNativeClient.create({
    deviceName: "Node-Sender",
    bindAddr: "127.0.0.1:0",
    enableMdns: false,
    parallelStreams: 4,
  });

  const receiver = await addon.VeloceNativeClient.create({
    deviceName: "Node-Receiver",
    bindAddr: "127.0.0.1:0",
    enableMdns: false,
    parallelStreams: 4,
  });

  const senderInfo = JSON.parse(sender.nodeInfoJson());
  const receiverInfo = JSON.parse(receiver.nodeInfoJson());
  assert.ok(senderInfo.nodeId.length > 20);
  assert.notEqual(senderInfo.nodeId, receiverInfo.nodeId);

  const events = [];
  let resolveComplete;
  let rejectComplete;
  const completedPromise = new Promise((res, rej) => {
    resolveComplete = res;
    rejectComplete = rej;
  });

  receiver.onEvent((rawJson) => {
    const evt = JSON.parse(rawJson);
    events.push(evt);
    if (evt.type === "TransferCompleted") {
      resolveComplete(evt);
    } else if (evt.type === "TransferFailed") {
      rejectComplete(new Error(evt.error));
    }
  });

  const started = JSON.parse(await sender.startSession([srcFile], null, null));
  assert.equal(started.fileCount, 1);
  assert.equal(started.totalSize, payload.length);
  assert.ok(started.ticketCode.startsWith("veloce1"));

  const preview = JSON.parse(addon.parseTicket(started.ticketCode));
  assert.equal(preview.totalSize, payload.length);

  // Smoke-test Pause & Resume on the hosted sender session before joining.
  assert.equal(sender.pauseTransfer(started.transferId), true);
  assert.equal(await sender.resumeTransfer(started.transferId), true);

  await receiver.joinSession(started.ticketCode, dstDir, 4);

  const completed = await Promise.race([
    completedPromise,
    new Promise((_, rej) => setTimeout(() => rej(new Error("timeout waiting for NAPI transfer")), 10000)),
  ]);

  assert.equal(completed.totalBytes, payload.length);
  const received = fs.readFileSync(path.join(dstDir, "veloce-napi-test.bin"));
  assert.equal(Buffer.compare(received, payload), 0);

  const sawConnType = events.some((e) => e.type === "ConnectionTypeChanged" && e.connectionType === "LAN");
  const sawProgress = events.some((e) => e.type === "TransferProgress");
  assert.ok(sawConnType, "expected ConnectionTypeChanged event");
  assert.ok(sawProgress, "expected TransferProgress event");

  sender.shutdown();
  receiver.shutdown();
  fs.rmSync(tmpRoot, { recursive: true, force: true });
  console.log(
    `[veloce] Native napi-rs smoke test PASSED (${payload.length} bytes, ${events.length} events, ${completed.elapsedMs} ms)`
  );
  process.exit(0);
}

main().catch((err) => {
  console.error("[veloce] Native smoke test FAILED:", err);
  process.exit(1);
});
