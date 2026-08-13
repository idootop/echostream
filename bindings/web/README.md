# EchoStream Web SDK（浏览器）

基于 **WebTransport**（HTTP/3 + QUIC）的浏览器客户端 SDK，与 Rust 服务端
（`echostream-web` crate）互通，支持 RPC / Event / Stream 三种模式。

## 架构

协议编解码由 **Rust 编译的 WASM**（`bindings/wasm` crate）提供，JS 只负责
WebTransport 网络层 —— 与 Rust 服务端的线缆格式单一事实来源，协议演进
无需跨语言同步修改。浏览器平台限制（无法监听端口）决定了 Web 端只提供
客户端能力；服务端能力由 Rust / Node / Python 提供。

## 使用

```html
<script type="module">
  import { EchoStream } from "./echostream.js";

  const client = new EchoStream("https://your-server:4433");
  client.onEvent("hello", (data) => console.log(new TextDecoder().decode(data)));
  await client.connect();

  // RPC：载荷为数组时按 Rust 元组/结构体顺序编码（无长度前缀）
  const resp = await client.request("add", [10, 20]);
  // Event
  await client.emit("join", "alice");
  // Stream
  const stream = await client.createStream("chat");
  await stream.send("frame-1");
  await stream.finish();
</script>
```

## 编解码约定（与 Rust postcard 线缆格式兼容）

| JS 值 | Rust 类型 | 编码 |
|-------|-----------|------|
| 非负 number / bigint | `u32` / `u64` | 无符号 varint |
| 负数 number / bigint | `i32` / `i64` | zigzag varint |
| string | `String` | 长度前缀 + UTF-8 |
| Uint8Array | `Bytes` | 长度前缀 + 字节 |
| 数组 | 元组 / 结构体字段 | 顺序编码（无长度前缀） |

> 注意：Rust 结构体与 JS 对象字段顺序需一致；`Vec` 场景请自行加长度前缀
> （`encodeList` 可后续补充）。

## 浏览器支持

- Chrome / Edge：原生支持 WebTransport
- 自签名证书：先访问 `https://host:port` 信任证书，再连接
- 生产环境：使用受信任 CA 证书

## 文件

- `echostream.js` — SDK 入口（连接 / RPC / Event / Stream / 事件监听）
- `wasm/` — Rust 编译的协议编解码模块（`echostream_wasm.js` + `.wasm`，由 `bindings/wasm` 构建）
- `index.html` — 浏览器 demo
- `echostream.test.mjs` — 与 Rust 侧交叉验证的编解码测试（`node echostream.test.mjs`）

## 重新构建 WASM 模块

```bash
cargo build -p echostream-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir sdk/web/wasm target/wasm32-unknown-unknown/release/echostream_wasm.wasm
wasm-bindgen --target nodejs --out-dir bindings/wasm/node target/wasm32-unknown-unknown/release/echostream_wasm.wasm
```
