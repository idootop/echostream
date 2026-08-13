//! EchoStream 核心框架
//!
//! 基于 `echostream-transport` 实现 RPC / Event / Stream 三种通信模式的
//! 服务端与客户端，提供：
//! - `Server` / `Client`：开箱即用的服务端与客户端（Builder 模式）
//! - `Session`：单个连接的会话上下文（双向主动通信）
//! - `Router`：RPC / Event / Stream 处理器注册与分发
//! - 强类型 Handler：框架层统一编解码，业务只面对具体类型
//!
//! 通信模型：每条消息（RPC 请求/响应、事件、流）使用独立的 QUIC 流，
//! 天然多路复用、背压隔离，协议简单对称。

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

pub use client::{Client, ClientBuilder};
pub use context::ServerContext;
pub use handler::{EventHandler, RpcHandler, StreamHandler};
pub use middleware::Middleware;
pub use plugin::ServerPlugin;
pub use server::{Server, ServerBuilder};
pub use session::Session;
pub use stream::{StreamReceiver, StreamSender};
