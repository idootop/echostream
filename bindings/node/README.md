# echostream-node

EchoStream 的 Node.js 绑定（napi-rs，ESM）—— 基于 QUIC 的高性能双向 RPC / Event / Stream 框架。

完整 client + server 能力，与 Rust 核心共享同一份实现；**载荷自动编解码**（纯 JS postcard 实现，
与 WASM 字节级交叉验证），业务侧无需手写任何字节。

## 安装与构建

```bash
cargo build -p echostream-node --release
cp target/release/libechostream_node.dylib bindings/node/echostream-node.node   # macOS
# Linux: cp target/release/libechostream_node.so bindings/node/echostream-node.node
```

## 客户端

```js
import { connect } from "echostream-node";

const client = await connect("127.0.0.1:5000");
const sum = await client.request("add", 10, 20);   // 30，自动编解码
await client.emit("hello", "world");
client.onEvent("hello", (data) => console.log(data));
client.onRpc("ping", async () => "pong");          // 双向通信

const stream = await client.createStream("chat");
await stream.send("hi");
await stream.finish();
client.close();
```

## 服务端

```js
import { ServerBuilder } from "echostream-node";

const builder = new ServerBuilder();
builder.bind("0.0.0.0:5000");
builder.addRpc("add", async (a, b) => a + b);        // 参数自动解码、返回值自动编码
builder.addEvent("hello", (data) => console.log(data));
builder.addStream("chat", async (receiver) => {
  let frame;
  while ((frame = await receiver.recv()) !== null) console.log(frame);
});

const server = await builder.build();
await server.run();
```

## 编解码约定

整数 → i64 ZigZag varint；BigInt → u64 varint；浮点 → f64；布尔 → 单字节；
字符串/字节 → 长度前缀；数组/多参数 → 元组字段序；对象 → 结构体字段序。
解码默认智能推断；歧义场景传选项：`client.request("get", { id: 1 }, { decode: { id: "number" } })`。

底层手动字节 API 通过 `nativeApi` 导出。

## 测试

```bash
npm test                 # codec 交叉验证 + server/client 闭环
npm run test:cross       # 跨端矩阵（需 Rust/Python 对端）
```

## 示例

```bash
pnpm build                    # 先构建 TypeScript（tsdown -> dist/）
node dist/examples/server.js   # 终端 1
node dist/examples/client.js   # 终端 2
```
