// EchoStream Node.js 示例：客户端
// 连接 127.0.0.1:5101，调用 add(10,20)、发送 hello 事件、推送 3 帧 chat 流后退出。
// 运行：node examples/client.cjs（需先启动 examples/server.cjs）
const { connect } = require("../index.js");

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

async function main() {
  const client = await connect("127.0.0.1:5101");
  console.log("[client] 已连接");

  // RPC：add(10, 20) -> 30
  const resp = await client.request("add", encodeI64(10).concat(encodeI64(20)));
  console.log(`[client] add(10, 20) = ${decodeI64(Uint8Array.from(resp))}`);

  // 事件：hello
  await client.emit("hello", encodeString("来自 node 客户端"));

  // 流：chat 推送 3 帧
  const stream = await client.createStream("chat");
  for (let i = 0; i < 3; i++) {
    await stream.send(encodeString(`node stream ${i}`));
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
