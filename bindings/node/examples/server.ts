// EchoStream Node.js 示例：服务端（自动编解码 DX，TypeScript）
// 运行：node dist/examples/server.js（另开终端运行 dist/examples/client.js）
import { ServerBuilder } from "../index.js";

async function main(): Promise<void> {
  const builder = new ServerBuilder();
  builder.bind("0.0.0.0:5101");

  // RPC：参数自动解码展开（泛型元组标注参数与响应类型），返回值自动编码
  builder.addRpc<[number, number], number>("add", async (a, b) => {
    console.log(`[server] add(${a}, ${b})`);
    return a + b;
  });

  // 事件：载荷自动解码
  builder.addEvent<[string]>("hello", (data) => {
    console.log(`[server] 收到事件: ${data}`);
  });

  // 流：receiver.recv() 自动解码帧（泛型标注帧类型）
  builder.addStream("chat", async (receiver) => {
    let count = 0;
    while (true) {
      const frame = await receiver.recv<string>();
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
