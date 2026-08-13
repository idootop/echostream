//! 传输抽象：连接/流接口与帧编解码
//!
//! 框架核心与具体传输（QUIC / WebSocket / WebTransport）解耦：
//! 本模块定义接口，各传输 crate（echostream-transport / echostream-ws /
//! echostream-web）实现，实现同一套帧协议（长度前缀 + postcard Message）。
//! 零运行时依赖（无 tokio / quinn）。

use std::net::SocketAddr;
use std::sync::Arc;

use crate::{Error, Message, Result};
use async_trait::async_trait;
use bytes::Bytes;

/// 单帧最大字节数（防止恶意长度导致内存膨胀）
pub const MAX_FRAME_SIZE: usize = 64 * 1024 * 1024;

/// 帧级流读写抽象（适配 QUIC / WebSocket / WebTransport 等不同传输的流）
///
/// 只读流（单向接收）调用 `write_message`/`finish` 返回错误，
/// 只写流（单向发送）调用 `read_message` 返回错误。
#[async_trait]
pub trait FrameIo: Send {
    /// 写入一帧
    async fn write_message(&mut self, msg: &Message) -> Result<()>;
    /// 读取一帧；流正常结束返回 `Ok(None)`
    async fn read_message(&mut self) -> Result<Option<Message>>;
    /// 关闭发送端
    async fn finish(&mut self) -> Result<()>;
}

/// 连接抽象（适配 QUIC / WebTransport / WebSocket 等不同传输的连接）
#[async_trait]
pub trait Endpoint: Send + Sync + 'static {
    /// 类型擦除访问（供传输实现内部使用）
    fn as_any(&self) -> &dyn std::any::Any;

    /// 打开双向流
    async fn open_bi(&self) -> Result<Box<dyn FrameIo>>;
    /// 打开单向发送流
    async fn open_uni(&self) -> Result<Box<dyn FrameIo>>;
    /// 接受双向流
    async fn accept_bi(&self) -> Result<Box<dyn FrameIo>>;
    /// 接受单向流
    async fn accept_uni(&self) -> Result<Box<dyn FrameIo>>;
    /// 对端地址
    fn peer_addr(&self) -> SocketAddr;
    /// 关闭连接
    fn close(&self);

    /// 是否支持数据报（不可靠通道）
    fn supports_datagram(&self) -> bool {
        false
    }

    /// 发送数据报（不可靠、无序，适合高频事件/音视频帧；默认不支持）
    fn send_datagram(&self, _data: Bytes) -> Result<()> {
        Err(Error::Protocol("数据报通道未启用".into()))
    }

    /// 接收数据报（默认不支持）
    async fn recv_datagram(&self) -> Result<Bytes> {
        Err(Error::Protocol("数据报通道未启用".into()))
    }
}

/// 监听器抽象（适配 QUIC / WebTransport / WebSocket 等不同传输的服务端）
#[async_trait]
pub trait Listener: Send + Sync + 'static {
    /// 接受下一个连接
    async fn accept(&self) -> Option<Arc<dyn Endpoint>>;
    /// 本地监听地址
    fn local_addr(&self) -> Result<SocketAddr>;
    /// 关闭监听
    fn close(&self);
}

/// 流读取抽象（适配 QUIC / WebTransport 的接收流）
#[async_trait]
pub trait FrameRead: Send {
    /// 读取数据；流结束返回 `Ok(None)`
    async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>>;
    /// 读取完整数据
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()>;
}

/// 流写入抽象（适配 QUIC / WebTransport 的发送流）
#[async_trait]
pub trait FrameWrite: Send {
    /// 写入完整数据
    async fn write_all(&mut self, buf: &[u8]) -> Result<()>;
    /// 关闭发送端
    fn finish(&mut self) -> Result<()>;
}

/// 编码消息帧：4 字节小端长度前缀 + postcard 载荷
pub fn encode_message(msg: &Message) -> Result<Bytes> {
    let payload = postcard::to_allocvec(msg).map_err(|e| Error::Serialization(e.to_string()))?;
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(&payload);
    Ok(Bytes::from(buf))
}

/// 从接收流读取一帧；流正常结束（对端 finish）时返回 `Ok(None)`
pub async fn read_message_frame<R: FrameRead + ?Sized>(recv: &mut R) -> Result<Option<Message>> {
    let mut len_buf = [0u8; 4];
    match recv.read(&mut len_buf).await? {
        Some(_) => {}
        None => return Ok(None), // 对端正常关闭流
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_SIZE {
        return Err(Error::Protocol(format!("帧大小超限: {len}")));
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;
    let msg = postcard::from_bytes(&buf).map_err(|e| Error::Serialization(e.to_string()))?;
    Ok(Some(msg))
}
