// EchoStream Node.js 示例：服务端（自动编解码 DX）
// 运行：node examples/server.mjs（另开终端运行 examples/client.mjs）
import { ServerBuilder } from "../index.js";

async function main() {
  const builder = new ServerBuilder();
  builder.bind("0.0.0.0:5101");

  // RPC：参数自动解码展开，返回值自动编码
  builder.addRpc("add", async (a, b) => {
    console.log(`[server] add(${a}, ${b})`);
    return a + b;
  });

  // 事件：载荷自动解码
  builder.addEvent("hello", (data) => {
    console.log(`[server] 收到事件: ${data}`);
  });

  // 流：receiver.recv() 自动解码帧
  builder.addStream("chat", async (receiver) => {
    let count = 0;
    while (true) {
      const frame = await receiver.recv();
      if (frame === null) break;
      count++;
      console.log(`[server] 流帧 #${count}: ${frame}`);
    }
    console.log(`[server] 流 chat 结束，共 ${count} 帧`);
  });

  const server = await builder.build();
  console.log(`[server] 监听 ${server.addr()}`);
  const runPromise = server.run(); // 后台运行，shutdown 后 resolve

  // Ctrl+C 优雅退出
  process.on("SIGINT", () => {
    console.log("\n[server] 收到 Ctrl+C，正在关闭...");
    server.shutdown();
  });

  await runPromise;
  console.log("[server] 已退出");
  process.exit(0); // napi runtime 线程会阻止自然退出
}

main().catch((e) => {
  console.error("❌ 服务端出错:", e);
  process.exit(1);
});
