//! 流发送器与接收器：流式数据的帧级收发（自动序列化）
//!
//! 流帧载荷遵循与 RPC/Event 相同的编码约定（postcard）：各端 send 自动
//! 序列化、recv 自动反序列化，跨语言无需手动编解码。

use bytes::Bytes;
use echostream_proto::endpoint::FrameIo;
use echostream_proto::{Message, Result, StreamEndMsg, StreamMsg, Timestamp};
use serde::{Serialize, de::DeserializeOwned};

use crate::codec;

/// 流发送器：发送流数据帧（自动递增序号 + 时间戳 + 自动序列化）
pub struct StreamSender {
    io: Box<dyn FrameIo>,
    id: u64,
    name: String,
    seq: u64,
}

impl StreamSender {
    pub(crate) fn new(io: Box<dyn FrameIo>, id: u64, name: String) -> Self {
        Self {
            io,
            id,
            name,
            seq: 0,
        }
    }

    /// 发送一帧（自动序列化，业务直接传强类型值）
    pub async fn send<T: Serialize + Send>(&mut self, data: T) -> Result<()> {
        self.send_raw(codec::encode(&data)?).await
    }

    /// 发送一帧（载荷为已编码字节，跳过序列化）
    pub async fn send_raw(&mut self, data: Bytes) -> Result<()> {
        let frame = StreamMsg {
            id: self.id,
            name: self.name.clone(),
            seq: self.seq,
            sender_ts: Timestamp::now(),
            data,
        };
        self.seq += 1;
        self.io.write_message(&Message::Stream(frame)).await
    }

    /// 流名
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 关闭流（对端读到流结束）
    ///
    /// 先发送 StreamEnd 帧（WebSocket 等无流关闭语义的传输以此标记结束，
    /// QUIC 上为冗余无害帧），再关闭底层发送端。
    pub async fn finish(&mut self) -> Result<()> {
        self.io
            .write_message(&Message::StreamEnd(StreamEndMsg { id: self.id }))
            .await?;
        self.io.finish().await
    }
}

/// 流接收器：持续读取流数据帧直到结束
pub struct StreamReceiver {
    io: Box<dyn FrameIo>,
    id: u64,
    name: String,
    pending: Option<StreamMsg>, // 首帧缓存（分派时已读出的第一帧）
}

impl StreamReceiver {
    pub(crate) fn new(io: Box<dyn FrameIo>, first: StreamMsg) -> Self {
        Self {
            id: first.id,
            name: first.name.clone(),
            pending: Some(first),
            io,
        }
    }

    /// 读取下一帧（自动反序列化）；流结束返回 Ok(None)
    pub async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        match self.recv_frame().await? {
            Some(frame) => Ok(Some(codec::decode(&frame.data)?)),
            None => Ok(None),
        }
    }

    /// 读取原始帧（含序号 / 时间戳等元数据）；流结束返回 Ok(None)
    pub async fn recv_frame(&mut self) -> Result<Option<StreamMsg>> {
        if let Some(first) = self.pending.take() {
            return Ok(Some(first));
        }
        loop {
            match self.io.read_message().await? {
                Some(Message::Stream(frame)) if frame.id == self.id => return Ok(Some(frame)),
                Some(_) => continue, // 忽略不属于本流的帧（含 StreamEnd）
                None => return Ok(None),
            }
        }
    }

    /// 流名
    pub fn name(&self) -> &str {
        &self.name
    }
}
