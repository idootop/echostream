# echostream-core

EchoStream 核心框架：RPC / Event / Stream 调度、会话管理与服务端客户端实现，
完全**传输无关**（具体传输由 echostream-transport 提供）。

## 功能

- Server / Client：Builder 模式，`listener()` / `endpoint()` 注入任意传输
- Session：单个连接的上下文，双向主动通信
- Router：RPC / Event / Stream 处理器注册与分发（Token 注册/移除 + 运行时管理）
- 生命周期回调：Server/Client 的 on_connect / on_disconnect 支持 HookId 注册与取消注册
- 强类型 Handler：框架统一编解码，业务只面对具体类型
- 复用通道：RPC 长连接双向流按 id 多路复用；事件长连接单向流批量帧
- 连接池：`ClientBuilder::pool(n)` 多连接跨核扩展
- 中间件 / 插件：洋葱链中间件（Next 包裹下游）+ 控制面生命周期扩展（插件可打包任意处理器与钩子）
- ClientCore：无 I/O 客户端状态机（RPC 匹配 / 事件路由 / 流管理，WASM 可编译）

## 特性

- `io`（默认）：启用 tokio 运行时相关模块（Server/Client/Session 等）；
  关闭后仅保留 ClientCore 与编解码（可编译 WASM）

## 快速开始

```rust
use echostream_core::{ServerBuilder, ClientBuilder, Session};
use echostream_proto::Result;

let server = ServerBuilder::new()
    .listener(my_listener)          // 注入任意传输监听器
    .add_rpc(my_handler)
    .build()
    .await?;

let client = ClientBuilder::new()
    .pool(2)                        // 可选：连接池
    .endpoint(my_conn)              // 注入任意传输连接
    .build()?;                      // 未注入连接时返回错误
// 或单步便捷：ClientBuilder::new().from_endpoint(my_conn)
```

> QUIC 便捷用法见 `echostream-transport`（`ServerBuilderExt::bind` / `ClientBuilderExt::connect`）。
