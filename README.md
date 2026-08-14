# EchoStream

> 基于 QUIC 的高性能异步双向 RPC 与流传输框架 —— 让实时通信像写本地函数一样简单。

EchoStream 通过 QUIC 在单连接上同时承载控制信令与实时数据流，支持客户端与服务端
**双向主动通信**：RPC 请求、事件推送、流式数据传输。四端（Rust / Node / Python / Web）
共享同一份 Rust 核心，**各端使用 API 均自动完成数据编解码**，无需手写任何字节。

## 核心特性

- **双向通信**：客户端和服务端都可以主动发起请求、发送事件和推送流
- **三种模式**：RPC（请求/响应）、Event（单向事件）、Stream（连续数据流）
- **高性能**：RPC 复用通道 + 连接池 + 数据报不可靠通道（见 [docs/BENCHMARK.md](docs/BENCHMARK.md)）
- **多端一致**：Rust / Node / Python / Web 同一协议同一 DX，跨语言互操作开箱即用
- **自动编解码**：各端直接传原生值（数字/字符串/对象/数组），框架自动序列化
- **基于 QUIC**：多路复用、0-RTT、自动拥塞控制、TLS 1.3 加密
- **开箱即用**：自动生成自签名证书，开发环境零配置
- **声明式宏**：`#[rpc]` / `#[event]` / `#[stream]`，业务只写强类型函数
- **可扩展**：中间件（数据面）+ 插件（控制面）+ mDNS 服务发现（可选）

## 快速开始（Rust）

```rust
use echostream::prelude::*;

// 用声明式宏定义处理器，像写本地函数一样
#[rpc("add")]
async fn add(_session: &Session, (a, b): (i64, i64)) -> Result<i64> {
    Ok(a + b)
}

#[event("hello")]
async fn on_hello(session: &Session, msg: String) -> Result<()> {
    println!("[{}] {msg}", session.peer_addr());
    Ok(())
}

// 服务端
#[tokio::main]
async fn main() -> Result<()> {
    let server = ServerBuilder::new()
        .bind("0.0.0.0:5000")     // QUIC 监听器（自动自签证书）
        .add_rpc(Add)
        .add_event(OnHello)
        .serve()
        .await
}

// 客户端
async fn client_demo() -> Result<()> {
    let client = ClientBuilder::new().connect("127.0.0.1:5000").await?;
    let sum: i64 = client.request("add", &(10, 20)).await?;  // 30
    client.emit("hello", &"world".to_string()).await?;
    Ok(())
}
```

## 多端开发体验一致

```js
// Node.js（ESM）：import { connect, ServerBuilder } from "echostream-node";
const client = await connect("127.0.0.1:5000");
const sum = await client.request("add", 10, 20);   // 30，自动编解码
```

```python
# Python：import echostream
client = echostream.connect("127.0.0.1:5000")
total = client.request("add", 10, 20)              # 30，自动编解码
```

```js
// 浏览器：import { EchoStream } from "./echostream.js";
const client = new EchoStream("ws://192.168.1.100:8081");
await client.connect();
const sum = await client.request("add", 10, 20);   // 30，自动编解码
```

## 服务发现（mDNS）

```rust
use echostream::prelude::*;
use std::time::Duration;

// 服务端：广播服务（携带元数据）
let service = ServiceInfo::new("echo-server", 5000)?
    .set_property("version", "0.1.0");
let _advertiser = Discovery::advertise(service)?;   // RAII：drop 后自动停止

// 客户端：零配置发现并连接
let found = Discovery::discover("echo-server", Duration::from_secs(3)).await?;
let client = ClientBuilder::new().connect(found[0].address()).await?;
```

## 文档

| 文档 | 内容 |
|------|------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构分层、模块职责与关键设计 |
| [docs/EXAMPLES.md](docs/EXAMPLES.md) | 全仓示例导航（Rust / Node / Python / Web） |
| [docs/BENCHMARK.md](docs/BENCHMARK.md) | 基准测试方法与结果 |
| [docs/WEB_E2E.md](docs/WEB_E2E.md) | 浏览器端 E2E 与兼容性调研 |
| [bindings/README.md](bindings/README.md) | 多端绑定与自动编解码约定 |

## 架构

```text
echostream                 统一入口（重导出 + prelude + 宏 + QUIC 便捷 bind/connect）
├── echostream-proto            协议层：Message/Error/传输接口/帧编解码/动态值编解码（零运行时依赖）
├── echostream-core             框架核心：Server/Client/Session/Router/Handler/中间件/插件
│                                复用通道/连接池 + 无 I/O 状态机 ClientCore（io feature 控制 tokio，WASM 可编译）
├── echostream-transport        传输层：QUIC（默认）/ WebSocket / WebTransport 三 feature
├── echostream-derive           过程宏：#[rpc] / #[event] / #[stream]
├── echostream-discovery        mDNS 局域网服务发现（可选）
├── plugins/                    基础插件：auth / reconnect / retry
├── middlewares/                基础中间件：logging
└── bindings/                   Node（napi-rs）/ Python（PyO3）/ WASM（wasm-bindgen）/ Web（浏览器 SDK）
```

分层依赖：`proto`（协议）→ `core`（框架，传输无关）→ `transport`（QUIC/WS/WebTransport）→ 扩展与绑定。
框架完全传输无关：`ServerBuilder::listener` / `ClientBuilder::from_endpoint` 注入任意传输。

## 通信模型

- **RPC**：默认走**复用通道**（一条长连接双向流按请求 id 多路复用，高频小请求性能最优）；
  载荷超过 64KiB 自动切换独立双向流（避免队头阻塞）；支持**连接池**（`ClientBuilder::pool(n)`）跨核扩展
- **Event**：复用长连接单向流批量帧（可靠）；数据报通道（不可靠，吞吐最高）
- **Stream**：独立单向流，帧自动编解码，多流并行不受限

## 开发

```bash
cargo build --workspace                        # 编译
cargo test -p echostream-proto -p echostream-core -p echostream-transport   # 测试
cargo run -p echostream --example simple_rpc   # 端到端示例
cargo run -p echostream --example bench --release   # 基准测试
node scripts/cross_e2e.mjs                     # 跨端矩阵：Rust ↔ Node ↔ Python
```

## License

MIT License © 2026-PRESENT [Del Wang](https://del.wang)
