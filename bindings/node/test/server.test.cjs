// EchoStream Node.js server + client 端到端测试
// 同一个 Node 进程内：Server 处理 RPC/事件，Client 连接调用 —— 完整闭环
const path = require("node:path");
const root = path.resolve(__dirname, "../../..");
const wasm = require(path.join(root, "bindings/wasm/node/echostream_wasm.js"));
const { connect, ServerBuilder } = require("../index.js");

const encode = wasm.encode_payload;
const decodeU64 = wasm.decode_u64;
const decodeString = wasm.decode_string;

async function main() {
  // ===== Node 侧服务端 =====
  const builder = new ServerBuilder();
  builder.bind("127.0.0.1:5001");
  // 回调遵循 Node error-first 约定：(err, data) => ...
  builder.addRpc("add", async (err, payload) => {
    if (err) throw err;
    // 载荷：(u64, u64) → [10, 20]
    const bytes = payload instanceof Uint8Array ? payload : Uint8Array.from(payload);
    const [a, b] = [wasm.decode_u64(bytes.subarray(0, 1)), wasm.decode_u64(bytes.subarray(1))];
    console.log(`[node-server] add(${a}, ${b})`);
    return Buffer.from(encode(a + b));
  });
  builder.addEvent("hello", (err, payload) => {
    if (err) throw err;
    const bytes = payload instanceof Uint8Array ? payload : Uint8Array.from(payload);
    console.log(`[node-server] 收到事件: ${decodeString(bytes)}`);
  });
  const server = await builder.build();
  console.log(`[node-server] 监听 ${server.addr()}`);
  const runPromise = server.run(); // 后台运行

  await new Promise((r) => setTimeout(r, 200));

  // ===== Node 侧客户端 =====
  const client = await connect("127.0.0.1:5001");
  console.log("[node-client] 已连接");

  const resp = await client.request("add", Array.from(encode([10, 20])));
  const sum = decodeU64(Uint8Array.from(resp));
  console.log(`[node-client] add(10, 20) = ${sum}`);
  if (sum !== 30) throw new Error(`期望 30，实际 ${sum}`);

  await client.emit("hello", Array.from(encode("from node client")));

  // 服务端广播
  await server.broadcast("hello", Array.from(encode("broadcast!")));

  await new Promise((r) => setTimeout(r, 200));
  client.close();
  server.shutdown();
  await runPromise;
  console.log("🎉 Node.js server + client 端到端测试通过");
  process.exit(0); // napi runtime 线程会阻止自然退出
}

main().catch((e) => {
  console.error("❌ 测试失败:", e);
  process.exit(1);
});
