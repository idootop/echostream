//! 流发送器与接收器：流式数据的帧级收发

use bytes::Bytes;
use echostream_proto::endpoint::FrameIo;
use echostream_proto::{Message, Result, StreamMsg, Timestamp};

/// 流发送器：发送流数据帧（自动递增序号 + 时间戳）
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

    /// 发送一帧数据
    pub async fn send<T: Into<Bytes>>(&mut self, data: T) -> Result<()> {
        let frame = StreamMsg {
            id: self.id,
            name: self.name.clone(),
            seq: self.seq,
            sender_ts: Timestamp::now(),
            data: data.into(),
        };
        self.seq += 1;
        self.io.write_message(&Message::Stream(frame)).await
    }

    /// 流名
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 关闭流（对端读到流结束）
    pub async fn finish(&mut self) -> Result<()> {
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

    /// 读取下一帧；流结束返回 `Ok(None)`
    pub async fn recv(&mut self) -> Result<Option<StreamMsg>> {
        if let Some(first) = self.pending.take() {
            return Ok(Some(first));
        }
        loop {
            match self.io.read_message().await? {
                Some(Message::Stream(frame)) if frame.id == self.id => return Ok(Some(frame)),
                Some(_) => continue, // 忽略不属于本流的帧
                None => return Ok(None),
            }
        }
    }

    /// 流名
    pub fn name(&self) -> &str {
        &self.name
    }
}
