# EchoStream

> 基于 QUIC 的高性能异步双向 RPC 与流传输框架，让实时通信像写本地函数一样简单。

## 简介

专为实时通信场景设计的 Rust RPC 框架，通过 QUIC 在单连接上同时处理控制信令与实时数据流，支持客户端与服务端**双向主动通信**：RPC 请求、事件推送、流式数据传输。

## 核心特性

- **双向通信**：客户端和服务端都可以主动发起请求、发送事件和推送流
- **三种模式**：RPC（请求/响应）、Event（单向事件）、Stream（连续数据流）
- **基于 QUIC**：多路复用、0-RTT、自动拥塞控制、TLS 1.3 加密
- **开箱即用**：自动生成自签名证书，开发环境零配置
- **声明式宏**：`#[rpc]` / `#[event]` / `#[stream]`，业务只写强类型函数
- **服务发现**：mDNS 局域网零配置发现（可选）
- **可扩展**：中间件（数据面）+ 插件（控制面）
- **轻量简单**：无过度抽象，错误处理与序列化由框架统一完成

## 快速开始

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

#[stream("chat")]
async fn on_chat(_session: &Session, mut stream: StreamReceiver) -> Result<()> {
    while let Some(frame) = stream.recv().await? {
        println!("帧 #{}: {}", frame.seq, String::from_utf8_lossy(&frame.data));
    }
    Ok(())
}

// 服务端
#[tokio::main]
async fn main() -> Result<()> {
    ServerBuilder::new()
        .bind("0.0.0.0:5000")
        .add_rpc(Add)
        .add_event(OnHello)
        .add_stream(OnChat)
        .serve()
        .await
}

// 客户端
async fn client_demo() -> Result<()> {
    let client = ClientBuilder::new().connect("127.0.0.1:5000").await?;

    // RPC 调用
    let sum: i64 = client.request("add", &(10, 20)).await?;

    // 发送事件
    client.emit("hello", &"world".to_string()).await?;

    // 推送流数据
    let mut stream = client.create_stream("chat").await?;
    stream.send("你好").await?;
    stream.finish()?;
    Ok(())
}
```

完整示例见 `crates/echostream/examples/`：

| 示例 | 内容 |
|------|------|
| `simple_rpc` | RPC / Event / Stream 三种模式端到端 |
| `bidi` | 服务端主动调用客户端、事件监听、中间件、生命周期钩子 |
| `discovery` | mDNS 服务发现与连接 |

## 服务发现

```rust
// 服务端广播
let _advertiser = advertise("echo-server", 5000)?;

// 客户端发现（局域网零配置）
let addrs = discover("echo-server", Duration::from_secs(3)).await?;
let client = ClientBuilder::new().connect(addrs[0]).await?;
```

## 多端支持

四端共享同一份 Rust 核心（协议编解码、客户端状态机、RPC/事件/流调度）：

| 端 | 形态 | 能力 |
|----|------|------|
| **Rust** | crates.io（`echostream`） | 完整 client + server |
| **Node.js** | npm（`echostream-node`，napi-rs） | 完整 client + server，error-first 回调 + Promise |
| **Python** | PyPI（`echostream`，PyO3） | 完整 client + server，同步 API |
| **Web** | `sdk/web`（WebTransport） | 浏览器 client：JS 仅网络层，编解码 + 状态机由 Rust WASM 提供 |

浏览器受平台限制（WebTransport 为纯客户端协议）只提供 client；服务端能力由
Rust / Node / Python 提供。协议编解码与客户端状态机（`echostream-client-core`）
为单一事实来源，各端无需重复实现。

## 架构

```
echostream             统一入口（重导出 + prelude + 宏）
├── echostream-core         框架核心：Server/Client/Session/Router/Handler/中间件/插件
├── echostream-client-core  无 I/O 客户端状态机（RPC 匹配/事件路由/流管理，可编译 WASM）
├── echostream-transport    传输层：QUIC 封装（流、数据报、证书、帧编解码）
├── echostream-proto        协议层：Message/Error（零运行时依赖）
├── echostream-derive       过程宏：#[rpc] / #[event] / #[stream]
├── echostream-discovery    服务发现：mDNS（独立可选）
├── echostream-web          WebTransport 服务端（浏览器直连）
└── bindings/               Node（napi-rs）/ Python（PyO3）/ WASM（wasm-bindgen）
```

通信模型：**每条消息使用独立 QUIC 流**，RPC 走双向流、事件/流走单向流，天然多路复用、背压隔离。

## 开发

```bash
cargo build --workspace   # 编译
cargo run -p echostream --example simple_rpc   # 端到端示例
```

## License

MIT License © 2026-PRESENT [Del Wang](https://del.wang)
