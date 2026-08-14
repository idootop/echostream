//! wtransport 连接与流的适配：实现 transport 的 `Endpoint` / `FrameIo` / `FrameRead` / `FrameWrite`

use std::net::SocketAddr;

use async_trait::async_trait;
use bytes::Bytes;
use echostream_core::{Endpoint, FrameIo, FrameRead, encode_message, read_message_frame};
use echostream_proto::{Error, Message, Result};

/// wtransport 连接适配（实现 `Endpoint`，可直接用于 `Session`）
#[derive(Clone)]
pub(crate) struct WtConn {
    conn: wtransport::Connection,
}

impl WtConn {
    pub(crate) fn new(conn: wtransport::Connection) -> Self {
        Self { conn }
    }
}

fn wt_conn_err(e: wtransport::error::ConnectionError) -> Error {
    Error::Io(e.to_string())
}

#[async_trait]
impl Endpoint for WtConn {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn open_bi(&self) -> Result<Box<dyn FrameIo>> {
        let streams = self
            .conn
            .open_bi()
            .await
            .map_err(wt_conn_err)?
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
        Ok(Box::new(WtBi::new(streams)))
    }

    async fn open_uni(&self) -> Result<Box<dyn FrameIo>> {
        let send = self
            .conn
            .open_uni()
            .await
            .map_err(wt_conn_err)?
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
        Ok(Box::new(WtUniSend::new(send)))
    }

    async fn accept_bi(&self) -> Result<Box<dyn FrameIo>> {
        Ok(Box::new(WtBi::new(
            self.conn.accept_bi().await.map_err(wt_conn_err)?,
        )))
    }

    async fn accept_uni(&self) -> Result<Box<dyn FrameIo>> {
        Ok(Box::new(WtUniRecv::new(
            self.conn.accept_uni().await.map_err(wt_conn_err)?,
        )))
    }

    fn peer_addr(&self) -> SocketAddr {
        self.conn.remote_address()
    }

    fn close(&self) {
        self.conn.close(0u32.into(), b"closed");
    }

    fn supports_datagram(&self) -> bool {
        true
    }

    fn send_datagram(&self, data: Bytes) -> Result<()> {
        self.conn
            .send_datagram(data)
            .map_err(|e| Error::Io(e.to_string()))
    }

    async fn recv_datagram(&self) -> Result<Bytes> {
        let d = self
            .conn
            .receive_datagram()
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
        Ok(Bytes::from(d.payload().to_vec()))
    }
}

// ======================== 流适配 ========================

/// 接收流包装（本地类型，用于实现 `FrameRead`）
pub(crate) struct WtRecv(wtransport::stream::RecvStream);

#[async_trait]
impl FrameRead for WtRecv {
    async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>> {
        self.0.read(buf).await.map_err(|e| Error::Io(e.to_string()))
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        self.0
            .read_exact(buf)
            .await
            .map_err(|e| Error::Io(e.to_string()))
    }
}

pub(crate) struct WtBi {
    send: wtransport::stream::SendStream,
    recv: WtRecv,
}

impl WtBi {
    fn new(
        (send, recv): (
            wtransport::stream::SendStream,
            wtransport::stream::RecvStream,
        ),
    ) -> Self {
        Self {
            send,
            recv: WtRecv(recv),
        }
    }
}

#[async_trait]
impl FrameIo for WtBi {
    async fn write_message(&mut self, msg: &Message) -> Result<()> {
        let frame = encode_message(msg)?;
        self.send.write_all(&frame).await.map_err(wt_write_err)?;
        Ok(())
    }

    async fn read_message(&mut self) -> Result<Option<Message>> {
        read_message_frame(&mut self.recv).await
    }

    async fn finish(&mut self) -> Result<()> {
        self.send.finish().await.map_err(wt_write_err)
    }

    /// 拆分为读写半部（RPC 复用通道用）
    fn split(self: Box<Self>) -> Result<(Box<dyn FrameIo>, Box<dyn FrameIo>)> {
        Ok((
            Box::new(WtUniSend { send: self.send }),
            Box::new(WtUniRecv { recv: self.recv }),
        ))
    }
}

pub(crate) struct WtUniSend {
    send: wtransport::stream::SendStream,
}

impl WtUniSend {
    fn new(send: wtransport::stream::SendStream) -> Self {
        Self { send }
    }
}

#[async_trait]
impl FrameIo for WtUniSend {
    async fn write_message(&mut self, msg: &Message) -> Result<()> {
        let frame = encode_message(msg)?;
        self.send.write_all(&frame).await.map_err(wt_write_err)?;
        Ok(())
    }

    async fn read_message(&mut self) -> Result<Option<Message>> {
        Err(Error::Protocol("单向发送流不支持读取".into()))
    }

    async fn finish(&mut self) -> Result<()> {
        self.send.finish().await.map_err(wt_write_err)
    }
}

pub(crate) struct WtUniRecv {
    recv: WtRecv,
}

impl WtUniRecv {
    fn new(recv: wtransport::stream::RecvStream) -> Self {
        Self { recv: WtRecv(recv) }
    }
}

#[async_trait]
impl FrameIo for WtUniRecv {
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

// ======================== 错误转换 ========================

fn wt_write_err(e: wtransport::error::StreamWriteError) -> Error {
    Error::Io(e.to_string())
}
