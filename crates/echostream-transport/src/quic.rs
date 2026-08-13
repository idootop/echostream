//! QUIC 连接封装：端点、连接、流与消息帧编解码

use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use echostream_proto::{Error, Message, Result};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use crate::cert;

/// 单帧最大字节数（防止恶意长度导致内存膨胀）
const MAX_FRAME_SIZE: usize = 64 * 1024 * 1024;

// ======================== 错误转换 ========================

/// 本地错误转换 trait（避免孤儿规则限制）
trait ToEcho: Sized {
    fn to_echo(self) -> Error;
}

/// 将带外部错误的 Result 转换为 proto::Result
fn to_echo<T, E: ToEcho>(r: std::result::Result<T, E>) -> Result<T> {
    r.map_err(ToEcho::to_echo)
}

impl ToEcho for quinn::ConnectionError {
    fn to_echo(self) -> Error {
        match self {
            quinn::ConnectionError::ConnectionClosed(_)
            | quinn::ConnectionError::ApplicationClosed(_)
            | quinn::ConnectionError::Reset
            | quinn::ConnectionError::TimedOut
            | quinn::ConnectionError::LocallyClosed => Error::SessionClosed,
            other => Error::Io(other.to_string()),
        }
    }
}

impl ToEcho for quinn::ReadError {
    fn to_echo(self) -> Error {
        match self {
            quinn::ReadError::Reset(_)
            | quinn::ReadError::ConnectionLost(_)
            | quinn::ReadError::ClosedStream
            | quinn::ReadError::ZeroRttRejected => Error::SessionClosed,
            other => Error::Io(other.to_string()),
        }
    }
}

impl ToEcho for quinn::ReadExactError {
    fn to_echo(self) -> Error {
        match self {
            quinn::ReadExactError::FinishedEarly(_) => Error::Protocol("流意外结束".into()),
            quinn::ReadExactError::ReadError(e) => e.to_echo(),
        }
    }
}

impl ToEcho for quinn::WriteError {
    fn to_echo(self) -> Error {
        match self {
            quinn::WriteError::Stopped(_)
            | quinn::WriteError::ConnectionLost(_)
            | quinn::WriteError::ClosedStream
            | quinn::WriteError::ZeroRttRejected => Error::SessionClosed,
        }
    }
}

impl ToEcho for quinn::ConnectError {
    fn to_echo(self) -> Error {
        Error::Io(self.to_string())
    }
}

impl ToEcho for quinn::SendDatagramError {
    fn to_echo(self) -> Error {
        match self {
            quinn::SendDatagramError::ConnectionLost(e) => e.to_echo(),
            quinn::SendDatagramError::TooLarge => {
                Error::InvalidParameter("数据报超过最大尺寸".into())
            }
            quinn::SendDatagramError::UnsupportedByPeer | quinn::SendDatagramError::Disabled => {
                Error::Protocol("数据报通道未启用".into())
            }
        }
    }
}

// ======================== 传输抽象 ========================

/// 帧级流读写抽象（适配 QUIC / WebTransport 等不同传输的流）
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
}

// ======================== 帧编解码 ========================

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

/// 构造传输配置（启用 datagram 通道 + 保活）
fn transport_config() -> Arc<quinn::TransportConfig> {
    let mut cfg = quinn::TransportConfig::default();
    cfg.keep_alive_interval(Some(Duration::from_secs(10)));
    cfg.datagram_receive_buffer_size(Some(64));
    cfg.datagram_send_buffer_size(64);
    Arc::new(cfg)
}

/// 确保 rustls CryptoProvider 已安装（幂等）
fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

// ======================== 服务端端点 ========================

/// 服务端端点（监听 + 接受连接，Clone 共享）
#[derive(Clone)]
pub struct QuicEndpoint {
    endpoint: quinn::Endpoint,
}

impl QuicEndpoint {
    /// 绑定监听地址，自动生成自签名证书（开发环境开箱即用）
    pub async fn bind(addr: impl ToSocketAddrs) -> Result<Self> {
        let (certs, key) = cert::self_signed()?;
        Self::bind_with_cert(addr, certs, key).await
    }

    /// 绑定监听地址，使用指定的 CA 证书（生产环境）
    pub async fn bind_with_cert(
        addr: impl ToSocketAddrs,
        certs: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> Result<Self> {
        let addr = addr
            .to_socket_addrs()
            .map_err(|e| Error::Io(e.to_string()))?
            .next()
            .ok_or_else(|| Error::InvalidParameter("无法解析监听地址".into()))?;

        ensure_crypto_provider();

        let mut server_config = quinn::ServerConfig::with_single_cert(certs, key)
            .map_err(|e| Error::Io(e.to_string()))?;
        server_config.transport_config(transport_config());

        let endpoint =
            quinn::Endpoint::server(server_config, addr).map_err(|e| Error::Io(e.to_string()))?;
        tracing::debug!("QUIC 服务端已监听: {addr}");
        Ok(Self { endpoint })
    }

    /// 接受下一个客户端连接
    pub async fn accept(&self) -> Option<QuicConn> {
        loop {
            let incoming = self.endpoint.accept().await?;
            match incoming.await {
                Ok(conn) => return Some(QuicConn { conn }),
                Err(e) => tracing::debug!("连接握手失败: {e}"),
            }
        }
    }

    /// 本地监听地址
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.endpoint
            .local_addr()
            .map_err(|e| Error::Io(e.to_string()))
    }

