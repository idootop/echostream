// EchoStream Node.js 示例：客户端（自动编解码 DX，TypeScript）
// 运行：node dist/examples/client.js（需先启动 dist/examples/server.js）
import { connect } from "../index.js";

async function main(): Promise<void> {
  const client = await connect("127.0.0.1:5101");
  console.log("[client] 已连接");

  // RPC：多参数自动元组，响应自动解码（泛型标注响应类型）
  const sum = await client.request<number>("add", 10, 20);
  console.log(`[client] add(10, 20) = ${sum}`);

  // 事件：自动编码
  await client.emit("hello", "来自 node 客户端");

  // 流：帧自动编码
  const stream = await client.createStream("chat");
  for (let i = 0; i < 3; i++) {
    await stream.send(`node stream ${i}`);
  }
  await stream.finish();

  // 等服务端处理完再退出
  await new Promise((r) => setTimeout(r, 300));
  client.close();
  console.log("[client] 完成");
  process.exit(0); // napi runtime 线程会阻止自然退出
}

main().catch((e) => {
  console.error("❌ 客户端出错:", e);
  process.exit(1);
});
