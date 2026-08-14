# EchoStream 多端绑定

四端共享同一份 Rust 核心（协议编解码、客户端状态机、RPC/事件/流调度），
**各端使用 API 均自动完成数据编解码** —— 业务侧直接传原生值，无需手写字节。

| 端 | 形态 | 能力 | 自动编解码 |
|----|------|------|-----------|
| **Rust** | crates.io（`echostream`） | 完整 client + server | derive 宏强类型编解码 |
| **Node.js** | npm（`echostream-node`，napi-rs） | 完整 client + server | 纯 JS postcard 编解码（ESM） |
| **Python** | PyPI（`echostream`，PyO3） | 完整 client + server | 纯 Python postcard 编解码 |
| **Web** | `bindings/web`（WebSocket / WebTransport 双传输） | 浏览器 client | WASM 编解码（Rust 单一事实来源） |

## 自动编解码约定（跨端统一）

所有 RPC / Event / Stream 载荷遵循同一编码约定（`echostream-proto::dynamic`）：

| 语言值 | 线缆格式 | Rust 对应 |
|--------|----------|-----------|
| 整数（含负数） | i64 ZigZag varint | `i64` |
| 非负 BigInt / 大整数 | u64 普通 varint | `u64` |
| 浮点数 | f64 小端 8 字节 | `f64` |
| 布尔 | 单字节 0/1 | `bool` |
| 字符串 | varint 长度 + UTF-8 | `String` |
| 字节数组 | varint 长度 + 原始字节 | `Vec<u8>` / `Bytes` |
| 数组 / 多参数 | 元组字段序（无长度前缀） | 元组 / 结构体 |
| 对象 / dict | 结构体字段序（无键名） | 结构体 |
| null / undefined / None | 空载荷 | `()` |

解码默认智能推断；歧义场景（如 Vec 与元组、空字符串与数字 0）可传
显式 schema（`"number" | "string" | "bytes" | "f64" | "f32" | "bool" | "list" | 数组 | 对象`）。

## 各端快速上手

### Rust（derive 宏，强类型）

```rust
use echostream::prelude::*;

#[rpc("add")]
async fn add(_s: &Session, (a, b): (i64, i64)) -> Result<i64> { Ok(a + b) }

let sum: i64 = client.request("add", &(10, 20)).await?; // 30
```

### Node.js（ESM）

```js
import { connect, ServerBuilder } from "echostream-node";

const client = await connect("127.0.0.1:5000");
const sum = await client.request("add", 10, 20); // 30
await client.emit("hello", "world");
client.onEvent("hello", (data) => console.log(data));

const builder = new ServerBuilder();
builder.bind("0.0.0.0:5000");
builder.addRpc("add", async (a, b) => a + b); // 自动解码参数、编码响应
```

### Python

```python
import echostream

client = echostream.connect("127.0.0.1:5000")
total = client.request("add", 10, 20)  # 30
client.emit("hello", "world")

builder = echostream.ServerBuilder()
builder.bind("0.0.0.0:5000")
builder.add_rpc("add", lambda a, b: a + b)
```

### Web（浏览器）

```js
import { EchoStream } from "./echostream.js";

const client = new EchoStream("ws://192.168.1.100:8081");
await client.connect();
const sum = await client.request("add", 10, 20); // 30
```

## 底层 API

各端保留手动字节的底层入口（`request_raw` / `emit_raw` / `native` 导出等），
供高级场景使用；日常开发请使用自动编解码 API。

## 测试

```bash
node bindings/node/test/codec.test.mjs      # Node 纯 JS 编解码 vs WASM 交叉验证
node bindings/node/test/server.test.mjs     # Node server + client 闭环
python3 bindings/python/tests/test_e2e.py   # Python server + client 闭环
node scripts/cross_e2e.mjs                  # 跨端矩阵：Rust ↔ Node ↔ Python 6 组合
```
