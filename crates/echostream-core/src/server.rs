//! 服务端：监听、接受连接、消息分发

use std::sync::Arc;

use echostream_proto::Message;

use echostream_proto::endpoint::Listener;

use crate::context::ServerContext;
use crate::handler::{DynEventHandler, DynRpcHandler, StreamHandler};
use crate::middleware::Middleware;
use crate::plugin::ServerPlugin;
use crate::router::{Router, Token};
use crate::session::Session;

/// 生命周期钩子类型：同步回调（异步逻辑可在回调内 tokio::spawn）
type Hook<T> = Arc<dyn Fn(&T) + Send + Sync>;

/// 服务端（hook 支持运行时注册 / 按 HookId 取消注册）
pub struct Server {
    listener: Arc<dyn Listener>,
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
    on_start: std::sync::RwLock<Vec<(HookId, Hook<ServerContext>)>>,
    on_stop: std::sync::RwLock<Vec<(HookId, Hook<ServerContext>)>>,
    on_connect: std::sync::RwLock<Vec<(HookId, Hook<Session>)>>,
    on_disconnect: std::sync::RwLock<Vec<(HookId, Hook<Session>)>>,
    next_hook_id: std::sync::atomic::AtomicU64,
    shutdown_signal: tokio::sync::Notify,
}

/// 回调注册 id（add_on_* 返回，供 remove_on_* 取消注册）
pub type HookId = u64;

impl Server {
    /// 本地监听地址
    pub fn endpoint_addr(&self) -> Option<std::net::SocketAddr> {
        self.listener.local_addr().ok()
    }

    /// 处理器注册表（运行时注册 RPC / 事件 / 流 / 中间件）
    pub fn router(&self) -> &Arc<Router> {
        &self.router
    }

    /// 服务端全局上下文
    pub fn ctx(&self) -> &Arc<ServerContext> {
        &self.ctx
    }

    /// 运行时注册 RPC 处理器，返回注册 token（remove_rpc 取消注册）
    pub fn add_rpc<H: DynRpcHandler>(&self, handler: H) -> Token {
        self.router.add_rpc(handler)
    }

    /// 批量注册 RPC 处理器，返回各注册 token
    pub fn add_rpcs<H: DynRpcHandler>(&self, handlers: impl IntoIterator<Item = H>) -> Vec<Token> {
        self.router.add_rpcs(handlers)
    }

    /// 取消注册 RPC 处理器（按注册 token）
    pub fn remove_rpc(&self, token: Token) -> bool {
        self.router.remove_rpc(token)
    }

    /// 运行时注册事件监听器，返回注册 token（remove_event 取消注册）
    pub fn add_event<H: DynEventHandler>(&self, handler: H) -> Token {
        self.router.add_event(handler)
    }

    /// 批量注册事件监听器，返回各注册 token
    pub fn add_events<H: DynEventHandler>(
        &self,
        handlers: impl IntoIterator<Item = H>,
    ) -> Vec<Token> {
        self.router.add_events(handlers)
    }

    /// 取消注册事件监听器（按注册 token）
    pub fn remove_event(&self, token: Token) -> bool {
        self.router.remove_event(token)
    }

    /// 运行时注册流处理器，返回注册 token（remove_stream 取消注册）
    pub fn add_stream<H: StreamHandler>(&self, handler: H) -> Token {
        self.router.add_stream(handler)
    }

    /// 批量注册流处理器，返回各注册 token
    pub fn add_streams<H: StreamHandler>(
        &self,
        handlers: impl IntoIterator<Item = H>,
    ) -> Vec<Token> {
        self.router.add_streams(handlers)
    }

    /// 取消注册流处理器（按注册 token）
    pub fn remove_stream(&self, token: Token) -> bool {
        self.router.remove_stream(token)
    }

    // ==================== 生命周期钩子（运行时注册 / 取消注册） ====================

    /// 注册服务启动钩子，返回注册 id
    pub fn add_on_start(&self, f: impl Fn(&ServerContext) + Send + Sync + 'static) -> HookId {
        self.register_hook(&self.on_start, f)
    }

    /// 取消注册服务启动钩子
    pub fn remove_on_start(&self, id: HookId) {
        self.on_start.write().unwrap().retain(|(i, _)| *i != id);
    }

    /// 注册服务关闭钩子，返回注册 id
    pub fn add_on_stop(&self, f: impl Fn(&ServerContext) + Send + Sync + 'static) -> HookId {
        self.register_hook(&self.on_stop, f)
    }

    /// 取消注册服务关闭钩子
    pub fn remove_on_stop(&self, id: HookId) {
        self.on_stop.write().unwrap().retain(|(i, _)| *i != id);
    }

    /// 注册客户端连接钩子，返回注册 id
    pub fn add_on_connect(&self, f: impl Fn(&Session) + Send + Sync + 'static) -> HookId {
        self.register_hook(&self.on_connect, f)
    }

    /// 取消注册客户端连接钩子
    pub fn remove_on_connect(&self, id: HookId) {
        self.on_connect.write().unwrap().retain(|(i, _)| *i != id);
    }

    /// 注册客户端断开钩子，返回注册 id
    pub fn add_on_disconnect(&self, f: impl Fn(&Session) + Send + Sync + 'static) -> HookId {
        self.register_hook(&self.on_disconnect, f)
    }

    /// 取消注册客户端断开钩子
    pub fn remove_on_disconnect(&self, id: HookId) {
        self.on_disconnect
            .write()
            .unwrap()
            .retain(|(i, _)| *i != id);
    }

