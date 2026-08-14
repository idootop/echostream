// EchoStream 跨端矩阵：Node 客户端（连接 Rust / Python 服务端）
// 用法：node test/cross_client.mjs [地址]（默认 127.0.0.1:5110）
import { connect } from "../index.js";

async function main() {
  const addr = process.argv[2] || "127.0.0.1:5110";
  const client = await connect(addr);
  console.log("[client] 已连接");

  // RPC：add(10, 20) -> 30（自动编解码）
  const sum = await client.request("add", 10, 20);
  console.log(`add(10, 20) = ${sum}`);
  if (sum !== 30) throw new Error(`期望 30，实际 ${sum}`);

  // 事件：hello
  await client.emit("hello", "from node client");

  // 流：chat 推送 3 帧
  const stream = await client.createStream("chat");
  for (let i = 0; i < 3; i++) {
    await stream.send(`node frame ${i}`);
  }
  await stream.finish();

  await new Promise((r) => setTimeout(r, 200));
  client.close();
  console.log("[client] 完成");
  process.exit(0);
}

main().catch((e) => {
  console.error("❌ 客户端出错:", e);
  process.exit(1);
});
