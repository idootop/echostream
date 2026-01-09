# echostream-discovery

基于 mDNS 的局域网服务发现。

## 功能列表

- **服务广播**: 在局域网内广播服务信息
- **服务发现**: 自动发现局域网内的服务
- **服务解析**: 解析服务地址和端口信息
- **零配置**: 无需手动配置 IP 地址和端口

## 子模块划分

- `advertiser.rs`: 服务广播实现
- `resolver.rs`: 服务发现和解析
- `service.rs`: 服务信息定义

## 技术栈

- `mdns-sd`: mDNS 协议实现
- `tokio`: 异步运行时
- `anyhow`: 错误处理

## 核心 API 设计

### 服务广播

在服务端启用服务发现，自动在局域网内广播服务信息。

```rust
use echostream::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // 启用服务发现
    let server = EchoServer::builder()
        .bind("0.0.0.0:5000")
        .enable_discovery("MyService")  // 广播服务名称
        .build()?;

    // 服务会自动在局域网内广播
    // 其他设备可以通过 "MyService" 发现此服务
    server.run().await
}
```

### 服务发现

客户端自动发现局域网内的服务。

```rust
use echostream::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // 方式1: 自动发现并连接（局域网）
    let client = EchoClient::discover("MyService").await?;

    // 方式2: 手动指定地址（公网或已知 IP）
    let client = EchoClient::connect("192.168.1.100:5000").await?;

    // 使用客户端
    let response = client.request("method", payload).await?;

    Ok(())
}
```

### 服务信息

```rust
use echostream_discovery::{ServiceInfo, ServiceDiscovery};

// 创建服务信息
let service = ServiceInfo {
    name: "AudioService".into(),
    port: 5000,
    properties: vec![
        ("version".into(), "1.0".into()),
        ("protocol".into(), "quic".into()),
    ],
};

// 广播服务
let advertiser = ServiceDiscovery::advertise(service).await?;

// 发现服务
let resolver = ServiceDiscovery::new();
let services = resolver.discover("AudioService", timeout).await?;

for service in services {
    println!("发现服务: {} at {}:{}",
        service.name,
        service.address,
        service.port
    );
}
```

### 服务属性

可以在服务广播时附加额外的属性信息。

```rust
use echostream::prelude::*;

let server = EchoServer::builder()
    .bind("0.0.0.0:5000")
    .enable_discovery("MyService")
    .service_property("version", "1.0.0")
    .service_property("region", "us-west")
    .service_property("capacity", "100")
    .build()?;

// 客户端可以根据属性筛选服务
let resolver = ServiceDiscovery::new();
let services = resolver
    .discover("MyService")
    .with_property("region", "us-west")
    .with_property("version", "1.0.0")
    .resolve()
    .await?;
```

## 使用场景

### 场景 1: 开发调试

开发时无需手动配置 IP，服务自动发现。

```rust
// 服务端
let server = EchoServer::builder()
    .bind("0.0.0.0:5000")
    .enable_discovery("DevService")
    .build()?;

// 客户端
let client = EchoClient::discover("DevService").await?;
```

### 场景 2: 本地网络应用

局域网内的应用自动发现和连接。

```rust
// 音频服务器
let server = EchoServer::builder()
    .bind("0.0.0.0:5000")
    .enable_discovery("AudioServer")
    .build()?;

// 音频客户端
let client = EchoClient::discover("AudioServer").await?;
let stream = client.create_stream("audio.stream").await?;
```

### 场景 3: 多服务实例

发现并连接到多个服务实例（负载均衡、故障转移）。

```rust
let resolver = ServiceDiscovery::new();
let services = resolver.discover_all("MyService").await?;

// 连接到所有实例
let clients = futures::future::join_all(
    services.iter().map(|s|
        EchoClient::connect(&format!("{}:{}", s.address, s.port))
    )
).await;

// 负载均衡
let client = select_client_round_robin(&clients);
```

## 注意事项

- mDNS 仅适用于局域网环境
- 公网部署需要手动指定地址：`EchoClient::connect("example.com:5000")`
- 服务发现有网络延迟，建议设置合理的超时时间
- 防火墙可能阻止 mDNS 流量（UDP 5353 端口）
