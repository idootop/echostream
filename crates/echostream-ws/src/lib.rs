//! EchoStream WebSocket 服务端端点
//!
//! 解决局域网 Web 端通信的证书死结：`ws://` 无需 TLS 证书，浏览器零门槛。
//! 帧协议与 QUIC 完全一致（长度前缀 + postcard Message），Web SDK 只需更换网络层。
//!
//! 使用方式与 `echostream::ServerBuilder` / `WebServerBuilder` 一致：

mod server;

pub use server::{WsServer, WsServerBuilder};