    fn register_hook<T>(
        &self,
        slots: &std::sync::RwLock<Vec<(HookId, Hook<T>)>>,
        f: impl Fn(&T) + Send + Sync + 'static,
    ) -> HookId {
        let id = self
            .next_hook_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        slots.write().unwrap().push((id, Arc::new(f)));
        id
    }

    /// 运行服务（阻塞直到 `shutdown` 被调用）
    pub async fn run(&self) -> echostream_proto::Result<()> {
        for (_, hook) in self.on_start.read().unwrap().clone() {
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
                            // 中间件连接钩子
                            self.router.run_connect_hooks(&session).await;
                            let s = session.clone();
                            for (_, hook) in self.on_connect.read().unwrap().clone() {
                                hook(&s);
                            }
                            let hooks = self.on_disconnect.read().unwrap().clone();
                            let r = self.router.clone();
                            let c = self.ctx.clone();
                            tokio::spawn(async move {
                                handle_connection(session, r, c).await;
                                for (_, hook) in hooks {
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

        for (_, hook) in self.on_stop.read().unwrap().clone() {
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
                                Ok(Some(Message::StreamOpen(open))) => {
                                    r.dispatch_stream(&s, stream, open).await;
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
                        Ok(Some(Message::StreamOpen(open))) => {
                            router.dispatch_stream(&session, recv, open).await;
                        }
                        Ok(Some(_)) => { /* 忽略不支持的帧类型 */ }
                        Ok(None) | Err(_) => break,
                    },
                    Err(_) => break,
                }
            }
        }
    }
    // 中间件断开钩子（在用户钩子之前，保证插件观测先于业务通知）
    router.run_disconnect_hooks(&session).await;
    ctx.unregister_session(session.id());
    tracing::debug!("客户端断开: session {}", session.id());
}

/// 监听器工厂：build 时延迟创建传输监听器（供 echostream-transport 便捷 bind 使用）
pub type ListenerFactory = Arc<
    dyn Fn() -> futures::future::BoxFuture<'static, echostream_proto::Result<Arc<dyn Listener>>>
        + Send
        + Sync,
>;

/// 服务端构建器
pub struct ServerBuilder {
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
    listener: Option<Arc<dyn Listener>>,
    listener_factory: Option<ListenerFactory>,
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
            listener_factory: None,
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

    /// 使用监听器工厂（build 时延迟创建；供 echostream-transport 便捷 bind 使用）
    pub fn listener_factory(mut self, factory: ListenerFactory) -> Self {
        self.listener_factory = Some(factory);
        self
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

    /// 批量注册 RPC 处理器
    pub fn add_rpcs<H: DynRpcHandler>(self, handlers: impl IntoIterator<Item = H>) -> Self {
        self.router.add_rpcs(handlers);
        self
    }

    /// 注册事件处理器（同名事件支持多个监听器）
    pub fn add_event<H: DynEventHandler>(self, handler: H) -> Self {
        self.router.add_event(handler);
        self
    }

    /// 批量注册事件处理器
    pub fn add_events<H: DynEventHandler>(self, handlers: impl IntoIterator<Item = H>) -> Self {
        self.router.add_events(handlers);
        self
    }

    /// 注册流处理器
    pub fn add_stream<H: StreamHandler>(self, handler: H) -> Self {
        self.router.add_stream(handler);
        self
    }

    /// 批量注册流处理器
    pub fn add_streams<H: StreamHandler>(self, handlers: impl IntoIterator<Item = H>) -> Self {
        self.router.add_streams(handlers);
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
        let listener = match self.listener {
            Some(l) => l,
            None => match self.listener_factory {
                Some(factory) => factory().await?,
                None => {
                    return Err(echostream_proto::Error::InvalidParameter(
                        "未指定监听器（listener() 或 listener_factory()）".into(),
                    ));
                }
            },
        };
        // 构建期注册的钩子编号从 1 开始，运行时注册延续
        let mut next_hook_id = 1u64;
        let on_start = self
            .on_start
            .into_iter()
            .map(|h| {
                let id = next_hook_id;
                next_hook_id += 1;
                (id, h)
            })
            .collect::<Vec<_>>();
        let on_stop = self
            .on_stop
            .into_iter()
            .map(|h| {
                let id = next_hook_id;
                next_hook_id += 1;
                (id, h)
            })
            .collect::<Vec<_>>();
        let on_connect = self
            .on_connect
            .into_iter()
            .map(|h| {
                let id = next_hook_id;
                next_hook_id += 1;
                (id, h)
            })
            .collect::<Vec<_>>();
        let on_disconnect = self
            .on_disconnect
            .into_iter()
            .map(|h| {
                let id = next_hook_id;
                next_hook_id += 1;
                (id, h)
            })
            .collect::<Vec<_>>();
        Ok(Server {
            listener,
            router: self.router,
            ctx: self.ctx,
            on_start: std::sync::RwLock::new(on_start),
            on_stop: std::sync::RwLock::new(on_stop),
            on_connect: std::sync::RwLock::new(on_connect),
            on_disconnect: std::sync::RwLock::new(on_disconnect),
            next_hook_id: std::sync::atomic::AtomicU64::new(next_hook_id),
            shutdown_signal: tokio::sync::Notify::new(),
        })
    }

    /// 构建并运行（阻塞直到服务关闭）
    pub async fn serve(self) -> echostream_proto::Result<()> {
        self.build().await?.run().await
    }
}
