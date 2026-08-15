//! 流发送器与接收器：流式数据的帧级收发（自动序列化）
//!
//! 流生命周期三帧协议（与 gRPC headers/messages/trailers 同构，适配
//! 音频 / 视频 / 文件 / 实时数据等场景）：
//!
//! 1. **StreamOpen（流开始）**：携带流名称与元数据 —— 音视频编码参数
//!    （codec/samplerate/bitrate/width/height/fps）、时间同步时钟
//!    （clock-rate，rtp_ts 的单位）、文件信息（filename/size/mime）、自定义扩展
//! 2. **StreamMsg（数据帧）**：按 id 路由（名称仅在 open 携带，省每帧字符串开销），
//!    带序列号（丢包检测）与墙钟时间戳（延迟测量）；核心只承载传输语义，
//!    采样时钟 / 关键帧等上层语义由 open.metadata 协商、上层插件在载荷内实现
//! 3. **StreamEnd（流结束）**：结束码（0 正常 / 非 0 异常取消）+ 原因 + 结束元数据
//!    （trailers 风格：统计、校验和等）
//!
//! 消费模式：
//! - **句柄拉取（Rust 主流）**：while-let 循环逐帧读取（futures::Stream 组合子互通）
//! - **回调推送（绑定/脚本端常用）**：接收器暴露为句柄，脚本侧逐帧 recv()

use bytes::Bytes;
use echostream_proto::endpoint::FrameIo;
use echostream_proto::{Message, Result, StreamEndMsg, StreamMetaEntry, StreamMsg, Timestamp};
use futures::{Stream, stream::unfold};
use serde::{Serialize, de::DeserializeOwned};

use crate::codec;

/// 流发送器：发送流数据帧（自动递增序号 + 时间戳 + 自动序列化）
pub struct StreamSender {
    io: Box<dyn FrameIo>,
    id: u64,
    name: String,
    seq: u64,
    /// 流元数据（首帧 StreamOpen 发送；对流不可变）
    metadata: Vec<StreamMetaEntry>,
    /// 是否已发送 StreamOpen（首帧前自动补发）
    opened: bool,
}

impl StreamSender {
    pub(crate) fn new(
        io: Box<dyn FrameIo>,
        id: u64,
        name: String,
        metadata: Vec<StreamMetaEntry>,
    ) -> Self {
        Self {
            io,
            id,
            name,
            seq: 0,
            metadata,
            opened: false,
        }
    }

    /// 发送一帧（自动序列化，业务直接传强类型值）
    pub async fn send<T: Serialize + Send>(&mut self, data: T) -> Result<()> {
        self.send_raw(codec::encode(&data)?).await
    }

