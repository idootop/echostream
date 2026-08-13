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

## 载荷编码约定（跨端线缆格式）

RPC / 事件载荷 = **postcard 序列化**（与 Rust 核心一致），各端调用方负责编解码：

| 内容 | 编码 | 示例 |
|------|------|------|
| RPC 请求/响应 | postcard（**i64 用 ZigZag varint**：10 → `0x14`；元组无长度前缀） | `(i64,i64)(10,20)` → `14 28` |
| 事件 | postcard String（varint 长度 + UTF-8） | `"hi"` → `02 68 69` |
| 流帧 | 原始字节（无编码） | — |

> ⚠️ 注意：postcard 的 `i64` 是 ZigZag varint，**不是** u64 普通 varint（10 → `0x0a`）。
> Node 端可用 wasm 的 `encode_i64`/`decode_i64` 原语；Python 端参考
> `tests/test_e2e.py` 中的 `encode_i64`/`decode_i64` 实现。

跨端验证：`bash tools/e2e/cross_matrix.sh`（Rust ↔ Node ↔ Python 6 组合，全部 PASS）。
