//! 转换中间件行为验证：请求方向（进入处理器前）与响应方向（返回后）转换

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use echostream_core::{Endpoint, FrameIo, Router, Session, ServerContext};
use echostream_middleware_transform::TransformMiddleware;
use echostream_proto::{Error, Message, RequestMsg, Result};

/// 无 I/O 端点桩
struct NoopEndpoint;

#[async_trait]
impl Endpoint for NoopEndpoint {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    async fn open_bi(&self) -> Result<Box<dyn FrameIo>> {
        Err(Error::Protocol("stub".into()))
    }
    async fn open_uni(&self) -> Result<Box<dyn FrameIo>> {
        Err(Error::Protocol("stub".into()))
    }
    async fn accept_bi(&self) -> Result<Box<dyn FrameIo>> {
        Err(Error::Protocol("stub".into()))
    }
    async fn accept_uni(&self) -> Result<Box<dyn FrameIo>> {
        Err(Error::Protocol("stub".into()))
    }
    fn peer_addr(&self) -> SocketAddr {
        "127.0.0.1:1".parse().unwrap()
    }
    fn close(&self) {}
}

/// 捕获写入帧的流桩（验证响应载荷；Arc 共享便于断言）
#[derive(Clone)]
struct CaptureIo {
    written: Arc<std::sync::Mutex<Vec<Message>>>,
}

#[async_trait]
impl FrameIo for CaptureIo {
    async fn write_message(&mut self, msg: &Message) -> Result<()> {
        self.written.lock().unwrap().push(msg.clone());
        Ok(())
    }
    async fn read_message(&mut self) -> Result<Option<Message>> {
        Err(Error::Protocol("stub".into()))
    }
    async fn finish(&mut self) -> Result<()> {
        Ok(())
    }
}

/// 回显处理器：载荷追加 "_handled" 后返回
struct EchoRpc;

#[async_trait]
impl echostream_core::RpcHandler for EchoRpc {
    type Req = String;
    type Resp = String;

    fn name(&self) -> &str {
        "echo"
    }

    async fn handle(&self, _s: &Session, req: String) -> Result<String> {
        Ok(format!("{req}_handled"))
    }
}

fn encode_str(s: &str) -> Bytes {
    Bytes::from(postcard::to_allocvec(&s.to_string()).unwrap())
}

#[tokio::test]
async fn transform_applies_to_request_and_response() {
    let router = Router::default();
    // 请求方向：剥离 0xAA 标记字节；响应方向：追加 0xBB 标记字节
    // （转换作用于原始载荷字节，生产场景如压缩/加密需对端配合）
    router.add_middleware(
        TransformMiddleware::new()
            .map_request(|data| {
                assert_eq!(data.first(), Some(&0xAA), "请求载荷应以标记字节开头");
                Ok(Bytes::copy_from_slice(&data[1..]))
            })
            .map_response(|data| {
                let mut out = data.to_vec();
                out.push(0xBB);
                Ok(Bytes::from(out))
            }),
    );
    router.add_rpc(EchoRpc);

    let session = Session::new(1, Arc::new(NoopEndpoint), Arc::new(ServerContext::new()));
    let capture = CaptureIo {
        written: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let mut stream: Box<dyn FrameIo> = Box::new(capture.clone());

    // 客户端发送带标记字节的载荷（0xAA + postcard("hello")）
    let mut tagged = vec![0xAA];
    tagged.extend_from_slice(&encode_str("hello"));
    router
        .dispatch_rpc(
            &session,
            &mut *stream,
            RequestMsg {
                id: 1,
                name: "echo".into(),
                data: Bytes::from(tagged),
            },
        )
        .await;

    let messages = capture.written.lock().unwrap().clone();
    assert_eq!(messages.len(), 1);
    match &messages[0] {
        Message::Response(r) => {
            assert_eq!(r.data.last(), Some(&0xBB), "响应应经过 map_response 转换");
            let resp: String = postcard::from_bytes(&r.data[..r.data.len() - 1]).unwrap();
            assert_eq!(resp, "hello_handled", "处理器应收到剥离标记后的载荷");
        }
        other => panic!("期望 Response，得到 {other:?}"),
    }
}
