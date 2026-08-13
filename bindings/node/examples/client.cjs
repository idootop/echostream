// EchoStream Node.js 示例：客户端
// 连接 127.0.0.1:5101，调用 add(10,20)、发送 hello 事件、推送 3 帧 chat 流后退出。
// 运行：node examples/client.cjs（需先启动 examples/server.cjs）
const { connect } = require("../index.js");

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

async function main() {
  const client = await connect("127.0.0.1:5101");
  console.log("[client] 已连接");

  // RPC：add(10, 20) -> 30
  const resp = await client.request("add", encodeU64(10).concat(encodeU64(20)));
  console.log(`[client] add(10, 20) = ${decodeU64(Uint8Array.from(resp))}`);

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
