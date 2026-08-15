// EchoStream 跨端 E2E 矩阵：Rust / Node / Python 交叉组合（6 组）
//
// 每组：启动服务端 -> 等待 E2E_SERVER_READY -> 运行客户端 ->
//       校验 add(10, 20) = 30、事件、3 帧流 -> 终止服务端。
// 端口 5110-5115。
//
// 用法：pnpm -C scripts build && node scripts/dist/cross_e2e.js
// 前置：cargo build -p echostream --release --example e2e_peer（脚本自动完成）
import { spawn, execSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import fs from "node:fs";
import os from "node:os";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
// 脚本位于 scripts/dist/，仓库根向上两级
const root = path.resolve(__dirname, "../..");
const LOG_DIR = path.join(os.tmpdir(), "echostream-cross-e2e");

const RUST_PEER = path.join(root, "target/release/examples/e2e_peer");
// Node 对端为 tsdown 构建产物（ESM）
const NODE_SERVER = path.join(root, "bindings/node/dist/test/cross_server.js");
const NODE_CLIENT = path.join(root, "bindings/node/dist/test/cross_client.js");
const PY_SERVER = path.join(root, "bindings/python/tests/cross_server.py");
const PY_CLIENT = path.join(root, "bindings/python/tests/cross_client.py");

// 前置：编译 Rust 通用对端
if (!fs.existsSync(RUST_PEER)) {
  console.log("== 编译 Rust 通用对端 (e2e_peer) ==");
  execSync("cargo build -p echostream --release --example e2e_peer", { cwd: root, stdio: "inherit" });
}

fs.mkdirSync(LOG_DIR, { recursive: true });

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

async function waitLog(logFile: string, timeoutMs: number, key: string): Promise<boolean> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      if (fs.readFileSync(logFile, "utf8").includes(key)) return true;
    } catch (_) { /* 日志尚未创建 */ }
    await sleep(100);
  }
  return false;
}

interface Combo {
  name: string;
  port: number;
  server: string;
  client: string;
}

async function runCombo(name: string, port: number, serverCmd: string, clientCmd: string): Promise<boolean> {
  const slog = path.join(LOG_DIR, name + ".server.log");
  const clog = path.join(LOG_DIR, name + ".client.log");
  fs.writeFileSync(slog, "");
  console.log("== [" + name + "] 启动服务端（端口 " + port + "）==");
  const server = spawn("bash", ["-c", serverCmd], {
    cwd: root,
    stdio: ["ignore", fs.openSync(slog, "a"), fs.openSync(slog, "a")],
  });

  const ready = await waitLog(slog, 15000, "E2E_SERVER_READY");
  if (!ready) {
    console.error("❌ FAIL [" + name + "] 服务端未就绪：" + fs.readFileSync(slog, "utf8"));
    server.kill("SIGKILL");
    return false;
  }

  console.log("== [" + name + "] 运行客户端 ==");
  let clientOk = true;
  try {
    execSync(clientCmd, { cwd: root, stdio: ["ignore", "inherit", "inherit"], timeout: 30000 });
  } catch (_) {
    clientOk = false;
  }

  const sLog = fs.readFileSync(slog, "utf8");
  const ok =
    clientOk &&
    sLog.includes("E2E_RPC add(10, 20)") &&
    sLog.includes("E2E_EVENT_RECEIVED") &&
    sLog.includes("E2E_STREAM_FRAMES=3");

  server.kill("SIGTERM");
  await sleep(300);
  if (server.exitCode === null) server.kill("SIGKILL");

  if (ok) {
    console.log("✅ PASS [" + name + "]");
  } else {
    console.error("❌ FAIL [" + name + "]（client exit=" + (clientOk ? 0 : "error") + "）");
    try { console.error(fs.readFileSync(clog, "utf8").slice(0, 2000)); } catch (_) { /* 无客户端日志 */ }
    console.error(sLog.slice(0, 3000));
  }
  return ok;
}

const combos: Combo[] = [
  { name: "rust-server_node-client", port: 5110, server: '"' + RUST_PEER + '" --server --addr 127.0.0.1:5110', client: 'node "' + NODE_CLIENT + '" 127.0.0.1:5110' },
  { name: "node-server_rust-client", port: 5111, server: 'node "' + NODE_SERVER + '" 5111', client: '"' + RUST_PEER + '" --client --addr 127.0.0.1:5111' },
  { name: "rust-server_python-client", port: 5112, server: '"' + RUST_PEER + '" --server --addr 127.0.0.1:5112', client: 'python3 "' + PY_CLIENT + '" 127.0.0.1:5112' },
  { name: "python-server_rust-client", port: 5113, server: 'python3 "' + PY_SERVER + '" 5113', client: '"' + RUST_PEER + '" --client --addr 127.0.0.1:5113' },
  { name: "node-server_python-client", port: 5114, server: 'node "' + NODE_SERVER + '" 5114', client: 'python3 "' + PY_CLIENT + '" 127.0.0.1:5114' },
  { name: "python-server_node-client", port: 5115, server: 'python3 "' + PY_SERVER + '" 5115', client: 'node "' + NODE_CLIENT + '" 127.0.0.1:5115' },
];

let pass = 0;
const failed: string[] = [];
for (const { name, port, server, client } of combos) {
  if (await runCombo(name, port, server, client)) pass++;
  else failed.push(name);
}

console.log("");
console.log("========== 跨端 E2E 矩阵结果 ==========");
console.log("PASS: " + pass + " / 6    FAIL: " + (6 - pass) + " / 6");
if (failed.length > 0) {
  console.log("失败组合: " + failed.join(", "));
  console.log("详细日志见: " + LOG_DIR);
  process.exit(1);
}
console.log("🎉 全部 6 个跨端组合通过，线缆格式跨端一致");
