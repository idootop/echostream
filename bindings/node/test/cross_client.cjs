// EchoStream 跨端矩阵：Node 客户端（连接 Rust / Python 服务端）
//
// 线缆格式约定（与 Rust 核心一致）：
// - RPC 载荷 = postcard 编码（i64 为 ZigZag varint，String 为长度前缀 + UTF-8）
// - 事件载荷 = postcard 编码的 String
// - 流帧载荷 = 原始 UTF-8 字节
//
// 用法：node cross_client.cjs <ip:port>（默认 127.0.0.1:5110）
const path = require("node:path");
const root = path.resolve(__dirname, "../../..");
const { connect } = require("../index.js");

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

// (u64, u64) 元组 -> postcard 编码
function encodeTuple(a, b) {
  return Buffer.concat([encodeU64(a), encodeU64(b)]);
}

// i64 ZigZag 编码（postcard 对 i64 的 varint 语义）：n -> 2n 或 -2n-1
function zigzagEncode(n) {
  return n < 0 ? -2 * n - 1 : 2 * n;
}

// ZigZag 解码：z -> 原值
function zigzagDecode(z) {
  const v = z >> 1;
  return z & 1 ? -v - 1 : v;
}

// (i64, i64) -> postcard 编码（add RPC 请求载荷）
function encodeTupleI64(a, b) {
  return Buffer.concat([encodeU64(zigzagEncode(a)), encodeU64(zigzagEncode(b))]);
}

async function main() {
  const addr = process.argv[2] || "127.0.0.1:5110";
  const client = await connect(addr);
  console.log("[node-client] 已连接");

  // RPC：add(10, 20) = 30（载荷为 postcard (i64, i64) = 两个 ZigZag varint）
  const resp = await client.request("add", Array.from(encodeTupleI64(10, 20)));
  const sum = zigzagDecode(decodeU64(Buffer.from(resp), 0)[0]);
  console.log(`add(10, 20) = ${sum}`);
  if (sum !== 30) throw new Error(`add 期望 30，实际 ${sum}`);

  // 事件：hello（postcard 编码的 String）
  await client.emit("hello", Array.from(encodeString("hello from node client")));

  // 流：chat 3 帧（原始 UTF-8 字节）
  const stream = await client.createStream("chat");
  for (let i = 0; i < 3; i++) {
    await stream.send(Array.from(Buffer.from(`node frame ${i}`, "utf8")));
  }
  await stream.finish();

  // 等服务端处理完事件与流帧
  await new Promise((r) => setTimeout(r, 500));
  client.close();
  console.log("E2E_CLIENT_DONE");
  process.exit(0); // napi runtime 线程会阻止自然退出
}

main().catch((e) => {
  console.error("❌ 客户端失败:", e);
  process.exit(1);
});
