// EchoStream Node.js server + client 端到端测试
// 同一个 Node 进程内：Server 处理 RPC/事件，Client 连接调用 —— 完整闭环
const path = require("node:path");
const root = path.resolve(__dirname, "../../..");
const wasm = require(path.join(root, "bindings/wasm/node/echostream_wasm.js"));
const { connect, ServerBuilder } = require("../index.js");

// 载荷编码约定（与 Rust 核心一致）：RPC 载荷 = postcard 编码，
// 有符号整数用 ZigZag varint（i64 10 → 0x14），事件 = postcard String
const encodeString = wasm.encode_payload;
const decodeString = wasm.decode_string;
const encodeI64 = wasm.encode_i64;
const decodeI64 = wasm.decode_i64;

async function main() {
  // ===== Node 侧服务端 =====
  const builder = new ServerBuilder();
  builder.bind("127.0.0.1:5001");
  // 回调遵循 Node error-first 约定：(err, data) => ...
  builder.addRpc("add", async (err, payload) => {
    if (err) throw err;
    // 载荷：(i64, i64) → [10, 20]
    const bytes = payload instanceof Uint8Array ? payload : Uint8Array.from(payload);
    const [a, b] = [decodeI64(bytes.subarray(0, 1)), decodeI64(bytes.subarray(1, 2))];
    console.log(`[node-server] add(${a}, ${b})`);
    return Buffer.from(encodeI64(BigInt(a + b)));
  });
  builder.addEvent("hello", (err, payload) => {
    if (err) throw err;
    const bytes = payload instanceof Uint8Array ? payload : Uint8Array.from(payload);
    console.log(`[node-server] 收到事件: ${decodeString(bytes)}`);
  });

  // 流接收：JS 侧拉帧
  const streamFrames = [];
  builder.addStream("chat", async (err, receiver) => {
    if (err) throw err;
    while (true) {
      const frame = await receiver.recv();
      if (frame === null) break;
      streamFrames.push(decodeString(Uint8Array.from(frame)));
    }
    console.log(`[node-server] 流 chat 结束，共 ${streamFrames.length} 帧`);
  });
  const server = await builder.build();
  console.log(`[node-server] 监听 ${server.addr()}`);
  const runPromise = server.run(); // 后台运行

  await new Promise((r) => setTimeout(r, 200));

  // ===== Node 侧客户端 =====
  const client = await connect("127.0.0.1:5001");
  console.log("[node-client] 已连接");

  const resp = await client.request(
    "add",
    Array.from(Buffer.concat([encodeI64(10n), encodeI64(20n)])),
  );
  const sum = decodeI64(Uint8Array.from(resp));
  console.log(`[node-client] add(10, 20) = ${sum}`);
  if (sum !== 30) throw new Error(`期望 30，实际 ${sum}`);

  await client.emit("hello", Array.from(encodeString("from node client")));

  // 推送流数据（服务端流接收验证）
  const stream = await client.createStream("chat");
  for (let i = 0; i < 3; i++) {
    await stream.send(Array.from(encodeString(`node stream ${i}`)));
  }
  await stream.finish();

  // 服务端广播
  await server.broadcast("hello", Array.from(encodeString("broadcast!")));

  await new Promise((r) => setTimeout(r, 300));
  client.close();
  server.shutdown();
  await runPromise;
  if (streamFrames.length !== 3) throw new Error(`期望 3 帧，实际 ${streamFrames.length}`);
  console.log("🎉 Node.js server + client 端到端测试通过（含流接收）");
  process.exit(0); // napi runtime 线程会阻止自然退出
}

main().catch((e) => {
  console.error("❌ 测试失败:", e);
  process.exit(1);
});
