//! 线缆格式单元测试：消息编解码往返 + 帧边界 + 状态码语义

use async_trait::async_trait;
use bytes::Bytes;
use echostream_proto::Result;
use echostream_proto::{
    EventMsg, FrameRead, Message, RequestMsg, ResponseMsg, StatusCode, StreamEndMsg, StreamMsg,
    Timestamp, encode_message, read_message_frame,
};

/// 内存读取器：实现 FrameRead 用于测试帧解码
struct MemReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

#[async_trait]
impl FrameRead for MemReader<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>> {
        if self.pos >= self.buf.len() {
            return Ok(None);
        }
        let n = buf.len().min(self.buf.len() - self.pos);
        buf[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(Some(n))
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        if self.buf.len() - self.pos < buf.len() {
            return Err(echostream_proto::Error::Protocol("数据不足".into()));
        }
        buf.copy_from_slice(&self.buf[self.pos..self.pos + buf.len()]);
        self.pos += buf.len();
        Ok(())
    }
}

async fn roundtrip(msg: &Message) -> Message {
    let frame = encode_message(msg).expect("编码失败");
    let mut reader = MemReader {
        buf: &frame,
        pos: 0,
    };
    read_message_frame(&mut reader)
        .await
        .expect("解码失败")
        .expect("帧不完整")
}

#[tokio::test]
async fn message_roundtrip_all_variants() {
    let messages = vec![
        Message::Request(RequestMsg {
            id: 42,
            name: "add".into(),
            data: Bytes::from(vec![1, 2, 3]),
        }),
        Message::Response(ResponseMsg {
            id: 42,
            code: StatusCode::SUCCESS,
            message: None,
            data: Bytes::from(vec![9, 9]),
        }),
        Message::Response(ResponseMsg {
            id: 7,
            code: StatusCode::FORBIDDEN,
            message: Some("被拦截".into()),
            data: Bytes::new(),
        }),
        Message::Event(EventMsg {
            id: 1,
            name: "hello".into(),
            data: Bytes::from(b"world".to_vec()),
        }),
        Message::StreamOpen(echostream_proto::StreamOpenMsg {
            id: 3,
            name: "chat".into(),
            metadata: vec![
                echostream_proto::StreamMetaEntry::str("codec", "h264"),
                echostream_proto::StreamMetaEntry::num("width", 1920),
            ],
        }),
        Message::Stream(StreamMsg {
            id: 3,
            seq: 0,
            flags: 0,
            sender_ts: Timestamp(123456),
            rtp_ts: 0,
            data: Bytes::from(vec![0u8; 4096]),
        }),
        Message::StreamEnd(StreamEndMsg {
            id: 3,
            code: 0,
            message: None,
            metadata: vec![echostream_proto::StreamMetaEntry::num("frames", 42)],
        }),
    ];
    for msg in messages {
        assert_eq!(roundtrip(&msg).await, msg, "往返不一致: {msg:?}");
    }
}

#[tokio::test]
async fn frame_boundary_multiple_messages() {
    // 连续编码多个帧，应能逐帧读回
    let msgs = vec![
        Message::Request(RequestMsg {
            id: 1,
            name: "a".into(),
            data: Bytes::new(),
        }),
        Message::Event(EventMsg {
            id: 2,
            name: "b".into(),
            data: Bytes::from(vec![7; 1000]),
        }),
        Message::StreamEnd(StreamEndMsg {
            id: 9,
            code: 0,
            message: None,
            metadata: Vec::new(),
        }),
    ];
    let mut buf = Vec::new();
    for m in &msgs {
        buf.extend_from_slice(&encode_message(m).expect("编码失败"));
    }
    let mut reader = MemReader { buf: &buf, pos: 0 };
    for expected in &msgs {
        let got = read_message_frame(&mut reader)
            .await
            .expect("解码失败")
            .expect("帧不完整");
        assert_eq!(&got, expected);
    }
    // 全部读完后应返回 None（对端关闭）
    assert!(read_message_frame(&mut reader).await.unwrap().is_none());
}

#[tokio::test]
async fn truncated_frame_detected() {
    let msg = Message::Event(EventMsg {
        id: 1,
        name: "hello".into(),
        data: Bytes::from(vec![0u8; 100]),
    });
    let frame = encode_message(&msg).expect("编码失败");
    // 截断一半：应报错（帧不完整）
    let cut = frame.len() / 2;
    let mut reader = MemReader {
        buf: &frame[..cut],
        pos: 0,
    };
    assert!(read_message_frame(&mut reader).await.is_err());
}

#[test]
fn status_code_semantics() {
    assert!(StatusCode::SUCCESS.is_success());
    assert!(!StatusCode::ERROR.is_success());
    assert_eq!(StatusCode::FORBIDDEN.0, 4);
    assert_eq!(StatusCode::new(99).0, 99);
}

#[test]
fn timestamp_now_is_monotonic_millis() {
    let a = Timestamp::now();
    let b = Timestamp::now();
    assert!(b.as_millis() >= a.as_millis());
    assert!(a.as_millis() > 1_700_000_000_000); // 2023 年后
}
