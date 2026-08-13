// EchoStream Node.js binding 端到端测试
// 前置：先运行 `cargo run -p echostream --example chat_server`
const { execSync, spawn } = require("node:child_process");
const path = require("node:path");

const root = path.resolve(__dirname, "../../..");
const wasm = require(path.join(root, "bindings/wasm/node/echostream_wasm.js"));
const { connect } = require("../index.js");

const encode = wasm.encode_payload;
const decodeU64 = wasm.decode_u64;
const decodeString = wasm.decode_string;

async function main() {
  const client = await connect("127.0.0.1:5000");
  console.log("✅ 已连接");

  // RPC：add(10, 20) —— 载荷为 postcard 字节（wasm 编解码复用）
  const resp = await client.request("add", Array.from(encode([10, 20])));
  const sum = decodeU64(Uint8Array.from(resp));
  console.log(`✅ add(10, 20) = ${sum}`);
  if (sum !== 30) throw new Error(`期望 30，实际 ${sum}`);

  // 事件：emit hello
  await client.emit("hello", Array.from(encode("from node")));
  console.log("✅ 事件已发送");

  // 流：推送 3 帧
  const stream = await client.createStream("chat");
  for (let i = 0; i < 3; i++) {
    await stream.send(Array.from(encode(`node frame ${i}`)));
  }
  await stream.finish();
  console.log("✅ 流已发送 3 帧");

  client.close();
  console.log("🎉 Node.js binding 端到端测试通过");
}

main().catch((e) => {
  console.error("❌ 测试失败:", e);
  process.exit(1);
});
