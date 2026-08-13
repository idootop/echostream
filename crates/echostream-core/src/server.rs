//! 服务端：监听、接受连接、消息分发

use std::sync::Arc;

use echostream_proto::Message;
use echostream_transport::QuicEndpoint;

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
    endpoint: QuicEndpoint,
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
    on_start: Vec<Hook<ServerContext>>,
    on_stop: Vec<Hook<ServerContext>>,
    on_connect: Vec<Hook<Session>>,
    on_disconnect: Vec<Hook<Session>>,
}

impl Server {
    /// 运行服务（阻塞直到端点关闭）
    pub async fn run(self) -> echostream_proto::Result<()> {
        let Server {
            endpoint,
            router,
            ctx,
            on_start,
            on_stop,
            on_connect,
            on_disconnect,
        } = self;

        for hook in &on_start {
            hook(&ctx);
        }

        while let Some(conn) = endpoint.accept().await {
            {
                let session = Session::new(ctx.next_session_id(), conn, ctx.clone());
                ctx.register_session(session.clone());
                tracing::debug!(
                    "客户端连接: {} (session {})",
                    session.peer_addr(),
                    session.id()
                );
                let s = session.clone();
                for hook in &on_connect {
                    hook(&s);
                }
                let hooks = on_disconnect.clone();
                let r = router.clone();
                let c = ctx.clone();
                tokio::spawn(async move {
                    handle_connection(session, r, c).await;
                    for hook in &hooks {
                        hook(&s);
                    }
                });
            }
        }

        for hook in &on_stop {
            hook(&ctx);
        }
        Ok(())
    }
}

/// 处理单个连接的消息循环
async fn handle_connection(session: Session, router: Arc<Router>, ctx: Arc<ServerContext>) {
    let conn = session.conn().clone();
    loop {
        tokio::select! {
            // 双向流：RPC 请求 / 流数据
            bi = conn.accept_bi() => {
                match bi {
                    Ok(mut stream) => match stream.read_message().await {
                        Ok(Some(Message::Request(req))) => {
                            router.dispatch_rpc(&session, &mut stream, req).await;
                            let _ = stream.finish();
                        }
                        Ok(Some(Message::Stream(frame))) => {
                            let (_, recv) = stream.split();
                            router.dispatch_stream(&session, recv, frame).await;
                        }
                        Ok(Some(_)) => { /* 忽略不支持的帧类型 */ }
                        Ok(None) | Err(_) => break,
                    },
                    Err(_) => break,
                }
            }
            // 单向流：事件 / 流数据
            uni = conn.accept_uni() => {
                match uni {
                    Ok(mut recv) => match recv.read_message().await {
                        Ok(Some(Message::Event(event))) => {
                            router.dispatch_event(&session, event).await;
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
    addr: Option<String>,
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
        let addr = self
            .addr
            .ok_or_else(|| echostream_proto::Error::InvalidParameter("未指定监听地址".into()))?;
        let endpoint = QuicEndpoint::bind(addr).await?;
        Ok(Server {
            endpoint,
            router: self.router,
            ctx: self.ctx,
            on_start: self.on_start,
            on_stop: self.on_stop,
            on_connect: self.on_connect,
            on_disconnect: self.on_disconnect,
        })
    }

    /// 构建并运行
    pub async fn serve(self) -> echostream_proto::Result<()> {
        self.build().await?.run().await
    }
}
