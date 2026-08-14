// EchoStream WASM 编解码交叉验证
// 基准字节来自 Rust 侧（echostream-proto::dynamic + postcard）
import assert from "node:assert/strict";
import wasm from "../wasm/node/echostream_wasm.js";

const {
  encode_payload, decode_payload, encode_message, decode_message, encode_frame,
  decode_u64, decode_i64, decode_string, decode_bytes, encode_i64,
} = wasm;

const hex = (bytes) => Array.from(bytes).map((b) => b.toString(16).padStart(2, "0")).join(" ");

// ===== 载荷编码（i64 ZigZag 跨端约定：10 -> 0x14） =====
assert.equal(hex(encode_payload("hello")), "05 68 65 6c 6c 6f");
assert.equal(hex(encode_payload([10, 20])), "14 28"); // (i64, i64) ZigZag
assert.equal(hex(encode_payload(42)), "54");          // 42 的 ZigZag 为 84 = 0x54
assert.equal(hex(encode_payload(-1)), "01");          // zigzag
assert.equal(hex(encode_payload(1.5)), "00 00 00 00 00 00 f8 3f"); // f64 LE
assert.equal(hex(encode_payload(true)), "01");
assert.equal(hex(encode_payload(10n)), "0a");         // BigInt -> u64 普通 varint
assert.equal(hex(encode_payload({ a: 1, b: "x" })), "02 01 78"); // 结构体字段序
assert.equal(hex(encode_payload(new Uint8Array([1, 2]))), "02 01 02"); // 字节长度前缀

// ===== 载荷解码（智能推断 + schema） =====
assert.equal(decode_payload(encode_payload(30)), 30);
assert.equal(decode_payload(encode_payload("hello")), "hello");
assert.equal(decode_payload(encode_payload(-7)), -7);
assert.deepEqual(decode_payload(encode_payload([10, 20])), [10, 20]);
// 结构体歧义场景（数字+字符串字段）：显式 schema 精确解码
assert.deepEqual(
  decode_payload(encode_payload({ a: 1, b: "x" }), { a: "number", b: "string" }),
  { a: 1, b: "x" }
);
assert.equal(decode_payload(encode_payload(1.5), "f64"), 1.5);
assert.equal(decode_payload(new Uint8Array([0x00, 0x00, 0xc0, 0x3f]), "f32"), 1.5); // f32 LE 1.5
assert.equal(decode_payload(encode_payload(true), "bool"), true);
assert.equal(decode_payload(encode_payload(10n), "u64"), 10);
assert.equal(decode_payload(encode_payload(12345678901234567890n), "u64"), 12345678901234567890n); // BigInt -> u64
// 字节
const bytes = decode_payload(encode_payload(new Uint8Array([0xff, 0xfe]))); // 非法 UTF-8 -> 字节
assert.ok(bytes instanceof Uint8Array && bytes[0] === 0xff && bytes[1] === 0xfe);
// 列表（长度前缀）
assert.deepEqual(decode_payload(encode_payload(0), "list"), []);
// 空载荷
assert.equal(decode_payload(new Uint8Array(0)), undefined);

// ===== 编解码原语（兼容） =====
assert.equal(hex(encode_i64(10n)), "14");
assert.equal(decode_i64(encode_i64(-99n)), -99);
assert.equal(decode_u64(encode_payload(10n)), 10);
assert.equal(decode_string(encode_payload("world")), "world");
assert.equal(hex(decode_bytes(encode_payload(new Uint8Array([9, 8])))), "09 08");

// ===== Message 编码与 Rust postcard 一致 =====
assert.equal(
  hex(encode_message({ type: "request", id: 1n, name: "add", data: encode_payload([10, 20]) })),
  "00 01 03 61 64 64 02 14 28"
);
assert.equal(
  hex(encode_message({ type: "response", id: 1n, code: 0, message: null, data: encode_payload(30) })),
  "01 01 00 00 01 3c"
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
assert.equal(hex(decoded.data), "14 28");

// 帧编码：4 字节长度前缀
const frame = encode_frame({ type: "event", id: 2n, name: "hello", data: encode_payload("world") });
assert.equal(hex(frame.subarray(0, 4)), "0f 00 00 00");
assert.equal(hex(frame.subarray(4)), "02 02 05 68 65 6c 6c 6f 06 05 77 6f 72 6c 64");

console.log("✅ WASM 编解码交叉验证通过（与 Rust 线缆格式一致）");