    /// 关闭端点
    pub fn close(&self) {
        self.endpoint.close(0u32.into(), b"server closed");
    }
}

// ======================== 连接 ========================

/// QUIC 连接（Clone 共享）
#[derive(Clone)]
pub struct QuicConn {
    conn: quinn::Connection,
}

impl QuicConn {
    /// 发送数据报（不可靠、无序，适合实时音视频帧）
    pub fn send_datagram(&self, data: Bytes) -> Result<()> {
        to_echo(self.conn.send_datagram(data))
    }

    /// 接收数据报
    pub async fn recv_datagram(&self) -> Result<Bytes> {
        to_echo(self.conn.read_datagram().await)
    }

    /// 底层 QUIC 连接
    pub fn raw(&self) -> &quinn::Connection {
        &self.conn
    }
}

#[async_trait]
impl Endpoint for QuicConn {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn open_bi(&self) -> Result<Box<dyn FrameIo>> {
        let (send, recv) = to_echo(self.conn.open_bi().await)?;
        Ok(Box::new(BiStream { send, recv }))
    }

    async fn open_uni(&self) -> Result<Box<dyn FrameIo>> {
        Ok(Box::new(UniSend {
            send: to_echo(self.conn.open_uni().await)?,
        }))
    }

    async fn accept_bi(&self) -> Result<Box<dyn FrameIo>> {
        let (send, recv) = to_echo(self.conn.accept_bi().await)?;
        Ok(Box::new(BiStream { send, recv }))
    }

    async fn accept_uni(&self) -> Result<Box<dyn FrameIo>> {
        Ok(Box::new(UniRecv {
            recv: to_echo(self.conn.accept_uni().await)?,
        }))
    }

    fn peer_addr(&self) -> SocketAddr {
        self.conn.remote_address()
    }

    fn close(&self) {
        self.conn.close(0u32.into(), b"closed");
    }
}

#[async_trait]
impl FrameRead for quinn::RecvStream {
    async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>> {
        to_echo(self.read(buf).await)
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        to_echo(self.read_exact(buf).await)
    }
}

#[async_trait]
impl FrameWrite for quinn::SendStream {
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        to_echo(self.write_all(buf).await)
    }

    fn finish(&mut self) -> Result<()> {
        self.finish().map_err(|_| Error::SessionClosed)
    }
}

// ======================== 流封装 ========================

/// 双向流：帧级读写
pub struct BiStream {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

impl BiStream {
    /// 拆分为独立的发送端与接收端（并行读写）
    pub fn split(self) -> (UniSend, UniRecv) {
        (UniSend { send: self.send }, UniRecv { recv: self.recv })
    }
}

#[async_trait]
impl FrameIo for BiStream {
    async fn write_message(&mut self, msg: &Message) -> Result<()> {
        let frame = encode_message(msg)?;
        to_echo(self.send.write_all(&frame).await)?;
        Ok(())
    }

    async fn read_message(&mut self) -> Result<Option<Message>> {
        read_message_frame(&mut self.recv).await
    }

    async fn finish(&mut self) -> Result<()> {
        self.send.finish().map_err(|_| Error::SessionClosed)
    }
}

/// 单向发送流
pub struct UniSend {
    send: quinn::SendStream,
}

#[async_trait]
impl FrameIo for UniSend {
    async fn write_message(&mut self, msg: &Message) -> Result<()> {
        let frame = encode_message(msg)?;
        to_echo(self.send.write_all(&frame).await)?;
        Ok(())
    }

    async fn read_message(&mut self) -> Result<Option<Message>> {
        Err(Error::Protocol("单向发送流不支持读取".into()))
    }

    async fn finish(&mut self) -> Result<()> {
        self.send.finish().map_err(|_| Error::SessionClosed)
    }
}

/// 单向接收流
pub struct UniRecv {
    recv: quinn::RecvStream,
}

#[async_trait]
impl FrameIo for UniRecv {
    async fn write_message(&mut self, _msg: &Message) -> Result<()> {
        Err(Error::Protocol("单向接收流不支持写入".into()))
    }

    async fn read_message(&mut self) -> Result<Option<Message>> {
        read_message_frame(&mut self.recv).await
    }

    async fn finish(&mut self) -> Result<()> {
        Err(Error::Protocol("单向接收流不支持关闭发送端".into()))
    }
}

// ======================== 客户端连接 ========================

/// 连接到服务端（开发模式：跳过证书验证）
pub async fn connect(addr: SocketAddr) -> Result<QuicConn> {
    ensure_crypto_provider();

    let tls = cert::insecure_client_config()?;
    let quic_config = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .map_err(|e| Error::Io(e.to_string()))?;
    let mut client_config = quinn::ClientConfig::new(Arc::new(quic_config));
    client_config.transport_config(transport_config());

    let mut endpoint = quinn::Endpoint::client(
        "[::]:0"
            .parse::<SocketAddr>()
            .map_err(|e| Error::Io(e.to_string()))?,
    )?;
    endpoint.set_default_client_config(client_config);
    let conn = endpoint
        .connect(addr, "localhost")
        .map_err(|e| Error::Io(e.to_string()))?
        .await
        .map_err(ToEcho::to_echo)?;
    Ok(QuicConn { conn })
}
