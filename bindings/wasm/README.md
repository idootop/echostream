# echostream-wasm

EchoStream 的 WASM 绑定（wasm-bindgen）—— 协议编解码 + 无 I/O 客户端状态机。

供 Web SDK 与 Node 复用：与 Rust 服务端的线缆格式与核心逻辑**单一事实来源**，
协议演进无需跨语言同步修改。

## 构建

```bash
cargo build -p echostream-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir bindings/web/wasm target/wasm32-unknown-unknown/release/echostream_wasm.wasm
wasm-bindgen --target nodejs --out-dir bindings/wasm/node target/wasm32-unknown-unknown/release/echostream_wasm.wasm
```

## API

- **自动编解码**：`encode_payload`（JS 值 → 字节）/ `decode_payload`（字节 → JS 值，
  支持 schema 精确解码）/ `encode_i64` / `decode_i64` / `decode_u64` / `decode_string` / `decode_bytes`
- **消息帧**：`encode_message` / `decode_message` / `encode_frame`
- **状态机**：`ClientCoreHandle`（`request` / `build_event` / `open_stream` /
  `build_stream_frame` / `build_response` / `build_error_response` / `handle_inbound` /
  `on_event` / `on_rpc` / `on_stream`）

## 测试

```bash
node bindings/web/echostream.test.mjs    # 编解码交叉验证（与 Rust 基准字节一致）
node bindings/web/client_core.test.mjs   # 状态机验证
node bindings/node/test/codec.test.mjs   # Node 纯 JS 编解码 vs WASM 交叉验证
```
