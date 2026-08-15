# echostream-transport

EchoStream 传输层 —— 实现框架的传输抽象（proto 的 Endpoint / FrameIo / Listener），
所有传输帧协议一致（长度前缀 + postcard Message），上层框架完全传输无关。

## 传输与特性

| 传输 | feature | 说明 |
|------|---------|------|
| QUIC | quic（默认） | quinn 封装：自签证书开箱即用、双向/单向流、数据报、便捷 bind/connect |
| WebSocket | ws | 局域网 Web 端零证书服务端（浏览器直连） |
| WebTransport | web | 公网浏览器服务端（HTTP/3） |

```toml
[dependencies]
echostream-transport = { version = "0.1", features = ["ws"] }  # 按需开启
```

## QUIC 便捷 API（默认）

框架的 ServerBuilder / ClientBuilder 本身传输无关；本 crate 提供扩展：

```rust
use echostream::prelude::*; // 或 use echostream_transport::{ServerBuilderExt, ClientBuilderExt};

let server = ServerBuilder::new()
    .bind("0.0.0.0:5000")        // QUIC 监听器（自动自签证书）
    .add_rpc(Add)
    .serve()
    .await?;

let client = ClientBuilder::new()
    .pool(4)                     // 连接池（可选）
    .connect("127.0.0.1:5000")   // QUIC 连接
    .await?;
```

## WebSocket 服务端（feature = "ws"）

```rust
use echostream_transport::ws::WsServerBuilder;

let server = WsServerBuilder::new()
    .bind("0.0.0.0:8081")
    .add_rpc(Add)
    .add_event(OnHello)
    .serve()
    .await?;
```

## WebTransport 服务端（feature = "web"）

```rust
use echostream_transport::web::WebServerBuilder;

let server = WebServerBuilder::new()
    .bind("0.0.0.0:4433")
    .add_rpc(Add)
    .build()
    .await?;
```

## 自定义传输

实现 proto 的 Endpoint / FrameIo / Listener 后，通过
ServerBuilder::listener() / ClientBuilder::endpoint().build() 注入即可。
