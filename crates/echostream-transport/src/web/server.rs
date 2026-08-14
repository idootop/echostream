//! WebTransport 服务端：监听、接受浏览器连接、消息分发
//!
//! 复用 `echostream-core` 的 Router / Session / ServerContext / Handler，
//! 与原生 QUIC 服务端共享同一套处理器体系。

use std::sync::Arc;

use echostream_core::{
    ServerContext, Session,
    handler::{DynEventHandler, DynRpcHandler, StreamHandler},
    middleware::Middleware,
    router::Router,
};
use echostream_proto::{Error, Message, Result};
use wtransport::endpoint::endpoint_side::Server as WtServer;
use wtransport::{Endpoint, Identity, ServerConfig};

use super::wt::WtConn;

/// 生命周期钩子类型
type Hook<T> = Arc<dyn Fn(&T) + Send + Sync>;

/// WebTransport 服务端
pub struct WebServer {
    endpoint: wtransport::Endpoint<WtServer>,
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
    on_start: Vec<Hook<ServerContext>>,
    on_stop: Vec<Hook<ServerContext>>,
    on_connect: Vec<Hook<Session>>,
    on_disconnect: Vec<Hook<Session>>,
    shutdown_signal: tokio::sync::Notify,
}

impl WebServer {
    /// 本地监听地址
    pub fn endpoint_addr(&self) -> Option<std::net::SocketAddr> {
        self.endpoint.local_addr().ok()
    }

    /// 运行服务（阻塞直到 `shutdown` 被调用）
    pub async fn run(&self) -> Result<()> {
        for hook in &self.on_start {
            hook(&self.ctx);
        }

        loop {
            tokio::select! {
                session = self.endpoint.accept() => {
                    let session_request = match session.await {
                        Ok(req) => req,
                        Err(e) => {
                            tracing::debug!("WebTransport 连接失败: {e}");
                            continue;
                        }
                    };
                    let conn = match session_request.accept().await {
                        Ok(conn) => conn,
                        Err(e) => {
                            tracing::debug!("WebTransport 会话握手失败: {e}");
                            continue;
                        }
                    };
                    let session = Session::new(
                        self.ctx.next_session_id(),
                        Arc::new(WtConn::new(conn)) as Arc<dyn echostream_core::Endpoint>,
                        self.ctx.clone(),
                    );
                    self.ctx.register_session(session.clone());
                    tracing::debug!(
                        "浏览器连接: {} (session {})",
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
        self.endpoint.close(0u32.into(), b"server closed");
    }
}

/// 处理单个连接的消息循环（与 QUIC 服务端相同的分派逻辑）
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
            bi = conn.accept_bi() => {
                match bi {
                    Ok(mut stream) => match stream.read_message().await {
                        // RPC 复用通道：长连接双向流上按 id 多路复用请求/响应
                        Ok(Some(Message::Request(req)))
                            if req.name == echostream_proto::RPC_CHANNEL_NAME =>
                        {
                            let s = session.clone();
                            let r = router.clone();
                            loop {
                                match stream.read_message().await {
                                    Ok(Some(Message::Request(req))) => {
                                        r.dispatch_rpc(&s, &mut *stream, req).await;
                                    }
                                    Ok(Some(_)) => continue,
                                    Ok(None) | Err(_) => break,
                                }
                            }
                        }
                        Ok(Some(Message::Request(req))) => {
                            router.dispatch_rpc(&session, &mut *stream, req).await;
                            let _ = stream.finish().await;
                            // 读尽剩余帧，避免 drop 未读完的接收流触发对端写错误
                            while let Ok(Some(_)) = stream.read_message().await {}
                        }
                        Ok(Some(Message::Stream(frame))) => {
                            router.dispatch_stream(&session, stream, frame).await;
                        }
                        Ok(Some(_)) => { /* 忽略不支持的帧类型 */ }
                        Ok(None) | Err(_) => break,
                    },
                    Err(_) => break,
                }
            }
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
    tracing::debug!("浏览器断开: session {}", session.id());
}

/// WebTransport 服务端构建器
pub struct WebServerBuilder {
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
    addr: Option<String>,
    identity: Option<Identity>,
    on_start: Vec<Hook<ServerContext>>,
    on_stop: Vec<Hook<ServerContext>>,
    on_connect: Vec<Hook<Session>>,
    on_disconnect: Vec<Hook<Session>>,
}

impl Default for WebServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl WebServerBuilder {
    /// 创建构建器
    pub fn new() -> Self {
        Self {
            router: Arc::new(Router::default()),
            ctx: Arc::new(ServerContext::new()),
            addr: None,
            identity: None,
            on_start: Vec::new(),
            on_stop: Vec::new(),
            on_connect: Vec::new(),
            on_disconnect: Vec::new(),
        }
    }

    /// 绑定监听地址（HTTP/3 + WebTransport）
    pub fn bind(mut self, addr: impl Into<String>) -> Self {
        self.addr = Some(addr.into());
        self
    }

    /// 使用指定身份（默认自动生成自签名证书）
    pub fn identity(mut self, identity: Identity) -> Self {
        self.identity = Some(identity);
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

    /// 访问服务端全局上下文
    pub fn ctx(&self) -> &Arc<ServerContext> {
        &self.ctx
    }

    /// 构建服务端（自动生成自签名证书）
    pub async fn build(self) -> Result<WebServer> {
        let addr = self
            .addr
            .ok_or_else(|| Error::InvalidParameter("未指定监听地址".into()))?;
        let addr = addr
            .parse::<std::net::SocketAddr>()
            .map_err(|e| Error::InvalidParameter(format!("监听地址无效: {e}")))?;
        let identity =
            Identity::self_signed(["localhost"]).map_err(|e| Error::Io(e.to_string()))?;
        let server_config = ServerConfig::builder()
            .with_bind_address(addr)
            .with_identity(identity)
            .build();
        let endpoint =
            Endpoint::<WtServer>::server(server_config).map_err(|e| Error::Io(e.to_string()))?;

        Ok(WebServer {
            endpoint,
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
