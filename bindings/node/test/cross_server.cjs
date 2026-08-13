// EchoStream 跨端矩阵：Node 服务端（供 Rust / Python 客户端连接）
//
// 线缆格式约定（与 Rust 核心一致）：
// - RPC 载荷 = postcard 编码（varint）
// - 事件载荷 = postcard 编码的 String
// - 流帧载荷 = 原始 UTF-8 字节
//
// 用法：node cross_server.cjs [端口]（默认 5110）
const path = require("node:path");
const root = path.resolve(__dirname, "../../..");
const { ServerBuilder } = require("../index.js");

// ---------- postcard 编解码小工具（与 Python 侧手写实现一致） ----------

// u64 varint 编码（postcard 兼容）
function encodeU64(n) {
  const out = [];
  while (n >= 0x80) {
    out.push((n & 0x7f) | 0x80);
    n = Math.floor(n / 128);
  }
  out.push(n);
  return Buffer.from(out);
}

// u64 varint 解码，返回 [值, 消耗字节数]
function decodeU64(bytes, offset = 0) {
  let value = 0;
  let shift = 0;
  for (let i = offset; i < bytes.length; i++) {
    value |= (bytes[i] & 0x7f) << shift;
    if (!(bytes[i] & 0x80)) return [value, i - offset + 1];
    shift += 7;
  }
  throw new Error("varint 解码失败");
}

// String -> postcard 编码（varint 长度 + UTF-8 字节）
function encodeString(s) {
  const b = Buffer.from(s, "utf8");
  return Buffer.concat([encodeU64(b.length), b]);
}

// postcard 编码 -> String
function decodeString(bytes) {
  const [len, n] = decodeU64(bytes, 0);
  return bytes.subarray(n, n + len).toString("utf8");
}

async function main() {
  const port = Number(process.argv[2] || 5110);
  const addr = `127.0.0.1:${port}`;
  const builder = new ServerBuilder();
  builder.bind(addr);

  // RPC：add，载荷为 (u64, u64) 两个连续 varint
  builder.addRpc("add", async (err, payload) => {
    if (err) throw err;
    const bytes = Buffer.isBuffer(payload) ? payload : Buffer.from(payload);
    const [a, n] = decodeU64(bytes, 0);
    const [b] = decodeU64(bytes, n);
    console.log(`E2E_RPC add(${a}, ${b})`);
    return encodeU64(a + b);
  });

  // 事件：hello，载荷为 postcard 编码的 String
  builder.addEvent("hello", (err, payload) => {
    if (err) throw err;
    const bytes = Buffer.isBuffer(payload) ? payload : Buffer.from(payload);
    console.log(`E2E_EVENT_RECEIVED: ${decodeString(bytes)}`);
  });

  // 流：chat，帧载荷为原始 UTF-8 字节
  builder.addStream("chat", async (err, receiver) => {
    if (err) throw err;
    let n = 0;
    while (true) {
      const frame = await receiver.recv();
      if (frame === null) break;
      console.log(`E2E_STREAM_FRAME ${n}: ${Buffer.from(frame).toString("utf8")}`);
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
