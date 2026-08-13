//! EchoStream 传输层 —— 基于 QUIC 的可靠/不可靠传输封装
//!
//! 本模块封装 quinn（QUIC 实现），提供：
//! - 服务端端点（自动生成自签名证书或指定 CA 证书）
//! - 客户端连接（开发模式跳过证书验证）
//! - 双向流 / 单向流（可靠、有序）
//! - 数据报（不可靠、无序，适合音视频帧）
//! - 消息帧编解码（长度前缀 + postcard 序列化）
//!
//! 上层（echostream-core）只与 `Message` 对象交互，不接触任何底层网络细节。

pub mod cert;
pub mod quic;

pub use quic::{BiStream, QuicConn, QuicEndpoint, UniRecv, UniSend, connect};
