//! EchoStream 核心框架
//!
//! 实现 RPC / Event / Stream 三种通信模式的服务端与客户端，提供：
//! - `Server` / `Client`：开箱即用的服务端与客户端（Builder 模式）
//! - `Session`：单个连接的会话上下文（双向主动通信）
//! - `Router`：RPC / Event / Stream 处理器注册与分发
//! - 强类型 Handler：框架层统一编解码，业务只面对具体类型
//! - 内置 QUIC 传输（feature = "quic"，默认开启）：`quic::QuicEndpoint` /
//!   `quic::connect`，WebSocket / WebTransport 等其它传输由同级 crate 提供
//!
//! 通信模型：RPC 每请求一条双向流（HTTP/3 语义），事件走复用通道（长连接
//! 单向流批量帧）或数据报（不可靠），流走独立单向流 —— 天然多路复用、背压隔离。

pub mod client;
pub mod codec;
pub mod context;
pub mod handler;
pub mod middleware;
pub mod plugin;
pub mod router;
pub mod server;
pub mod session;
pub mod stream;

/// 内置 QUIC 传输（quinn 封装；由原 echostream-transport 合并而来）
#[cfg(feature = "quic")]
pub mod quic;

pub use client::{Client, ClientBuilder};
pub use context::ServerContext;
pub use echostream_proto::endpoint::{
    Endpoint, FrameIo, FrameRead, FrameWrite, Listener, encode_message, read_message_frame,
};
pub use handler::{DynEventHandler, DynRpcHandler, EventHandler, RpcHandler, StreamHandler};
pub use middleware::Middleware;
pub use plugin::{ClientPlugin, ServerPlugin};
pub use router::Router;
pub use server::{Server, ServerBuilder};
pub use session::Session;
pub use stream::{StreamReceiver, StreamSender};
