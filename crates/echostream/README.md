# echostream

基于 QUIC 的高性能异步双向 RPC 和流传输框架 - 统一入口

## Features

- `default`: [derive, discovery]
- `derive`: 过程宏,提供更便利的开发体验
- `discovery`: mDNS 服务发现,支持局域网零配置

## 使用示例

```rust
use echostream::prelude::*;

// 服务端
#[echostream::rpc]
async fn hello(name: String) -> Result<String> {
    Ok(format!("你好, {}!", name))
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = EchoServer::builder()
        .listen("0.0.0.0:5000")
        .add_rpc(hello)
        .build()?;

    server.run().await
}

// 客户端
#[tokio::main]
async fn main() -> Result<()> {
    let client = EchoClient::connect("127.0.0.1:5000").await?;
    let response: String = client.call("hello", "世界").await?;
    println!("{}", response); // 你好, 世界!
    Ok(())
}
```

## 模块架构

EchoStream 采用分层设计,各模块职责清晰:

### [echostream-proto](../echostream-proto/README.md)

底层协议定义、Wire Format 和基础类型:

- 消息帧格式 (Frame, Header, MessageType)
- Session 和时间戳类型
- 错误定义和 Result 类型
- **零依赖设计**: 不依赖异步运行时和网络库

### [echostream-transport](../echostream-transport/README.md)

QUIC 传输层封装,处理底层网络通信:

- 封装 Quinn (QUIC 实现)
- TLS 握手和 0-RTT 支持
- Streams/Datagrams 多路复用
- 传输层抽象 (支持未来扩展 WebTransport)

### [echostream-core](../echostream-core/README.md)

核心抽象层,实现框架的主要功能:

- RPC 调度器 (Request/Response 生命周期管理)
- 插件系统 (生命周期 Hook: on_connect, on_close)
- 中间件系统 (Tower-like Layer 和 Service)
- 时间同步 (内置 NTP 算法)
- 流处理和抖动缓冲

### [echostream-derive](../echostream-derive/README.md)

过程宏,简化开发体验:

- `#[rpc]`: RPC 处理器
- `#[event]`: 事件监听器
- `#[stream]`: 流数据处理器
- `#[derive(Plugin)]`: 插件派生宏

### [echostream-discovery](../echostream-discovery/README.md)

基于 mDNS 的局域网服务发现:

- 服务注册和广播
- 服务发现和解析
- 零配置网络

## 依赖关系

```
echostream (统一入口)
    ├── echostream-core
    │   ├── echostream-transport
    │   │   └── echostream-proto
    │   └── echostream-proto
    ├── echostream-derive
    └── echostream-discovery
        └── echostream-proto
```

## 设计理念

### 分层解耦

- **proto**: 稳定的协议层,所有模块共享
- **transport**: 传输层隔离,支持替换为其他协议
- **core**: 核心逻辑层,不直接依赖网络库
- **derive**: 编译时代码生成,零运行时开销

### 插件优先

- **中间件 (Middleware)**: 处理消息流 (鉴权、日志、限流)
- **插件 (Plugin)**: 处理连接生命周期 (统计、重连、服务发现)
