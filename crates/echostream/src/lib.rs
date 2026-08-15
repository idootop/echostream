//! # EchoStream
//!
//! 基于 QUIC 的高性能异步双向 RPC 与流传输框架。
//!
//! 统一入口：重导出核心框架、协议类型、过程宏与 QUIC 便捷 API（bind/connect）。

pub use async_trait::async_trait;
pub use echostream_core::*;
pub use echostream_proto::{
    Error, EventMsg, Message, RequestMsg, ResponseMsg, Result, StatusCode, StreamEndMsg,
    StreamMetaEntry, StreamMsg, StreamOpenMsg, Timestamp, stream_flags,
};
/// QUIC 便捷 API：ServerBuilder::bind / ClientBuilder::connect（echostream-transport）
pub use echostream_transport::{ClientBuilderExt, ServerBuilderExt};

/// 过程宏（feature = "derive"）
#[cfg(feature = "derive")]
pub use echostream_derive::{event, rpc, stream};

/// 服务发现（feature = "discovery"）
#[cfg(feature = "discovery")]
pub use echostream_discovery::{Advertiser, Discovery, ServiceInfo};

/// 常用类型预导入
pub mod prelude {
    pub use crate::*;
    pub use serde::{Deserialize, Serialize};
}
