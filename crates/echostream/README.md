# echostream

基于 QUIC 的高性能异步双向 RPC 与流传输框架 —— 让实时通信像写本地函数一样简单。

统一入口：重导出核心框架（Server/Client/Session/Router/Handler）、协议类型、
`#[rpc]` / `#[event]` / `#[stream]` 声明式宏、mDNS 服务发现与 QUIC 便捷 API。

## 快速开始

```rust
use echostream::prelude::*;

// Session 参数可省略（需要会话时再写 session: &Session）
#[rpc("add")]
async fn add((a, b): (i64, i64)) -> Result<i64> {
    Ok(a + b)
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = ServerBuilder::new()
        .bind("0.0.0.0:5000")     // QUIC 监听器（自动自签证书）
        .add_rpc(Add)
        .build()
        .await?;
    tokio::spawn(async move { server.run().await });

    let client = ClientBuilder::new().connect("127.0.0.1:5000").await?;
    let sum: i64 = client.request("add", &(10, 20)).await?;
    println!("add(10, 20) = {sum}"); // 30
    Ok(())
}
```

## 能力

- RPC / Event / Stream 三种通信模式，客户端与服务端双向主动通信
- RPC 复用通道（高频小请求）+ 独立流（大载荷）+ 连接池（跨核扩展）
- 事件复用通道（可靠）+ 数据报（不可靠，吞吐最高）
- 强类型 Handler：`#[rpc]` / `#[event]` / `#[stream]` 自动编解码
- 中间件（数据面）与插件（控制面）扩展机制
- mDNS 局域网零配置服务发现：

```rust
use echostream::prelude::*;
use std::time::Duration;

let service = ServiceInfo::new("echo-server", 5000)?
    .set_property("version", "0.1.0");
let _advertiser = Discovery::advertise(service)?;

let found = Discovery::discover("echo-server", Duration::from_secs(3)).await?;
let client = ClientBuilder::new().connect(found[0].address()).await?;
```

## 特性

- `derive`（默认）：`#[rpc]` / `#[event]` / `#[stream]` 过程宏
- `discovery`（可选）：mDNS 服务发现（extensions/echostream-discovery；示例：cargo run -p echostream-discovery --example discovery）
- `file`：文件流传输（echostream::file：FileSender / recv_file 等）
- `av`：音视频推流/接收（echostream::av：AvSender / AvReceiver 等）

## 多端

Node（npm `echostream-node`）/ Python（PyPI `echostream`）/ Web（浏览器 SDK）
与 Rust 共享同一协议核心，使用 API 均自动编解码，详见仓库 `bindings/README.md`。
