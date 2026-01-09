# echostream-transport

> QUIC 传输层封装,处理底层网络通信

## 模块职责

`echostream-transport` 封装了 Quinn (QUIC 实现),为上层提供统一的传输层抽象:

- **QUIC 封装**: 封装 Quinn 库,处理 QUIC 连接的建立和管理
- **TLS 握手**: 处理 TLS 1.3 安全连接,支持自签名证书和 CA 证书
- **0-RTT 支持**: 快速重连,减少握手延迟
- **多路复用**: 管理 QUIC Streams 和 Datagrams 的多路复用
- **传输抽象**: 为未来扩展其他传输协议 (如 WebTransport) 预留接口

## 设计原则

### 传输层隔离

将网络传输细节与 RPC 逻辑完全分离:
- 上层 `echostream-core` 不直接依赖 `quinn`
- 通过 trait 定义传输层接口
- 支持未来替换为其他传输协议

### 最小化暴露

仅暴露必要的传输层抽象:
- 不暴露 Quinn 的底层 API
- 提供简化的连接、流、数据报接口
- 隐藏 TLS 配置等复杂细节

## 核心接口

### 传输层 Trait

```rust
/// 传输层抽象
#[async_trait]
pub trait Transport: Send + Sync {
    type Connection: Connection;

    /// 监听指定地址
    async fn listen(&self, addr: SocketAddr) -> Result<Listener>;

    /// 连接到远程地址
    async fn connect(&self, addr: SocketAddr) -> Result<Self::Connection>;
}

/// 连接抽象
#[async_trait]
pub trait Connection: Send + Sync {
    /// 打开双向流
    async fn open_bi_stream(&self) -> Result<(SendStream, RecvStream)>;

    /// 接受双向流
    async fn accept_bi_stream(&self) -> Result<(SendStream, RecvStream)>;

    /// 发送数据报
    async fn send_datagram(&self, data: Bytes) -> Result<()>;

    /// 接收数据报
    async fn recv_datagram(&self) -> Result<Bytes>;

    /// 获取对端地址
    fn peer_addr(&self) -> SocketAddr;

    /// 关闭连接
    async fn close(&self) -> Result<()>;
}
```

### QUIC 实现

```rust
use echostream_transport::{QuicTransport, QuicConfig};

// 创建 QUIC 传输层
let config = QuicConfig::builder()
    .with_self_signed_cert()  // 使用自签名证书
    .build()?;

let transport = QuicTransport::new(config);

// 监听
let listener = transport.listen("0.0.0.0:5000".parse()?).await?;

// 接受连接
while let Some(conn) = listener.accept().await? {
    tokio::spawn(async move {
        handle_connection(conn).await;
    });
}
```

### 连接管理

```rust
// 连接到服务器
let conn = transport.connect("127.0.0.1:5000".parse()?).await?;

// 打开流
let (mut send, mut recv) = conn.open_bi_stream().await?;

// 发送数据
send.write_all(b"Hello").await?;
send.finish().await?;

// 接收数据
let data = recv.read_to_end(1024).await?;

// 发送数据报 (无序,不可靠)
conn.send_datagram(Bytes::from("quick message")).await?;
```

## TLS 配置

### 自签名证书 (开发环境)

```rust
let config = QuicConfig::builder()
    .with_self_signed_cert()
    .skip_cert_verification()  // 开发环境跳过验证
    .build()?;
```

### CA 证书 (生产环境)

```rust
let config = QuicConfig::builder()
    .with_cert_chain("cert.pem", "key.pem")
    .with_ca_cert("ca.pem")
    .build()?;
```

## 性能优化

### 0-RTT 握手

```rust
// 服务端启用 0-RTT
let config = QuicConfig::builder()
    .enable_0rtt()
    .build()?;

// 客户端自动使用 0-RTT (如果服务端支持)
let conn = transport.connect(addr).await?;
```

### 拥塞控制

```rust
let config = QuicConfig::builder()
    .congestion_control(CongestionControl::BBR)  // 使用 BBR 算法
    .max_concurrent_streams(1000)                // 最大并发流
    .build()?;
```

## 依赖关系

```
echostream-transport (本模块)
    ├── echostream-proto   - 基础类型
    ├── quinn              - QUIC 实现
    ├── tokio              - 异步运行时
    ├── bytes              - 零拷贝字节操作
    └── tracing            - 日志追踪
```

被以下模块依赖:
- `echostream-core` - 核心抽象层

## 未来扩展

### WebTransport 支持

```rust
// 未来可以添加 WebTransport 实现
pub struct WebTransport { ... }

impl Transport for WebTransport {
    // 实现相同的 trait
}
```

### 自定义传输协议

```rust
// 用户可以实现自己的传输层
pub struct MyCustomTransport { ... }

impl Transport for MyCustomTransport {
    // 自定义实现
}
```

## 注意事项

- 该模块专注于传输层,不包含业务逻辑
- 所有网络 I/O 操作都应在此模块内完成
- 保持接口简洁,避免暴露过多 QUIC 细节
- 错误处理应该清晰,区分网络错误和协议错误
