# echostream-core

EchoStream 核心框架：RPC / Event / Stream 调度、会话管理与服务端客户端实现，
并内置 QUIC 传输（`quic` 模块，原 echostream-transport 合并而来，默认开启）。

## 功能

- **Server / Client**：Builder 模式，`listener()` / `from_endpoint()` 注入任意传输
- **Session**：单个连接的上下文，双向主动通信
- **Router**：RPC / Event / Stream 处理器注册与分发（支持运行时注册）
- **强类型 Handler**：框架统一编解码，业务只面对具体类型
- **复用通道**：RPC 一条长连接双向流按 id 多路复用（`request_muxed`，`request_raw` 默认），
  事件一条长连接单向流批量帧
- **连接池**：`ClientBuilder::pool(n)` 多连接跨核扩展
- **中间件 / 插件**：数据面拦截转换 + 控制面生命周期扩展
- **QUIC 传输**：自签证书开箱即用、双向/单向流、数据报、大窗口调优

## 快速开始

```rust
use echostream_core::{ServerBuilder, ClientBuilder, Session};
use echostream_proto::Result;

let server = ServerBuilder::new()
    .bind("0.0.0.0:5000")
    .add_rpc(my_handler)
    .build()
    .await?;

let client = ClientBuilder::new()
    .pool(2) // 可选：连接池
    .connect("127.0.0.1:5000")
    .await?;
```

## 特性

- `quic`（默认）：内置 QUIC 传输（quinn）
