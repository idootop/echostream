//! EchoStream WebTransport 服务端
//!
//! 让浏览器通过 WebTransport（HTTP/3 + QUIC）直连 EchoStream 服务端，
//! 复用与原生客户端完全相同的帧协议与处理器体系（`#[rpc]` / `#[event]` / `#[stream]`）。
//!
//! 使用方式与 `echostream::ServerBuilder` 一致，只需换成 `WebServerBuilder`。

mod server;
mod wt;

pub use server::{WebServer, WebServerBuilder};
