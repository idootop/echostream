//! WebSocket 服务端：监听、接受浏览器连接、消息分发
//!
//! 帧协议与 QUIC 完全一致（长度前缀 + postcard Message）。
//! 能力说明：浏览器 → 服务器（RPC/事件/流）全功能；
//! 服务器 → 浏览器支持事件推送与流推送；服务器主动调用客户端 RPC 暂不支持
//! （WebSocket 无流语义，后续版本以消息路由实现）。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use echostream_core::{Endpoint, FrameIo};
use echostream_core::{
    ServerContext, Session,
    handler::{DynEventHandler, DynRpcHandler, StreamHandler},
    middleware::Middleware,
    router::Router,
};
use echostream_proto::{Error, Message, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

/// 生命周期钩子类型
type Hook<T> = Arc<dyn Fn(&T) + Send + Sync>;

/// WebSocket 服务端
pub struct WsServer {
    listener: TcpListener,
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
    on_start: Vec<Hook<ServerContext>>,
    on_stop: Vec<Hook<ServerContext>>,
    on_connect: Vec<Hook<Session>>,
    on_disconnect: Vec<Hook<Session>>,
    shutdown_signal: tokio::sync::Notify,
}

impl WsServer {
    /// 本地监听地址
    pub fn endpoint_addr(&self) -> Option<SocketAddr> {
        self.listener.local_addr().ok()
    }

    /// 运行服务（阻塞直到 `shutdown`）
    pub async fn run(&self) -> Result<()> {
        for hook in &self.on_start {
            hook(&self.ctx);
        }
        loop {
            tokio::select! {
                conn = self.listener.accept() => {
                    match conn {
                        Ok((stream, peer)) => {
                            match tokio_tungstenite::accept_async(stream).await {
                                Ok(ws) => {
                                    let session = Session::new(
                                        self.ctx.next_session_id(),
                                        Arc::new(WsEndpoint::new(ws, peer)),
                                        self.ctx.clone(),
                                    );
                                    self.ctx.register_session(session.clone());
                                    tracing::debug!("WebSocket 连接: {peer} (session {})", session.id());
                                    let s = session.clone();
                                    for hook in &self.on_connect { hook(&s); }
                                    let hooks = self.on_disconnect.clone();
                                    let r = self.router.clone();
                                    let c = self.ctx.clone();
                                    tokio::spawn(async move {
                                        handle_connection(session, r, c).await;
                                        for hook in &hooks { hook(&s); }
                                    });
                                }
                                Err(e) => tracing::debug!("WebSocket 握手失败: {e}"),
                            }
                        }
                        Err(_) => break,
                    }
                }
                _ = self.shutdown_signal.notified() => break,
            }
        }
        for hook in &self.on_stop {
            hook(&self.ctx);
        }
        Ok(())
    }

    /// 优雅关闭
    pub fn shutdown(&self) {
        self.shutdown_signal.notify_waiters();
    }
}

/// 处理单个连接的消息循环
async fn handle_connection(session: Session, router: Arc<Router>, ctx: Arc<ServerContext>) {
    let ep = session
        .conn()
        .as_any()
        .downcast_ref::<WsEndpoint>()
        .cloned()
        .unwrap();

    // 活跃流路由表：流 id → 帧通道（多帧流）
    let streams: Arc<Mutex<HashMap<u64, mpsc::UnboundedSender<Message>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    loop {
        match ep.recv_frame().await {
            Ok(Some(msg)) => match msg {
                Message::Request(req) => {
                    let resp = respond(&router, &session, req).await;
                    let _ = ep.send_message(&resp).await;
                }
                Message::Event(event) => {
                    router.dispatch_event(&session, event).await;
                }
                Message::StreamOpen(open) => {
                    // 已注册的流：路由到对应接收器；否则尝试新建流会话
                    let existing = streams.lock().unwrap().get(&open.id).cloned();
                    match existing {
                        Some(tx) => {
                            let _ = tx.send(Message::StreamOpen(open));
                        }
                        None if router.has_stream(&open.name) => {
                            let (tx, rx) = mpsc::unbounded_channel::<Message>();
                            streams.lock().unwrap().insert(open.id, tx);
                            let s = session.clone();
                            let r = router.clone();
                            let streams2 = streams.clone();
                            let open_id = open.id;
                            tokio::spawn(async move {
                                let io = WsStreamIo::new(rx);
                                r.dispatch_stream(&s, Box::new(io), open).await;
                                streams2.lock().unwrap().remove(&open_id);
                            });
                        }
                        None => {
                            tracing::debug!("未找到流处理器: {}", open.name);
                        }
                    }
                }
                Message::Stream(frame) => {
                    // 数据帧：路由到已注册流的接收器
                    if let Some(tx) = streams.lock().unwrap().get(&frame.id).cloned() {
                        let _ = tx.send(Message::Stream(frame));
                    }
                }
                Message::StreamEnd(end) => {
                    if let Some(tx) = streams.lock().unwrap().get(&end.id).cloned() {
                        let _ = tx.send(Message::StreamEnd(end));
                    }
                }
                Message::Response(_) => {
                    tracing::debug!("收到未预期的响应帧");
                }
            },
            Ok(None) => break,
            Err(e) => {
                tracing::debug!("连接读取错误: {e}");
                break;
            }
        }
    }
    ctx.unregister_session(session.id());
    tracing::debug!("WebSocket 断开: session {}", session.id());
}

/// RPC 分发并构造响应帧
async fn respond(router: &Router, session: &Session, req: echostream_proto::RequestMsg) -> Message {
    let result = match router.get_rpc(&req.name) {
        Some(handler) => handler.handle_encoded(session, req.data.clone()).await,
        None => Err(Error::HandlerNotFound(req.name.clone())),
    };
    match result {
        Ok(data) => Message::Response(echostream_proto::ResponseMsg {
            id: req.id,
            code: echostream_proto::StatusCode::SUCCESS,
            message: None,
            data,
        }),
        Err(e) => Message::Response(echostream_proto::ResponseMsg {
            id: req.id,
            code: echostream_proto::StatusCode::ERROR,
            message: Some(e.to_string()),
            data: Bytes::new(),
        }),
    }
}

/// 帧编码（长度前缀 + postcard）
fn encode_frame(msg: &Message) -> Bytes {
    let payload = postcard::to_allocvec(msg).expect("编码失败");
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(&payload);
    Bytes::from(buf)
}

// ======================== WebSocket 端点 ========================

/// WebSocket 端点：写方向（服务器推事件/流）映射到 WS 消息；
/// 读方向由连接循环分发（不实现流式 accept）
#[derive(Clone)]
pub(crate) struct WsEndpoint {
    inner: Arc<WsEndpointInner>,
}

struct WsEndpointInner {
    writer: tokio::sync::Mutex<
        futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
            tungstenite::Message,
        >,
    >,
    reader: tokio::sync::Mutex<
        futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        >,
    >,
    peer: SocketAddr,
}

impl WsEndpoint {
    fn new(
        ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        peer: SocketAddr,
    ) -> Self {
        let (writer, reader) = ws.split();
        Self {
            inner: Arc::new(WsEndpointInner {
                writer: tokio::sync::Mutex::new(writer),
                reader: tokio::sync::Mutex::new(reader),
                peer,
            }),
        }
    }

    /// 发送一帧
    async fn send_message(&self, msg: &Message) -> Result<()> {
        let frame = encode_frame(msg);
        let mut w = self.inner.writer.lock().await;
        w.send(tungstenite::Message::Binary(frame.to_vec().into()))
            .await
            .map_err(|e| Error::Io(e.to_string()))
    }

    /// 接收一帧；连接关闭返回 Ok(None)
    async fn recv_frame(&self) -> Result<Option<Message>> {
        // 读取端由 handle_connection 使用独立 reader 完成；
        // 此处通过内部队列实现（见 new 中 reader 的移交）
        let mut rx = self.inner.reader.lock().await;
        loop {
            match rx.next().await {
                Some(Ok(tungstenite::Message::Binary(data))) => {
                    if data.len() < 4 {
                        return Err(Error::Protocol("帧长度不足".into()));
                    }
                    let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
                    let msg: Message = postcard::from_bytes(&data[4..4 + len])
                        .map_err(|e| Error::Serialization(e.to_string()))?;
                    return Ok(Some(msg));
                }
                Some(Ok(tungstenite::Message::Close(_))) => return Ok(None),
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(Error::Io(e.to_string())),
                None => return Ok(None),
            }
        }
    }
}

#[async_trait::async_trait]
impl Endpoint for WsEndpoint {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn open_bi(&self) -> Result<Box<dyn FrameIo>> {
        // WebSocket 无双向流语义：写方向可用（事件/流推送），读方向不支持
        Ok(Box::new(WsWriteIo { ep: self.clone() }))
    }

    async fn open_uni(&self) -> Result<Box<dyn FrameIo>> {
        Ok(Box::new(WsWriteIo { ep: self.clone() }))
    }

    async fn accept_bi(&self) -> Result<Box<dyn FrameIo>> {
        Err(Error::Protocol("WebSocket 由连接循环分发消息".into()))
    }

    async fn accept_uni(&self) -> Result<Box<dyn FrameIo>> {
        Err(Error::Protocol("WebSocket 由连接循环分发消息".into()))
    }

    fn peer_addr(&self) -> SocketAddr {
        self.inner.peer
    }

    fn close(&self) {}
}

/// 只写流：write → WS 发送；read 返回不支持
struct WsWriteIo {
    ep: WsEndpoint,
}

#[async_trait::async_trait]
impl FrameIo for WsWriteIo {
    async fn write_message(&mut self, msg: &Message) -> Result<()> {
        self.ep.send_message(msg).await
    }

    async fn read_message(&mut self) -> Result<Option<Message>> {
        Err(Error::Protocol("WebSocket 只写流不支持读取".into()))
    }

    async fn finish(&mut self) -> Result<()> {
        Ok(())
    }
}

/// 流接收适配：从帧通道读取（连接循环按流 id 路由）
struct WsStreamIo {
    rx: mpsc::UnboundedReceiver<Message>,
}

impl WsStreamIo {
    fn new(rx: mpsc::UnboundedReceiver<Message>) -> Self {
        Self { rx }
    }
}

#[async_trait::async_trait]
impl FrameIo for WsStreamIo {
    async fn write_message(&mut self, _msg: &Message) -> Result<()> {
        Err(Error::Protocol("WebSocket 流不支持写入".into()))
    }

    async fn read_message(&mut self) -> Result<Option<Message>> {
        match self.rx.recv().await {
            Some(Message::Stream(frame)) => Ok(Some(Message::Stream(frame))),
            Some(Message::StreamOpen(open)) => Ok(Some(Message::StreamOpen(open))),
            Some(Message::StreamEnd(_)) => Ok(None), // 流结束
            Some(_) => Ok(None),
            None => Ok(None),
        }
    }

    async fn finish(&mut self) -> Result<()> {
        Ok(())
    }
}

// ======================== 构建器 ========================

/// WebSocket 服务端构建器
pub struct WsServerBuilder {
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
    addr: Option<String>,
    on_start: Vec<Hook<ServerContext>>,
    on_stop: Vec<Hook<ServerContext>>,
    on_connect: Vec<Hook<Session>>,
    on_disconnect: Vec<Hook<Session>>,
}

impl Default for WsServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl WsServerBuilder {
    /// 创建构建器
    pub fn new() -> Self {
        Self {
            router: Arc::new(Router::default()),
            ctx: Arc::new(ServerContext::new()),
            addr: None,
            on_start: Vec::new(),
            on_stop: Vec::new(),
            on_connect: Vec::new(),
            on_disconnect: Vec::new(),
        }
    }

    /// 绑定监听地址
    pub fn bind(mut self, addr: impl Into<String>) -> Self {
        self.addr = Some(addr.into());
        self
    }

    /// 注册 RPC 处理器
    pub fn add_rpc<H: DynRpcHandler>(self, handler: H) -> Self {
        self.router.add_rpc(handler);
        self
    }

    /// 注册事件处理器
    pub fn add_event<H: DynEventHandler>(self, handler: H) -> Self {
        self.router.add_event(handler);
        self
    }

    /// 注册流处理器
    pub fn add_stream<H: StreamHandler>(self, handler: H) -> Self {
        self.router.add_stream(handler);
        self
    }

    /// 添加中间件
    pub fn middleware<M: Middleware>(self, middleware: M) -> Self {
        self.router.add_middleware(middleware);
        self
    }

    /// 客户端连接钩子
    pub fn on_connect<F>(mut self, f: F) -> Self
    where
        F: Fn(&Session) + Send + Sync + 'static,
    {
        self.on_connect.push(Arc::new(f));
        self
    }

    /// 客户端断开钩子
    pub fn on_disconnect<F>(mut self, f: F) -> Self
    where
        F: Fn(&Session) + Send + Sync + 'static,
    {
        self.on_disconnect.push(Arc::new(f));
        self
    }

    /// 访问服务端全局上下文
    pub fn ctx(&self) -> &Arc<ServerContext> {
        &self.ctx
    }

    /// 构建服务端
    pub async fn build(self) -> Result<WsServer> {
        let addr = self
            .addr
            .ok_or_else(|| Error::InvalidParameter("未指定监听地址".into()))?;
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
        Ok(WsServer {
            listener,
            router: self.router,
            ctx: self.ctx,
            on_start: self.on_start,
            on_stop: self.on_stop,
            on_connect: self.on_connect,
            on_disconnect: self.on_disconnect,
            shutdown_signal: tokio::sync::Notify::new(),
        })
    }

    /// 构建并运行
    pub async fn serve(self) -> Result<()> {
        self.build().await?.run().await
    }
}
