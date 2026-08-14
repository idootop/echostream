//! EchoStream 核心框架
//!
//! 实现 RPC / Event / Stream 三种通信模式的服务端与客户端，提供：
//! - Server / Client：服务端与客户端（Builder 模式，传输无关）
//! - Session：单个连接的会话上下文（双向主动通信）
//! - Router：RPC / Event / Stream 处理器注册与分发
//! - 强类型 Handler：框架层统一编解码，业务只面对具体类型
//! - ClientCore：无 I/O 客户端状态机（WASM/绑定复用，不依赖运行时）
//!
//! 通信模型：RPC 复用通道（长连接双向流按 id 多路复用）+ 大载荷独立流；
//! 事件走复用通道（长连接单向流批量帧）或数据报（不可靠）；流走独立单向流。
//!
//! 具体传输（QUIC / WebSocket / WebTransport）由 echostream-transport 提供，
//! 本框架通过 ServerBuilder::listener / ClientBuilder::from_endpoint 注入；
//! feature = "io"（默认）启用 tokio 运行时相关模块，关闭后可编译 WASM。

/// 无 I/O 客户端状态机（RPC 匹配 / 事件路由 / 流管理，WASM 可编译）
mod client_core;
pub use client_core::ClientCore;

/// 载荷编解码：统一使用 postcard
pub mod codec;

// ==================== I/O 框架（feature = "io"，默认开启） ====================

#[cfg(feature = "io")]
pub mod client;
#[cfg(feature = "io")]
pub mod context;
#[cfg(feature = "io")]
pub mod handler;
#[cfg(feature = "io")]
pub mod middleware;
#[cfg(feature = "io")]
pub mod plugin;
#[cfg(feature = "io")]
pub mod router;
#[cfg(feature = "io")]
pub mod server;
#[cfg(feature = "io")]
pub mod session;
#[cfg(feature = "io")]
pub mod stream;

#[cfg(feature = "io")]
pub use client::{Client, ClientBuilder};
#[cfg(feature = "io")]
pub use context::ServerContext;
#[cfg(feature = "io")]
pub use handler::{DynEventHandler, DynRpcHandler, EventHandler, RpcHandler, StreamHandler};
#[cfg(feature = "io")]
pub use middleware::Middleware;
#[cfg(feature = "io")]
pub use plugin::{ClientPlugin, ServerPlugin};
#[cfg(feature = "io")]
pub use router::Router;
#[cfg(feature = "io")]
pub use server::{Server, ServerBuilder};
#[cfg(feature = "io")]
pub use session::Session;
#[cfg(feature = "io")]
pub use stream::{StreamReceiver, StreamSender};

// ==================== 协议层重导出（传输接口 + 帧编解码） ====================

pub use echostream_proto::endpoint::{
    Endpoint, FrameIo, FrameRead, FrameWrite, Listener, encode_message, read_message_frame,
};
