# EchoStream Web SDK（浏览器）

基于 **WebSocket**（局域网零证书）与 **WebTransport**（公网可信证书）的浏览器客户端 SDK，
与 Rust 服务端互通，支持 RPC / Event / Stream 三种模式，**载荷自动编解码**。

## 架构

协议编解码与客户端状态机由 **Rust 编译的 WASM**（`bindings/wasm` crate）提供，
JS 只负责网络层 —— 与 Rust 服务端的线缆格式单一事实来源。浏览器平台限制
（无法监听端口）决定了 Web 端只提供客户端能力；服务端能力由 Rust / Node / Python 提供。

## 使用

```html
<script type="module">
  import { EchoStream } from "./dist/echostream.js";

  const client = new EchoStream("ws://192.168.1.100:8081");  // 或 https://host:4433
  await client.connect();

  const sum = await client.request("add", 10, 20);   // 30，自动编解码
  await client.emit("hello", "world");
  client.onEvent("hello", (data) => console.log(data));
  client.onRpc("ping", async () => "pong");          // 双向通信

  const stream = await client.createStream("chat");
  await stream.send("frame-1");
  await stream.finish();
  client.onStream("notice", async (stream) => {
    let frame;
    while ((frame = await stream.recv()) !== null) console.log(frame);
  });
</script>
```

## 编解码约定（与 Rust postcard 线缆格式兼容）

| JS 值 | Rust 类型 | 编码 |
|-------|-----------|------|
| 整数（含负数） | `i64` | ZigZag varint |
| 非负 BigInt | `u64` | 普通 varint |
| 浮点数 | `f64` | 小端 8 字节 |
| 布尔 | `bool` | 单字节 |
| 字符串 | `String` | 长度前缀 + UTF-8 |
| Uint8Array | `Vec<u8>` | 长度前缀 + 字节 |
| 数组 / 多参数 | 元组 / 结构体 | 顺序编码（无长度前缀） |
| 对象 | 结构体 | 字段序（无键名） |

解码默认智能推断；歧义场景传 schema：`client.request("get", { id: 1 }, { decode: { id: "number" } })`
或 `{ decode: "string" | "bytes" | "f64" | "list" | ... }`。

## 浏览器支持

- WebSocket：Chrome / Firefox / Safari 全支持，局域网零证书开箱即用
- WebTransport：Chrome / Edge 原生支持；自签名证书需先访问 `https://host:port` 信任；
  生产环境使用受信任 CA 证书（浏览器兼容性调研见 [docs/WEB_E2E.md](../../docs/WEB_E2E.md)）

## 文件

- `echostream.ts` — SDK 入口源码（tsdown 构建到 `dist/echostream.js`）
- `wasm/` — Rust 编译的协议核心（编解码 + 状态机）
- `index.html` / `e2e.html` — 浏览器 demo 与 E2E 页面
- `echostream.test.mjs` / `client_core.test.mjs` — 编解码与状态机交叉验证
- `e2e.sdk.test.mjs` — SDK 端到端（Node 环境驱动，需 Rust ws_chat_server）

## 重新构建 WASM 模块

```bash
cargo build -p echostream-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir bindings/web/wasm target/wasm32-unknown-unknown/release/echostream_wasm.wasm
wasm-bindgen --target nodejs --out-dir bindings/wasm/node target/wasm32-unknown-unknown/release/echostream_wasm.wasm
```
