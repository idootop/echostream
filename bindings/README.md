# EchoStream Bindings（多语言绑定）

所有语言绑定统一位于本目录：

| 目录 | 技术 | 说明 |
|------|------|------|
| `node/` | napi-rs | Node.js 绑定（完整 client + server，Rust crate） |
| `python/` | PyO3 | Python 绑定（完整 client + server，Rust crate） |
| `wasm/` | wasm-bindgen | 协议编解码 + 无 I/O 状态机（Rust crate，供 Web/Node 复用） |
| `web/` | 纯 JS | 浏览器 SDK（WebSocket/WebTransport 双传输，依赖 wasm 产物） |

分层：`wasm/`（Rust 编译产物）→ `web/`（JS SDK 消费）；`node/`、`python/` 为独立原生绑定。

Web SDK 构建（wasm 产物）：
```bash
cargo build -p echostream-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir bindings/web/wasm target/wasm32-unknown-unknown/release/echostream_wasm.wasm
wasm-bindgen --target nodejs --out-dir bindings/wasm/node target/wasm32-unknown-unknown/release/echostream_wasm.wasm
```
