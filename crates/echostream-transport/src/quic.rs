//! QUIC 连接封装：端点、连接、流与消息帧编解码

use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

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

// ======================== 帧编解码 ========================

/// 编码消息帧：4 字节小端长度前缀 + postcard 载荷
fn encode(msg: &Message) -> Result<Bytes> {
    let payload = postcard::to_allocvec(msg).map_err(|e| Error::Serialization(e.to_string()))?;
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(&payload);
    Ok(Bytes::from(buf))
}

/// 从接收流读取一帧；流正常结束（对端 finish）时返回 `Ok(None)`
async fn read_frame(recv: &mut quinn::RecvStream) -> Result<Option<Message>> {
    let mut len_buf = [0u8; 4];
    match to_echo(recv.read(&mut len_buf).await)? {
        Some(_) => {}
        None => return Ok(None), // 对端正常关闭流
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_SIZE {
        return Err(Error::Protocol(format!("帧大小超限: {len}")));
    }
    let mut buf = vec![0u8; len];
    to_echo(recv.read_exact(&mut buf).await)?;
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
    /// 打开一条双向流（可靠、有序）
    pub async fn open_bi(&self) -> Result<BiStream> {
        let (send, recv) = to_echo(self.conn.open_bi().await)?;
        Ok(BiStream { send, recv })
    }

    /// 打开一条单向发送流（用于事件/流数据推送）
    pub async fn open_uni(&self) -> Result<UniSend> {
        Ok(UniSend {
            send: to_echo(self.conn.open_uni().await)?,
        })
    }

    /// 接受对端打开的双向流
    pub async fn accept_bi(&self) -> Result<BiStream> {
        let (send, recv) = to_echo(self.conn.accept_bi().await)?;
        Ok(BiStream { send, recv })
    }

    /// 接受对端打开的单向接收流
    pub async fn accept_uni(&self) -> Result<UniRecv> {
        Ok(UniRecv {
            recv: to_echo(self.conn.accept_uni().await)?,
        })
    }

    /// 发送数据报（不可靠、无序，适合实时音视频帧）
    pub fn send_datagram(&self, data: Bytes) -> Result<()> {
        to_echo(self.conn.send_datagram(data))
    }

    /// 接收数据报
    pub async fn recv_datagram(&self) -> Result<Bytes> {
        to_echo(self.conn.read_datagram().await)
    }

    /// 对端地址
    pub fn peer_addr(&self) -> SocketAddr {
        self.conn.remote_address()
    }

    /// 关闭连接
    pub fn close(&self) {
        self.conn.close(0u32.into(), b"closed");
    }
}

// ======================== 流封装 ========================

/// 双向流：帧级读写
pub struct BiStream {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

impl BiStream {
    /// 写入一帧
    pub async fn write_message(&mut self, msg: &Message) -> Result<()> {
        let frame = encode(msg)?;
        to_echo(self.send.write_all(&frame).await)?;
        Ok(())
    }

    /// 读取一帧；流结束返回 `Ok(None)`
    pub async fn read_message(&mut self) -> Result<Option<Message>> {
        read_frame(&mut self.recv).await
    }

    /// 关闭发送端（对端读到流结束）
    pub fn finish(&mut self) -> Result<()> {
        self.send.finish().map_err(|_| Error::SessionClosed)
    }

    /// 拆分为独立的发送端与接收端（并行读写）
    pub fn split(self) -> (UniSend, UniRecv) {
        (UniSend { send: self.send }, UniRecv { recv: self.recv })
    }
}

/// 单向发送流
pub struct UniSend {
    send: quinn::SendStream,
}

impl UniSend {
    /// 写入一帧
    pub async fn write_message(&mut self, msg: &Message) -> Result<()> {
        let frame = encode(msg)?;
        to_echo(self.send.write_all(&frame).await)?;
        Ok(())
    }

    /// 关闭发送端
    pub fn finish(&mut self) -> Result<()> {
        self.send.finish().map_err(|_| Error::SessionClosed)
    }
}

/// 单向接收流
pub struct UniRecv {
    recv: quinn::RecvStream,
}

impl UniRecv {
    /// 读取一帧；流结束返回 `Ok(None)`
    pub async fn read_message(&mut self) -> Result<Option<Message>> {
        read_frame(&mut self.recv).await
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
