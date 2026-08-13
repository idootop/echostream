//! # EchoStream
//!
//! 基于 QUIC 的高性能异步双向 RPC 与流传输框架。
//!
//! 统一入口：重导出核心框架、协议类型与过程宏。

pub use echostream_core::*;
pub use echostream_proto::{Error, EventMsg, Message, RequestMsg, ResponseMsg, Result, StatusCode, StreamMsg, Timestamp};

/// 常用类型预导入
pub mod prelude {
    pub use crate::*;
    pub use async_trait::async_trait;
    pub use serde::{Deserialize, Serialize};
}
