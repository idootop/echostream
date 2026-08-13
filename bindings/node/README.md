# echostream-node

EchoStream 的 Node.js 绑定（napi-rs）—— 基于 QUIC 的高性能双向 RPC / Event / Stream 框架。

完整 client + server 能力，与 Rust 核心共享同一份实现。

## 安装与构建

```bash
# 构建原生模块（需要 Rust 工具链）
cargo build -p echostream-node --release
cp target/release/libechostream_node.dylib bindings/node/echostream-node.node   # macOS
# Linux: cp target/release/libechostream_node.so bindings/node/echostream-node.node

npm install   # 或直接 require 本目录
```

## 服务端

```js
const { ServerBuilder } = require("echostream-node");

const builder = new ServerBuilder();
builder.bind("0.0.0.0:5000");

// 回调遵循 Node error-first 约定：(err, data) => ...
builder.addRpc("add", async (err, payload) => {
  const sum = /* 解码载荷并计算 */;
  return Buffer.from(/* 编码响应 */);
});
builder.addEvent("hello", (err, payload) => console.log("事件:", payload));
builder.addStream("chat", async (err, receiver) => {
  while ((frame = await receiver.recv()) !== null) { /* 处理流帧 */ }
});

const server = await builder.build();
server.run();          // 后台运行
server.shutdown();     // 优雅关闭
```

## 客户端

```js
const { connect } = require("echostream-node");

const client = await connect("127.0.0.1:5000");
const resp = await client.request("add", Buffer.from(...));
await client.emit("hello", Buffer.from(...));
const stream = await client.createStream("chat");
await stream.send(Buffer.from(...));
await stream.finish();
client.close();
```

## 载荷约定

所有 RPC / Event / Stream 载荷为 **postcard 编码字节**（与 Rust 线缆格式一致），
可用 `sdk/web/wasm` 的编解码模块或 `echostream`（WASM）生成。

## 测试

```bash
node test/index.test.cjs    # client 端到端（需 chat_server 示例）
node test/server.test.cjs   # server + client 进程内闭环
```
