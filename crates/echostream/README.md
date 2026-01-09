# echostream

统一入口 crate，重导出所有公共 API，提供 `prelude` 模块。

## Features

- `default`: 默认开启 discovery
- `discovery`: 启用局域网服务自动发现功能

## 使用示例

```rust
use echostream::prelude::*;

// 服务端
#[echostream::rpc("hello")]
async fn hello(session: Session, name: String) -> Result<String> {
    Ok(format!("Hello, {}!", name))
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = EchoServer::builder()
        .bind("0.0.0.0:5000")
        .add_rpc(hello)
        .build()?;
    server.run().await
}

// 客户端
#[tokio::main]
async fn main() -> Result<()> {
    let client = EchoClient::connect("127.0.0.1:5000").await?;
    let response: String = client.request("hello", "World").await?;
    println!("{}", response); // Hello, World!
    Ok(())
}
```

## 子模块

### [echostream-core](../echostream-core/README.md)

核心框架，实现 RPC 和流传输能力:

- 连接管理、协议层、RPC 框架
- 流管理、插件系统
- 服务端/客户端实现

### [echostream-types](../echostream-types/README.md)

公共类型、错误定义和工具函数:

- 错误类型、上下文类型
- Session 会话、时间戳类型

### [echostream-derive](../echostream-derive/README.md)

过程宏，简化处理器定义:

- `handler` 宏：请求处理器
- `listener` 宏：事件监听器
- `stream_handler` 宏：流处理器

### [echostream-discovery](../echostream-discovery/README.md)

基于 mDNS 的局域网服务发现:

- 服务广播、服务发现
- 服务解析、零配置
