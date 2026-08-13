// EchoStream WASM 编解码交叉验证
// 基准字节来自 Rust 侧（crates/echostream/examples/codec_probe.rs）
import assert from "node:assert/strict";
import wasm from "../wasm/node/echostream_wasm.js";

const { encode_payload, encode_message, decode_message, encode_frame, decode_u64, decode_string } = wasm;

const hex = (bytes) => Array.from(bytes).map((b) => b.toString(16).padStart(2, "0")).join(" ");

// 载荷编码与 Rust postcard 一致
assert.equal(hex(encode_payload("hello")), "05 68 65 6c 6c 6f");
assert.equal(hex(encode_payload([10, 20])), "0a 14"); // (10u64, 20u64)
assert.equal(hex(encode_payload(42)), "2a");
assert.equal(hex(encode_payload(-1)), "01"); // zigzag

// Message 编码与 Rust 一致
assert.equal(
  hex(encode_message({ type: "request", id: 1n, name: "add", data: encode_payload([10, 20]) })),
  "00 01 03 61 64 64 02 0a 14"
);
assert.equal(
  hex(encode_message({ type: "response", id: 1n, code: 0, message: null, data: encode_payload(30) })),
  "01 01 00 00 01 1e"
);
assert.equal(
  hex(encode_message({ type: "event", id: 2n, name: "hello", data: encode_payload("world") })),
  "02 02 05 68 65 6c 6c 6f 06 05 77 6f 72 6c 64"
);
assert.equal(
  hex(encode_message({ type: "stream", id: 3n, name: "chat", seq: 0n, senderTs: 123n, data: encode_payload("hi") })),
  "03 03 04 63 68 61 74 00 7b 03 02 68 69"
);

// 解码回环
const decoded = decode_message(encode_message({ type: "request", id: 1n, name: "add", data: encode_payload([10, 20]) }));
assert.equal(decoded.type, "request");
assert.equal(decoded.id, 1);
assert.equal(decoded.name, "add");
assert.equal(hex(decoded.data), "0a 14");

// 帧编码：4 字节长度前缀
const frame = encode_frame({ type: "event", id: 2n, name: "hello", data: encode_payload("world") });
assert.equal(hex(frame.subarray(0, 4)), "0f 00 00 00");
assert.equal(hex(frame.subarray(4)), "02 02 05 68 65 6c 6c 6f 06 05 77 6f 72 6c 64");

// 载荷解码原语
assert.equal(decode_u64(encode_payload(30)), 30);
assert.equal(decode_string(encode_payload("world")), "world");

console.log("✅ WASM 编解码交叉验证通过（与 Rust 线缆格式一致）");
