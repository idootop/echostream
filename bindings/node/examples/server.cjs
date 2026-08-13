// EchoStream Node.js 示例：服务端
// 注册 add RPC（(a,b) -> a+b）、hello 事件、chat 流（接收并打印帧），监听 127.0.0.1:5101。
// 运行：node examples/server.cjs（另开终端运行 examples/client.cjs）
const { ServerBuilder } = require("../index.js");

// ---- postcard 编解码（varint，与 Rust 线缆格式一致）----
function encodeU64(n) {
  const out = [];
  while (n >= 0x80) {
    out.push((n & 0x7f) | 0x80);
    n = Math.floor(n / 128);
  }
  out.push(n);
  return out;
}

function decodeU64(bytes) {
  let result = 0;
  let shift = 0;
  for (const b of bytes) {
    result |= (b & 0x7f) << shift;
    if (!(b & 0x80)) break;
    shift += 7;
  }
  return result;
}

// 字符串：varint 长度 + UTF-8 字节
function encodeString(s) {
  const buf = Buffer.from(s, "utf8");
  return encodeU64(buf.length).concat([...buf]);
}

function decodeString(bytes) {
  let len = 0;
  let shift = 0;
  let i = 0;
  for (; i < bytes.length; i++) {
    len |= (bytes[i] & 0x7f) << shift;
    if (!(bytes[i] & 0x80)) {
      i++;
      break;
    }
    shift += 7;
  }
  return Buffer.from(bytes.subarray(i, i + len)).toString("utf8");
}

async function main() {
  const builder = new ServerBuilder();
  builder.bind("127.0.0.1:5101");

  // add RPC：载荷 (u64, u64) → 响应 u64
  builder.addRpc("add", async (err, payload) => {
    if (err) throw err;
    const bytes = payload instanceof Uint8Array ? payload : Uint8Array.from(payload);
    const [a, b] = [decodeU64(bytes.subarray(0, 1)), decodeU64(bytes.subarray(1))];
    console.log(`[server] add(${a}, ${b})`);
    return Buffer.from(encodeU64(a + b));
  });

  // hello 事件：打印收到的文本
  builder.addEvent("hello", (err, payload) => {
    if (err) throw err;
    const bytes = payload instanceof Uint8Array ? payload : Uint8Array.from(payload);
    console.log(`[server] 收到事件: ${decodeString(bytes)}`);
  });

  // chat 流：JS 侧拉帧直到结束
  builder.addStream("chat", async (err, receiver) => {
    if (err) throw err;
    let count = 0;
    while (true) {
      const frame = await receiver.recv();
      if (frame === null) break;
      count++;
      console.log(`[server] 流帧 #${count}: ${decodeString(Uint8Array.from(frame))}`);
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
