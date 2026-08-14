// EchoStream 浏览器 SDK 端到端测试（Node 环境模拟浏览器网络层）
//
// 启动 Rust WebSocket 服务端（ws_chat_server），用 Node 内置 WebSocket 驱动
// 浏览器 SDK（bindings/web/echostream.js），验证新 DX 全链路：
// RPC 自动编解码 / 事件 / 出站流。
//
// 运行：node bindings/web/e2e.sdk.test.mjs

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "../..");

// Node 内置 WebSocket 需要 arraybuffer binaryType（与浏览器一致）
const server = spawn(
  "cargo",
  ["run", "-q", "-p", "echostream-transport", "--features", "ws", "--example", "ws_chat_server"],
  { cwd: root, stdio: ["ignore", "pipe", "pipe"] },
);
let serverLog = "";
server.stdout.on("data", (d) => { serverLog += d; });
server.stderr.on("data", (d) => { serverLog += d; });

function waitFor(key, timeoutMs = 30000) {
  return new Promise((resolve, reject) => {
    const start = Date.now();
    const timer = setInterval(() => {
      if (serverLog.includes(key)) { clearInterval(timer); resolve(); }
      else if (Date.now() - start > timeoutMs) {
        clearInterval(timer);
        reject(new Error("等待服务端就绪超时: " + key + " | 日志: " + serverLog));
      }
    }, 200);
  });
}

try {
  await waitFor("WebSocket 监听");
  console.log("✅ Rust WS 服务端已就绪");

  // 浏览器 SDK（Node 环境注入 wasm 字节，WebSocket 用 Node 内置实现）
  const { EchoStream } = await import("./echostream.js");
  const wasmBytes = new Uint8Array(
    await (await import("node:fs/promises")).readFile(
      new URL("./wasm/echostream_wasm_bg.wasm", import.meta.url),
    ),
  );
  const client = new EchoStream("ws://127.0.0.1:8081", { wasmModule: wasmBytes });
  await client.connect();
  console.log("✅ 已连接（自动编解码 DX）");

  // RPC：多参数自动元组 + 响应自动解码
  const sum = await client.request("add", 10, 20);
  assert.equal(sum, 30);
  console.log("✅ RPC add(10, 20) =", sum, "（自动编解码）");

  // 事件：自动编码
  await client.emit("hello", "e2e from web sdk");
  console.log("✅ 事件已发送（自动编码）");

  // 出站流：帧自动编码
  const stream = await client.createStream("chat");
  await stream.send("web frame 1");
  await stream.send("web frame 2");
  await stream.finish();
  console.log("✅ 出站流已发送 2 帧");

  await new Promise((r) => setTimeout(r, 300));

  // 服务端应收到事件与流
  assert.ok(serverLog.includes("收到事件"), "服务端应收到事件");
  assert.ok(serverLog.includes("流 chat 结束（2 帧）"), "服务端应收满 2 帧流");
  console.log("✅ 服务端收到事件与流帧");
  console.log("---- 服务端日志 ----");
  console.log(serverLog.split(String.fromCharCode(10)).filter((l) => l.includes("[server]")).join(String.fromCharCode(10)));

  client.close();
  console.log("🎉 浏览器 SDK 端到端测试通过（自动编解码全链路）");
} finally {
  server.kill("SIGTERM");
  await new Promise((r) => setTimeout(r, 300));
}