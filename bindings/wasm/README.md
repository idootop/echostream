# echostream-wasm

EchoStream 的 WASM 绑定（wasm-bindgen）—— 协议编解码 + 无 I/O 客户端状态机。

供 Web SDK 与 Node 使用：与 Rust 服务端的线缆格式与核心逻辑**单一事实来源**，
协议演进无需跨语言同步修改。

## 构建

```bash
cargo build -p echostream-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir sdk/web/wasm target/wasm32-unknown-unknown/release/echostream_wasm.wasm
wasm-bindgen --target nodejs --out-dir bindings/wasm/node target/wasm32-unknown-unknown/release/echostream_wasm.wasm
```

## API

- **编解码**：`encode_payload` / `encode_message` / `decode_message` / `encode_frame` / `decode_u64` / `decode_string` / `decode_bytes`
- **状态机**：`ClientCoreHandle`（`request` / `build_event` / `open_stream` / `build_stream_frame` / `handle_inbound` / `on_event` / `on_rpc`）

## 测试

```bash
node sdk/web/echostream.test.mjs    # 编解码交叉验证（与 Rust 基准字节一致）
node sdk/web/client_core.test.mjs   # 状态机验证（RPC 匹配/事件路由/主动调用/流序号）
```
