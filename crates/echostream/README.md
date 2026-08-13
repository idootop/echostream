# echostream

基于 QUIC 的高性能异步双向 RPC 与流传输框架 —— 让实时通信像写本地函数一样简单。

统一入口：重导出核心框架（Server/Client/Session/Router/Handler）、协议类型、
`#[rpc]` / `#[event]` / `#[stream]` 声明式宏、mDNS 服务发现。

## 快速开始

```rust
use echostream::prelude::*;

#[rpc("add")]
async fn add(_session: &Session, (a, b): (u64, u64)) -> Result<u64> {
    Ok(a + b)
}

#[tokio::main]
async fn main() -> Result<()> {
    // 服务端
    let server = ServerBuilder::new()
        .bind("0.0.0.0:5000")
        .add_rpc(Add)
        .serve()
        .await?;

    // 客户端（同一进程或另一进程）
    let client = ClientBuilder::new().connect("127.0.0.1:5000").await?;
    let sum: u64 = client.request("add", &(10, 20)).await?;
    Ok(())
}
```

完整文档见仓库根目录 README；示例见 `crates/echostream/examples/`。
