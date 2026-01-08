# echostream

统一入口 crate，重导出所有公共 API。

## 功能

- 重导出所有核心 API
- 提供统一的 `prelude` 模块
- 可选的服务发现功能（通过 `discovery` feature 开关）

## 依赖

- `echostream-core`: 核心 RPC 和流传输功能
- `echostream-types`: 公共类型和错误定义
- `echostream-derive`: 过程宏支持
- `echostream-discovery`: 可选的 mDNS 服务发现

## Features

- `discovery`: 启用局域网服务自动发现功能

## 使用示例

```rust
use echostream::prelude::*;

#[echostream::handler("hello")]
async fn hello(session: Session, name: String) -> Result<String> {
    Ok(format!("Hello, {}!", name))
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = RpcServer::builder()
        .bind("0.0.0.0:5000")
        .handler(hello)
        .build()?;

    server.run().await
}
```
