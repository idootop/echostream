// EchoStream 跨端矩阵：Node 服务端（供 Rust / Python 客户端连接）
// 新 DX：处理器参数自动解码、返回值自动编码，线缆格式与 Rust 核心一致。
// 用法：node test/cross_server.mjs [端口]（默认 5110）
import { ServerBuilder } from "../index.js";

async function main() {
  const port = Number(process.argv[2] || 5110);
  const addr = `127.0.0.1:${port}`;
  const builder = new ServerBuilder();
  builder.bind(addr);

  // RPC：add(i64, i64) -> i64
  builder.addRpc("add", async (a, b) => {
    console.log(`E2E_RPC add(${a}, ${b})`);
    return a + b;
  });

  // 事件：hello（String 载荷）
  builder.addEvent("hello", (data) => {
    console.log(`E2E_EVENT_RECEIVED: ${data}`);
  });

  // 流：chat（String 帧）
  builder.addStream("chat", async (receiver) => {
    let n = 0;
    while (true) {
      const frame = await receiver.recv();
      if (frame === null) break;
      console.log(`E2E_STREAM_FRAME ${n}: ${frame}`);
      n++;
    }
    console.log(`E2E_STREAM_FRAMES=${n}`);
  });

  const server = await builder.build();
  console.log(`E2E_SERVER_READY ${addr}`);
  await server.run(); // 阻塞至进程被终止
}

main().catch((e) => {
  console.error("❌ 服务端出错:", e);
  process.exit(1);
});
