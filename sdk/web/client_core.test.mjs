// ClientCore 状态机交叉验证：RPC 匹配/事件路由/服务端主动调用
// 使用 Node 加载 wasm 产物，模拟网络层直接喂帧
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const wasm = require("../../bindings/wasm/node/echostream_wasm.js");
const { ClientCoreHandle, encode_payload, encode_frame, decode_message, decode_u64, decode_string } = wasm;

const core = new ClientCoreHandle();
console.log("✅ 状态机创建");

// ===== RPC 请求/响应匹配 =====
let respValue = null;
const reqFrame = core.request("add", encode_payload([10, 20]), (data) => {
  respValue = decode_u64(data);
});
const req = decode_message(reqFrame.subarray(4));
assert.equal(req.type, "request");
assert.equal(req.name, "add");
console.log(`✅ 请求帧构造: id=${req.id} name=${req.name}`);

// 模拟服务端响应（错误 id 的响应应被忽略）
core.handle_inbound(encode_frame({ type: "response", id: 999n, code: 0, message: null, data: encode_payload(1) }));
assert.equal(respValue, null, "错误 id 不应触发回调");
// 正确响应
core.handle_inbound(encode_frame({ type: "response", id: req.id, code: 0, message: null, data: encode_payload(30) }));
assert.equal(respValue, 30);
console.log("✅ RPC 响应匹配（错误 id 忽略 + 正确 id 触发）");

// ===== 事件路由 =====
const events = [];
core.on_event("hello", (name, data) => {
  events.push([name, decode_string(data)]);
});
core.handle_inbound(encode_frame({ type: "event", id: 1n, name: "hello", data: encode_payload("world") }));
assert.deepEqual(events, [["hello", "world"]]);
console.log("✅ 事件路由到监听器");

// ===== 服务端主动调用（同步响应） =====
core.on_rpc("ping", (name, data) => {
  return encode_payload("pong:" + decode_string(data));
});
const outbound = core.handle_inbound(encode_frame({ type: "request", id: 7n, name: "ping", data: encode_payload("hi") }));
assert.ok(outbound, "应返回响应帧");
const resp = decode_message(outbound.subarray(4));
assert.equal(resp.type, "response");
assert.equal(resp.id, 7);
assert.equal(decode_string(resp.data), "pong:hi");
console.log("✅ 服务端主动调用（状态机自动回响应）");

// ===== 流序号管理 =====
const sid = core.open_stream("chat");
const f0 = decode_message(core.build_stream_frame(sid, "chat", encode_payload("a"), 1n).subarray(4));
const f1 = decode_message(core.build_stream_frame(sid, "chat", encode_payload("b"), 2n).subarray(4));
assert.equal(f0.seq, 0);
assert.equal(f1.seq, 1);
console.log("✅ 流序号自动递增（状态机管理）");

console.log("🎉 ClientCore 状态机全部验证通过");
