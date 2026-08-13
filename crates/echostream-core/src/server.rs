//! 服务端：监听、接受连接、消息分发

use std::sync::Arc;

use echostream_proto::Message;
use echostream_transport::QuicEndpoint;

use crate::context::ServerContext;
use crate::handler::{DynEventHandler, DynRpcHandler, StreamHandler};
use crate::router::Router;
use crate::session::Session;

/// 服务端
pub struct Server {
    endpoint: QuicEndpoint,
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
}

impl Server {
    /// 运行服务（阻塞直到端点关闭）
    pub async fn run(self) -> echostream_proto::Result<()> {
        let Server {
            endpoint,
            router,
            ctx,
        } = self;
        loop {
            match endpoint.accept().await {
                Some(conn) => {
                    let session = Session::new(ctx.next_session_id(), conn, ctx.clone());
                    ctx.register_session(session.clone());
                    tracing::debug!("客户端连接: {} (session {})", session.peer_addr(), session.id());
                    tokio::spawn(handle_connection(session, router.clone(), ctx.clone()));
                }
                None => return Ok(()),
            }
        }
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
        })
    }

    /// 构建并运行
    pub async fn serve(self) -> echostream_proto::Result<()> {
        self.build().await?.run().await
    }
}
