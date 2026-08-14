//! 服务端：监听、接受连接、消息分发

use std::sync::Arc;

use echostream_proto::Message;

use echostream_proto::endpoint::Listener;

use crate::context::ServerContext;
use crate::handler::{DynEventHandler, DynRpcHandler, StreamHandler};
use crate::middleware::Middleware;
use crate::plugin::ServerPlugin;
use crate::router::Router;
use crate::session::Session;

/// 生命周期钩子类型：同步回调（异步逻辑可在回调内 `tokio::spawn`）
type Hook<T> = Arc<dyn Fn(&T) + Send + Sync>;

/// 服务端
pub struct Server {
    listener: Arc<dyn Listener>,
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
    on_start: Vec<Hook<ServerContext>>,
    on_stop: Vec<Hook<ServerContext>>,
    on_connect: Vec<Hook<Session>>,
    on_disconnect: Vec<Hook<Session>>,
    shutdown_signal: tokio::sync::Notify,
}

impl Server {
    /// 本地监听地址
    pub fn endpoint_addr(&self) -> Option<std::net::SocketAddr> {
        self.listener.local_addr().ok()
    }

    /// 运行服务（阻塞直到 `shutdown` 被调用）
    pub async fn run(&self) -> echostream_proto::Result<()> {
        for hook in &self.on_start {
            hook(&self.ctx);
        }

        loop {
            tokio::select! {
                conn = self.listener.accept() => {
                    match conn {
                        Some(conn) => {
                            let session = Session::new(self.ctx.next_session_id(), conn, self.ctx.clone());
                            self.ctx.register_session(session.clone());
                            tracing::debug!(
                                "客户端连接: {} (session {})",
                                session.peer_addr(),
                                session.id()
                            );
                            let s = session.clone();
                            for hook in &self.on_connect {
                                hook(&s);
                            }
                            let hooks = self.on_disconnect.clone();
                            let r = self.router.clone();
                            let c = self.ctx.clone();
                            tokio::spawn(async move {
                                handle_connection(session, r, c).await;
                                for hook in &hooks {
                                    hook(&s);
                                }
                            });
                        }
                        None => break,
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

    /// 优雅关闭：停止接受新连接并触发 on_stop
    pub fn shutdown(&self) {
        self.shutdown_signal.notify_waiters();
        self.listener.close();
    }
}

/// 处理单个连接的消息循环
async fn handle_connection(session: Session, router: Arc<Router>, ctx: Arc<ServerContext>) {
    let conn = session.conn();
    // 数据报接收任务（不可靠事件通道）
    if conn.supports_datagram() {
        let s = session.clone();
        let r = router.clone();
        tokio::spawn(async move {
            let conn = s.conn();
            while let Ok(data) = conn.recv_datagram().await {
                if let Ok(msg) = postcard::from_bytes(&data) {
                    r.dispatch_inbound_datagram(&s, msg).await;
                }
            }
        });
    }
    loop {
        tokio::select! {
            // 双向流：RPC 请求 / 流数据（spawn 并行处理，避免阻塞 accept 循环）
            bi = conn.accept_bi() => {
                match bi {
                    Ok(stream) => {
                        let s = session.clone();
                        let r = router.clone();
                        tokio::spawn(async move {
                            let mut stream = stream;
                            match stream.read_message().await {
                                // RPC 复用通道：长连接双向流上按 id 多路复用请求/响应
                                Ok(Some(Message::Request(req)))
                                    if req.name == echostream_proto::RPC_CHANNEL_NAME =>
                                {
                                    let s = s.clone();
                                    let r = r.clone();
                                    loop {
                                        match stream.read_message().await {
                                            Ok(Some(Message::Request(req))) => {
                                                // 逐请求分发并写回响应（同一流）；慢处理器会暂缓通道
                                                r.dispatch_rpc(&s, &mut *stream, req).await;
                                            }
                                            Ok(Some(_)) => continue,
                                            Ok(None) | Err(_) => break,
                                        }
                                    }
                                }
                                Ok(Some(Message::Request(req))) => {
                                    r.dispatch_rpc(&s, &mut *stream, req).await;
                                    let _ = stream.finish().await;
                                    // 读尽剩余帧，避免 drop 未读完的接收流触发对端写错误
                                    while let Ok(Some(_)) = stream.read_message().await {}
                                }
                                Ok(Some(Message::Stream(frame))) => {
                                    r.dispatch_stream(&s, stream, frame).await;
                                }
                                Ok(Some(_)) => { /* 忽略不支持的帧类型 */ }
                                Ok(None) | Err(_) => {}
                            }
                        });
                    }
                    Err(_) => break,
                }
            }
            // 单向流：事件 / 流数据
            uni = conn.accept_uni() => {
                match uni {
                    Ok(mut recv) => match recv.read_message().await {
                        Ok(Some(Message::Event(event))) => {
                            // 事件流可能是长连接通道（复用流）或一次性流：
                            // 统一交给读任务处理（首帧 + 持续帧），主循环不阻塞在分发上
                            let s = session.clone();
                            let r = router.clone();
                            tokio::spawn(async move {
                                let mut recv = recv;
                                r.dispatch_event(&s, event).await;
                                loop {
                                    match recv.read_message().await {
                                        Ok(Some(Message::Event(e))) => {
                                            r.dispatch_event(&s, e).await;
                                        }
                                        Ok(Some(_)) => continue,
                                        Ok(None) | Err(_) => break,
                                    }
                                }
                            });
                        }
                        Ok(Some(Message::Stream(frame))) => {
                            router.dispatch_stream(&session, recv, frame).await;
                        }
                        Ok(Some(_)) => { /* 忽略不支持的帧类型 */ }
                        Ok(None) | Err(_) => break,
                    },
                    Err(_) => break,
                }
            }
        }
    }
    ctx.unregister_session(session.id());
    tracing::debug!("客户端断开: session {}", session.id());
}

/// 服务端构建器
pub struct ServerBuilder {
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
    listener: Option<Arc<dyn Listener>>,
    #[cfg(feature = "quic")]
    quic_addr: Option<String>,
    on_start: Vec<Hook<ServerContext>>,
    on_stop: Vec<Hook<ServerContext>>,
    on_connect: Vec<Hook<Session>>,
    on_disconnect: Vec<Hook<Session>>,
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerBuilder {
    /// 创建构建器
    pub fn new() -> Self {
        Self {
            router: Arc::new(Router::default()),
            ctx: Arc::new(ServerContext::new()),
            listener: None,
            #[cfg(feature = "quic")]
            quic_addr: None,
            on_start: Vec::new(),
            on_stop: Vec::new(),
            on_connect: Vec::new(),
            on_disconnect: Vec::new(),
        }
    }

    /// 使用传输层监听器（QUIC / WebSocket / WebTransport 等）
    pub fn listener(mut self, listener: Arc<dyn Listener>) -> Self {
        self.listener = Some(listener);
        self
    }

    /// 使用 QUIC 监听器（feature = "quic"）
    #[cfg(feature = "quic")]
    pub fn bind_quic(mut self, addr: impl Into<String>) -> Self {
        self.quic_addr = Some(addr.into());
        self
    }

    /// 使用 QUIC 监听器（便捷别名，feature = "quic"）
    #[cfg(feature = "quic")]
    pub fn bind(self, addr: impl Into<String>) -> Self {
        self.bind_quic(addr)
    }

    /// 使用现有的处理器注册表（供各语言绑定层注入处理器）
    pub fn with_router(mut self, router: Arc<Router>) -> Self {
        self.router = router;
        self
    }

    /// 使用现有的上下文（供各语言绑定层共享会话与状态）
    pub fn with_ctx(mut self, ctx: Arc<ServerContext>) -> Self {
        self.ctx = ctx;
        self
    }

    /// 注册 RPC 处理器
    pub fn add_rpc<H: DynRpcHandler>(self, handler: H) -> Self {
        self.router.add_rpc(handler);
        self
    }

    /// 注册事件处理器（同名事件支持多个监听器）
    pub fn add_event<H: DynEventHandler>(self, handler: H) -> Self {
        self.router.add_event(handler);
        self
    }

    /// 注册流处理器
    pub fn add_stream<H: StreamHandler>(self, handler: H) -> Self {
        self.router.add_stream(handler);
        self
    }

    /// 添加中间件（数据面：鉴权、日志、拦截等）
    pub fn middleware<M: Middleware>(self, middleware: M) -> Self {
        self.router.add_middleware(middleware);
        self
    }

    /// 添加插件（控制面：打包处理器与钩子）
    pub fn plugin<P: ServerPlugin>(self, plugin: P) -> Self {
        (Box::new(plugin) as Box<dyn ServerPlugin>).install(self)
    }

    /// 服务启动钩子
    pub fn on_start<F>(mut self, f: F) -> Self
    where
        F: Fn(&ServerContext) + Send + Sync + 'static,
    {
        self.on_start.push(Arc::new(f));
        self
    }

    /// 服务关闭钩子
    pub fn on_stop<F>(mut self, f: F) -> Self
    where
        F: Fn(&ServerContext) + Send + Sync + 'static,
    {
        self.on_stop.push(Arc::new(f));
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

    /// 访问服务端全局上下文（可提前注册全局状态）
    pub fn ctx(&self) -> &Arc<ServerContext> {
        &self.ctx
    }

    /// 构建服务端
    pub async fn build(self) -> echostream_proto::Result<Server> {
        #[cfg(feature = "quic")]
        let listener = match self.listener {
            Some(l) => l,
            None => {
                let addr = self.quic_addr.ok_or_else(|| {
                    echostream_proto::Error::InvalidParameter("未指定监听器或监听地址".into())
                })?;
                Arc::new(crate::quic::QuicEndpoint::bind(addr).await?) as Arc<dyn Listener>
            }
        };
        #[cfg(not(feature = "quic"))]
        let listener = self
            .listener
            .ok_or_else(|| echostream_proto::Error::InvalidParameter("未指定监听器".into()))?;
        Ok(Server {
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

    /// 构建并运行（阻塞直到服务关闭）
    pub async fn serve(self) -> echostream_proto::Result<()> {
        self.build().await?.run().await
    }
}
