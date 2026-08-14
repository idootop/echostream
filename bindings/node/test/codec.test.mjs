// Node 纯 JS 编解码（postcard.js）与 WASM 编解码（bindings/wasm）交叉验证
// 保证两套实现字节级一致，是跨语言自动编解码一致性的基石。
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { encodePayload, decodePayload } from "../postcard.js";

const require = createRequire(import.meta.url);
const wasm = require("../../wasm/node/echostream_wasm.js");

const hex = (b) => Buffer.from(b).toString("hex");
const toBytes = (v) => (v instanceof Uint8Array ? v : Uint8Array.from(v));

// ===== 编码字节一致性（JS codec vs WASM codec） =====
const cases = [
  ["整数", 10],
  ["负数", -7],
  ["浮点", 1.5],
  ["布尔 true", true],
  ["布尔 false", false],
  ["字符串", "hello"],
  ["中文", "你好世界"],
  ["空串", ""],
  ["BigInt", 10n],
  ["负 BigInt", -10n],
  ["零", 0],
  ["字节", new Uint8Array([0xff, 0xfe, 0x01])],
  ["空载荷", undefined],
  ["元组", [10, 20]],
  ["字符串元组", ["a", "b"]],
  ["结构体", { a: 1, b: "x" }],
  ["嵌套", [1, [2, "y"]]],
];
for (const [label, value] of cases) {
  const js = hex(encodePayload(value));
  const wm = hex(toBytes(wasm.encode_payload(value === undefined ? null : value)));
  assert.equal(js, wm, `编码不一致: ${label}`);
  console.log(`✅ 编码一致 [${label}]: ${js}`);
}

// ===== 解码一致性（智能推断 + schema） =====
const decodeCases = [
  [30, 30],
  ["hello", "hello"],
  [-7, -7],
  [[10, 20], [10, 20]],
  [["a", "b"], ["a", "b"]],
  [1.5, 1.5, "f64"],
  [true, true, "bool"],
  [10n, 10, "u64"],
];
for (const [value, expected, schema] of decodeCases) {
  const bytes = encodePayload(value);
  const js = decodePayload(bytes, schema);
  assert.deepEqual(js, expected, `解码不一致: ${String(value)}`);
  const wm = wasm.decode_payload(toBytes(bytes), schema || undefined);
  assert.deepEqual(wm, expected, `WASM 解码不一致: ${String(value)}`);
  console.log(`✅ 解码一致 [${String(value)}] -> ${JSON.stringify(expected)}`);
}

// ===== 与 WASM decode 交叉验证（同字节互解） =====
const x = encodePayload([1, "hi", 3.5]);
assert.deepEqual(wasm.decode_payload(toBytes(x)), decodePayload(x));
const y = toBytes(wasm.encode_payload({ name: "echo", count: 42 }));
assert.deepEqual(decodePayload(y, { name: "string", count: "number" }), { name: "echo", count: 42 });
console.log("✅ 交叉互解一致");

console.log("🎉 Node 纯 JS 编解码与 WASM 编解码完全一致");