    /// 发送帧（载荷为已编码字节，跳过序列化）
    pub async fn send_raw(&mut self, data: Bytes) -> Result<()> {
        // 首帧前自动补发 StreamOpen（零数据流不发送）
        if !self.opened {
            self.io
                .write_message(&Message::StreamOpen(echostream_proto::StreamOpenMsg {
                    id: self.id,
                    name: self.name.clone(),
                    metadata: self.metadata.clone(),
                }))
                .await?;
            self.opened = true;
        }
        let frame = StreamMsg {
            id: self.id,
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

    /// 流元数据
    pub fn metadata(&self) -> &[StreamMetaEntry] {
        &self.metadata
    }

    /// 关闭流（正常结束：StreamEnd code=0 + 关闭底层发送端）
    ///
    /// 先发送 StreamEnd 帧（WebSocket 等无流关闭语义的传输以此标记结束，
    /// QUIC 上为冗余无害帧），再关闭底层发送端。
    pub async fn finish(&mut self) -> Result<()> {
        self.finish_with(0, None, Vec::new()).await
    }

    /// 关闭流（指定结束码与原因；0 = 正常，非 0 = 异常 / 取消 / 业务终止）
    pub async fn finish_with(
        &mut self,
        code: u16,
        message: Option<String>,
        metadata: Vec<StreamMetaEntry>,
    ) -> Result<()> {
        self.io
            .write_message(&Message::StreamEnd(StreamEndMsg {
                id: self.id,
                code,
                message,
                metadata,
            }))
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
    /// 流元数据（来自 StreamOpen 首帧）
    metadata: Vec<StreamMetaEntry>,
    pending: Option<StreamMsg>, // 首帧缓存（分派时已读出的第一帧）
    /// 结束码（0 = 正常；读到 StreamEnd 或流 EOF 后有效）
    end_code: u16,
    /// 结束原因
    end_message: Option<String>,
    /// 结束元数据（trailers）
    end_metadata: Vec<StreamMetaEntry>,
    /// 流是否已结束
    ended: bool,
}

impl StreamReceiver {
    /// 构造接收器（供框架分派与扩展 crate 测试使用；业务请通过流处理器获得）
    #[doc(hidden)]
    pub fn new(io: Box<dyn FrameIo>, first: StreamMsg, metadata: Vec<StreamMetaEntry>) -> Self {
        Self {
            id: first.id,
            name: String::new(), // 由 dispatch 通过 with_name 填充
            metadata,
            pending: Some(first),
            io,
            end_code: 0,
            end_message: None,
            end_metadata: Vec::new(),
            ended: false,
        }
    }

    /// 设置流名（分派时从 StreamOpen 解析）
    pub(crate) fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// 读取下一帧（自动反序列化）；流结束返回 Ok(None)
    pub async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        match self.recv_frame().await? {
            Some(frame) => Ok(Some(codec::decode(&frame.data)?)),
            None => Ok(None),
        }
    }

    /// 读取原始帧（含序号 / 标志 / 时间戳等元数据）；流结束返回 Ok(None)
    pub async fn recv_frame(&mut self) -> Result<Option<StreamMsg>> {
        if self.ended {
            return Ok(None);
        }
        if let Some(first) = self.pending.take() {
            return Ok(Some(first));
        }
        loop {
            match self.io.read_message().await? {
                Some(Message::Stream(frame)) if frame.id == self.id => return Ok(Some(frame)),
                Some(Message::StreamEnd(end)) if end.id == self.id => {
                    // 协议末帧：记录结束信息，流结束
                    self.end_code = end.code;
                    self.end_message = end.message;
                    self.end_metadata = end.metadata;
                    self.ended = true;
                    return Ok(None);
                }
                Some(_) => continue, // 忽略不属于本流的帧
                None => {
                    // 底层流关闭（QUIC FIN 未带 StreamEnd）：视为正常结束
                    self.ended = true;
                    return Ok(None);
                }
            }
        }
    }

    /// 转换为 futures::Stream（逐帧拉取，含元数据；错误以 Err(item) 呈现）
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
    pub fn into_stream_typed<T: DeserializeOwned>(self) -> impl Stream<Item = Result<T>> {
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

    /// 流元数据（来自 StreamOpen 首帧：音视频参数 / 文件信息等）
    pub fn metadata(&self) -> &[StreamMetaEntry] {
        &self.metadata
    }

    /// 按键查询流元数据（字符串值）
    pub fn get_metadata(&self, key: &str) -> Option<String> {
        self.metadata
            .iter()
            .find(|m| m.key == key)
            .map(|m| String::from_utf8_lossy(&m.value).to_string())
    }

    /// 按键查询流元数据（原始字节）
    pub fn get_metadata_bytes(&self, key: &str) -> Option<&Bytes> {
        self.metadata
            .iter()
            .find(|m| m.key == key)
            .map(|m| &m.value)
    }

    /// 按键查询流元数据（布尔值；识别 "true"/"1" 与 "false"/"0"）
    pub fn get_metadata_bool(&self, key: &str) -> Option<bool> {
        self.get_metadata(key).and_then(|s| match s.as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        })
    }

    /// 结束码（流结束后有效；0 = 正常结束，非 0 = 异常 / 取消 / 业务终止）
    pub fn end_code(&self) -> u16 {
        self.end_code
    }

    /// 结束原因（流结束后有效）
    pub fn end_message(&self) -> Option<&str> {
        self.end_message.as_deref()
    }

    /// 结束元数据（trailers：统计信息、校验和等）
    pub fn end_metadata(&self) -> &[StreamMetaEntry] {
        &self.end_metadata
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use echostream_proto::endpoint::FrameIo;
    use echostream_proto::{Error, StreamEndMsg};

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

    /// postcard 编码帧（typed 解码测试用）
    fn frame(id: u64, seq: u64, data: &str) -> Message {
        Message::Stream(StreamMsg {
            id,
            seq,
            sender_ts: Timestamp(0),
            data: Bytes::from(postcard::to_allocvec(&data.to_string()).unwrap()),
        })
    }

    fn end(id: u64, code: u16, message: Option<&str>) -> Message {
        Message::StreamEnd(StreamEndMsg {
            id,
            code,
            message: message.map(|s| s.to_string()),
            metadata: Vec::new(),
        })
    }

    fn recv_with(io: Box<dyn FrameIo>, first_data: &str) -> StreamReceiver {
        StreamReceiver::new(
            io,
            StreamMsg {
                id: 1,
                seq: 0,
                sender_ts: Timestamp(0),
                data: Bytes::from(postcard::to_allocvec(&first_data.to_string()).unwrap()),
            },
            vec![],
        )
    }

    #[test]
    fn metadata_bool_parses_true_false() {
        let meta = vec![
            StreamMetaEntry::bool("live", true),
            StreamMetaEntry::bool("recorded", false),
            StreamMetaEntry::str("codec", "h264"),
            StreamMetaEntry::num("width", 1920),
        ];
        let recv = StreamReceiver::new(
            Box::new(FakeIo::new(vec![])),
            StreamMsg {
                id: 1,
                seq: 0,
                sender_ts: Timestamp(0),
                data: Bytes::new(),
            },
            meta,
        );
        assert_eq!(recv.get_metadata_bool("live"), Some(true));
        assert_eq!(recv.get_metadata_bool("recorded"), Some(false));
        // 字符串 / 数字 / 缺失键：None（非布尔值不误判）
        assert_eq!(recv.get_metadata_bool("codec"), None);
        assert_eq!(recv.get_metadata_bool("width"), None);
        assert_eq!(recv.get_metadata_bool("missing"), None);
        // 字符串形态与 bytes 形态一致
        assert_eq!(recv.get_metadata("live").as_deref(), Some("true"));
        assert_eq!(recv.get_metadata("recorded").as_deref(), Some("false"));
    }

    #[tokio::test]
    async fn recv_frame_skips_foreign_frames_and_ends() {
        let io = Box::new(FakeIo::new(vec![
            frame(99, 0, "noise"), // 不属于本流
            end(1, 7, Some("cancelled")),
            frame(1, 1, "after-end"), // 结束后的帧应被忽略
        ]));
        let mut recv = recv_with(io, "x");
        let f = recv.recv_frame().await.unwrap().unwrap();
        assert_eq!(f.seq, 0);
        let text: String = codec::decode(&f.data).unwrap();
        assert_eq!(text, "x");
        assert!(recv.recv_frame().await.unwrap().is_none(), "流应结束");
        assert_eq!(recv.end_code(), 7, "应记录结束码");
        assert_eq!(recv.end_message(), Some("cancelled"), "应记录结束原因");
        // 结束后再读返回 None
        assert!(recv.recv_frame().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn eof_without_end_is_normal_finish() {
        let io = Box::new(FakeIo::new(vec![]));
        let mut recv = recv_with(io, "only");
        let _ = recv.recv_frame().await.unwrap();
        assert!(recv.recv_frame().await.unwrap().is_none());
        assert_eq!(recv.end_code(), 0, "EOF 视为正常结束");
    }

    #[tokio::test]
    async fn into_stream_pulls_typed_items_until_end() {
        use futures::StreamExt;
        let io = Box::new(FakeIo::new(vec![frame(1, 1, "world"), end(1, 0, None)]));
        let recv = recv_with(io, "hi");
        let items: Vec<String> = recv.into_stream_typed().map(|r| r.unwrap()).collect().await;
        assert_eq!(items, vec!["hi".to_string(), "world".to_string()]);
    }
}
