// EchoStream SDK 编解码交叉验证
// 基准字节来自 Rust 侧（crates/echostream/examples/codec_probe.rs）
import assert from "node:assert/strict";
import { encode, encodeMessage, decodeMessage, encodeFrame, Reader } from "./postcard.js";

const hex = (bytes) => Array.from(bytes).map((b) => b.toString(16).padStart(2, "0")).join(" ");

// 载荷编码与 Rust postcard 一致
assert.equal(hex(encode("hello")), "05 68 65 6c 6c 6f");
assert.equal(hex(encode([10, 20])), "0a 14"); // (10u64, 20u64)
assert.equal(hex(encode([1, 2, 3])), "01 02 03"); // 元组语义（无长度前缀）
assert.equal(hex(encode(42)), "2a");

// 负数 zigzag
assert.equal(hex(encode(-1)), "01");
assert.equal(hex(encode(-2)), "03");

// Message 编码与 Rust 一致
assert.equal(
  hex(encodeMessage({ type: "request", id: 1n, name: "add", data: encode([10, 20]) })),
  "00 01 03 61 64 64 02 0a 14"
);
assert.equal(
  hex(encodeMessage({ type: "response", id: 1n, code: 0, message: null, data: encode(30) })),
  "01 01 00 00 01 1e"
);
assert.equal(
  hex(encodeMessage({ type: "event", id: 2n, name: "hello", data: encode("world") })),
  "02 02 05 68 65 6c 6c 6f 06 05 77 6f 72 6c 64"
);
assert.equal(
  hex(encodeMessage({ type: "stream", id: 3n, name: "chat", seq: 0n, senderTs: 123n, data: encode("hi") })),
  "03 03 04 63 68 61 74 00 7b 03 02 68 69"
);

// 解码回环
const decoded = decodeMessage(encodeMessage({ type: "request", id: 1n, name: "add", data: encode([10, 20]) }));
assert.equal(decoded.type, "request");
assert.equal(decoded.id, 1n);
assert.equal(decoded.name, "add");
assert.equal(hex(decoded.data), "0a 14");

// 帧编码：4 字节长度前缀
const frame = encodeFrame({ type: "event", id: 2n, name: "hello", data: encode("world") });
assert.equal(hex(frame.subarray(0, 4)), "0f 00 00 00"); // 15 字节载荷
assert.equal(hex(frame.subarray(4)), "02 02 05 68 65 6c 6c 6f 06 05 77 6f 72 6c 64");

// Reader 载荷解码
const r = new Reader(encode([10, 20]));
assert.equal(r.varint(), 10);
assert.equal(r.varint(), 20);

console.log("✅ 全部 8 项交叉验证通过");
