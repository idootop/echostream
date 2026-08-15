//! 流发送器与接收器：流式数据的帧级收发（自动序列化）
//!
//! 流帧载荷遵循与 RPC/Event 相同的编码约定（postcard）：各端 send 自动
//! 序列化、recv 自动反序列化，跨语言无需手动编解码。
//!
//! # 消费模式（业界主流）
//!
//! - **句柄拉取（Rust 主流）**：处理器拿到 StreamReceiver 句柄自行决定何时读帧，
//!   用 while-let 循环逐帧读取（实现 futures::Stream，可组合 map / try_for_each
//!   等组合子）或强类型 recv::<T>() 循环；何时读、读多少完全由业务控制。
//! - **回调推送（绑定/脚本端常用）**：Node / Python / Web 绑定把接收器暴露为句柄，
//!   由脚本侧逐帧 recv() 拉取（async iterator 语义），数据到达即回调。
//!
//! 两种模式同源：接收器即流句柄，拉取式消费是回调式消费的底层原语。

use bytes::Bytes;
use echostream_proto::endpoint::FrameIo;
use echostream_proto::{Message, Result, StreamEndMsg, StreamMsg, Timestamp};
use futures::{Stream, stream::unfold};
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
///
/// 实现 futures::Stream（Item = Result<StreamMsg>），可配合 StreamExt
/// 组合子消费，或直接用强类型 recv::<T>() 循环。
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

    /// 转换为 futures::Stream（逐帧拉取，含元数据；错误以 Err(item) 呈现）
    ///
    /// 底层 FrameIo 为 async_trait（装箱 future），无法在 poll 中直接驱动，
    /// 故以 unfold 异步拉取实现；消费方式与原生 Stream 完全一致。
    pub fn into_stream(self) -> impl Stream<Item = Result<StreamMsg>> {
        unfold(Some(self), |mut recv| async move {
            let result = match recv.as_mut() {
                Some(r) => r.recv_frame().await,
                None => return None, // 已结束
            };
            match result {
                Ok(Some(frame)) => Some((Ok(frame), recv)),
                Ok(None) => None, // 流结束：正常终止
                Err(e) => Some((Err(e), recv)),
            }
        })
    }

    /// 转换为强类型 futures::Stream（逐帧自动反序列化；错误以 Err(item) 呈现）
    ///
    /// ```text
    /// let frames = stream.into_stream_typed::<String>();
    /// futures::pin_mut!(frames); // unfold 流需固定后再 next()
    /// while let Some(item) = frames.next().await {
    ///     println!("帧: {}", item?);
    /// }
    /// ```
    pub fn into_stream_typed<T: DeserializeOwned>(
        self,
    ) -> impl Stream<Item = Result<T>> {
        unfold(Some(self), |mut recv| async move {
            let result = match recv.as_mut() {
                Some(r) => r.recv_frame().await,
                None => return None, // 已结束
            };
            match result {
                Ok(Some(frame)) => match codec::decode(&frame.data) {
                    Ok(v) => Some((Ok(v), recv)),
                    Err(e) => Some((Err(e), recv)),
                },
                Ok(None) => None, // 流结束：正常终止
                Err(e) => Some((Err(e), recv)),
            }
        })
    }

    /// 流名
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use echostream_proto::endpoint::FrameIo;
    use echostream_proto::Error;

    /// 测试用内存流：按序返回预置帧
    struct FakeIo {
        frames: std::vec::IntoIter<Message>,
    }

    impl FakeIo {
        fn new(frames: Vec<Message>) -> Self {
            Self {
                frames: frames.into_iter(),
            }
        }
    }

    #[async_trait]
    impl FrameIo for FakeIo {
        async fn write_message(&mut self, _msg: &Message) -> Result<()> {
            Err(Error::Protocol("只读".into()))
        }

        async fn read_message(&mut self) -> Result<Option<Message>> {
            Ok(self.frames.next())
        }

        async fn finish(&mut self) -> Result<()> {
            Err(Error::Protocol("只读".into()))
        }
    }

    /// 原始字节帧（raw 测试用）
    fn frame_raw(id: u64, name: &str, seq: u64, data: &[u8]) -> Message {
        Message::Stream(StreamMsg {
            id,
            name: name.into(),
            seq,
            sender_ts: Timestamp(0),
            data: Bytes::copy_from_slice(data),
        })
    }

    /// postcard 编码帧（强类型测试用）
    fn frame(id: u64, name: &str, seq: u64, data: &str) -> Message {
        frame_raw(
            id,
            name,
            seq,
            &postcard::to_allocvec(&data.to_string()).unwrap(),
        )
    }

    #[tokio::test]
    async fn recv_frame_skips_foreign_frames_and_ends() {
        let io = Box::new(FakeIo::new(vec![
            frame_raw(1, "a", 0, b"x"),
            frame_raw(99, "other", 0, b"noise"), // 不属于本流
            Message::StreamEnd(StreamEndMsg { id: 1 }),
        ]));
        let mut recv = StreamReceiver {
            id: 1,
            name: "a".into(),
            pending: None,
            io,
        };
        let f = recv.recv_frame().await.unwrap().unwrap();
        assert_eq!(f.data.as_ref(), b"x");
        assert_eq!(f.seq, 0);
        assert!(recv.recv_frame().await.unwrap().is_none(), "流应结束");
    }

    #[tokio::test]
    async fn into_stream_pulls_typed_items_until_end() {
        use futures::StreamExt;
        let io = Box::new(FakeIo::new(vec![
            frame(1, "chat", 0, "hi"),
            frame(1, "chat", 1, "world"),
        ]));
        let recv = StreamReceiver {
            id: 1,
            name: "chat".into(),
            pending: None,
            io,
        };
        let items: Vec<String> = recv
            .into_stream_typed()
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(items, vec!["hi".to_string(), "world".to_string()]);
    }
}
