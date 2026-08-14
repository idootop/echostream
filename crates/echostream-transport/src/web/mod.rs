//! WebTransport 服务端：让浏览器通过 WebTransport（HTTP/3 + QUIC）直连 EchoStream 服务端，
//! 复用与原生客户端完全相同的帧协议与处理器体系。

mod server;
mod wt;

pub use server::{WebServer, WebServerBuilder};
