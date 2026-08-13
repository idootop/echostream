// EchoStream Node.js 示例：服务端
// 注册 add RPC（(a,b) -> a+b）、hello 事件、chat 流（接收并打印帧），监听 127.0.0.1:5101。
// 运行：node examples/server.cjs（另开终端运行 examples/client.cjs）
const { ServerBuilder } = require("../index.js");

// ---- postcard 编解码（与 Rust 线缆格式一致）----
// i64 用 ZigZag varint：10 → 0x14；字符串 = varint 长度 + UTF-8 字节
function encodeI64(n) {
  const zz = BigInt.asUintN(64, (BigInt(n) << 1n) ^ (BigInt(n) >> 63n));
  const out = [];
  let v = zz;
  while (v >= 0x80n) {
    out.push(Number((v & 0x7fn) | 0x80n));
    v >>= 7n;
  }
  out.push(Number(v));
  return out;
}

function decodeI64(bytes) {
  let v = 0n;
  let shift = 0n;
  for (const b of bytes) {
    v |= BigInt(b & 0x7f) << shift;
    if (!(b & 0x80)) break;
    shift += 7n;
  }
  return Number(BigInt.asIntN(64, (v >> 1n) ^ -(v & 1n)));
}

// 字符串：varint 长度 + UTF-8 字节
function encodeString(s) {
  const buf = Buffer.from(s, "utf8");
  const out = [];
  let n = buf.length;
  while (n >= 0x80) {
    out.push((n & 0x7f) | 0x80);
    n = Math.floor(n / 128);
  }
  out.push(n);
  return out.concat([...buf]);
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

  // add RPC：载荷 (i64, i64) → 响应 i64
  builder.addRpc("add", async (err, payload) => {
    if (err) throw err;
    const bytes = payload instanceof Uint8Array ? payload : Uint8Array.from(payload);
    const [a, b] = [decodeI64(bytes.subarray(0, 1)), decodeI64(bytes.subarray(1, 2))];
    console.log(`[server] add(${a}, ${b})`);
    return Buffer.from(encodeI64(a + b));
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
