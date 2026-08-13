//! 客户端：连接、发起 RPC / 事件 / 流，并处理服务端主动调用

use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use echostream_proto::Message;
use echostream_proto::endpoint::Endpoint;

use crate::context::ServerContext;
use crate::handler::{DynEventHandler, DynRpcHandler, StreamHandler};
use crate::plugin::ClientPlugin;
use crate::router::Router;
use crate::session::Session;
use crate::stream::StreamSender;

/// 客户端
#[derive(Clone)]
pub struct Client {
    session: Arc<std::sync::RwLock<Session>>,
    router: Arc<Router>,
    /// 断开回调（重连插件等使用）
    on_disconnect: Arc<std::sync::RwLock<Vec<Arc<dyn Fn(&Client) + Send + Sync>>>>,
    /// 是否已主动关闭（关闭后不再触发断开回调）
    closed: Arc<AtomicBool>,
}

impl Client {
    /// 底层会话快照（可发起双向调用）
    pub fn session(&self) -> Session {
        self.session.read().unwrap().clone()
    }

    /// 替换会话（断线重连后调用；旧会话的连接将被关闭）
    pub fn reconnect(&self, conn: Arc<dyn Endpoint>) {
        let (ctx, timeout) = {
            let guard = self.session.read().unwrap();
            (guard.ctx().clone(), guard.timeout())
        };
        self.closed.store(false, Ordering::Relaxed);
        let new_session = Session::with_timeout(ctx.next_session_id(), conn, ctx, timeout);
        let old = {
            let mut guard = self.session.write().unwrap();
            std::mem::replace(&mut *guard, new_session)
        };
        old.close();
        tokio::spawn(receive_loop(self.clone()));
    }

    /// 注册断开回调（连接断开时触发；自动重连插件等使用）
    pub fn add_on_disconnect(&self, f: impl Fn(&Client) + Send + Sync + 'static) {
        self.on_disconnect
            .write()
            .unwrap()
            .push(Arc::new(f));
    }

    /// 触发断开回调（接收循环结束时调用；主动关闭后不触发）
    fn notify_disconnect(&self) {
        if self.is_closed() {
            return;
        }
        let hooks = self.on_disconnect.read().unwrap().clone();
        for h in hooks {
            h(self);
        }
    }

    /// 发起 RPC 请求
    pub async fn request<Req: serde::Serialize + Send, Resp: serde::de::DeserializeOwned + Send>(
        &self,
        name: &str,
        req: &Req,
    ) -> echostream_proto::Result<Resp> {
        self.session().request(name, req).await
    }

    /// 发送单向事件
    pub async fn emit<T: serde::Serialize + Send>(
        &self,
        name: &str,
        data: &T,
    ) -> echostream_proto::Result<()> {
        self.session().emit(name, data).await
    }

    /// 创建流（推送连续数据）
    pub async fn create_stream(&self, name: &str) -> echostream_proto::Result<StreamSender> {
        self.session().create_stream(name).await
    }

    /// 发送不可靠事件（数据报通道）
    pub async fn emit_unreliable_raw(
        &self,
        name: &str,
        payload: bytes::Bytes,
    ) -> echostream_proto::Result<()> {
        self.session().emit_unreliable_raw(name, payload).await
    }

    /// 发起 RPC 请求（载荷为已编码字节，供各语言绑定使用）
    pub async fn request_raw(
        &self,
        name: &str,
        payload: bytes::Bytes,
    ) -> echostream_proto::Result<bytes::Bytes> {
        self.session().request_raw(name, payload).await
    }

    /// 发送单向事件（载荷为已编码字节）
    pub async fn emit_raw(
        &self,
        name: &str,
        payload: bytes::Bytes,
    ) -> echostream_proto::Result<()> {
        self.session().emit_raw(name, payload).await
    }

    /// 主动关闭连接（关闭后断开回调不再触发）
    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
        self.session().close();
    }

    /// 是否已主动关闭
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    // ==================== 运行时注册（动态添加处理器） ====================

    /// 运行时注册事件监听
    pub fn add_event_handler<H: DynEventHandler>(&self, handler: H) {
        self.router.add_event(handler);
    }

    /// 运行时注册 RPC 处理器（处理服务端主动调用）
    pub fn add_rpc_handler<H: DynRpcHandler>(&self, handler: H) {
        self.router.add_rpc(handler);
    }

    /// 运行时注册流处理器
    pub fn add_stream_handler<H: StreamHandler>(&self, handler: H) {
        self.router.add_stream(handler);
    }
}

/// 客户端构建器
pub struct ClientBuilder {
    router: Arc<Router>,
    timeout: std::time::Duration,
    on_disconnect: Vec<Arc<dyn Fn(&Client) + Send + Sync>>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientBuilder {
    /// 创建构建器
    pub fn new() -> Self {
        Self {
            router: Arc::new(Router::default()),
            timeout: std::time::Duration::from_secs(30),
            on_disconnect: Vec::new(),
        }
    }

    /// 添加客户端插件（重连/认证等控制面扩展）
    pub fn plugin<P: ClientPlugin>(self, plugin: P) -> Self {
        (Box::new(plugin) as Box<dyn ClientPlugin>).install(self)
    }

    /// 注册断开回调（连接断开时触发）
    pub fn on_disconnect<F>(mut self, f: F) -> Self
    where
        F: Fn(&Client) + Send + Sync + 'static,
    {
        self.on_disconnect.push(Arc::new(f));
        self
    }

    /// 设置 RPC 请求默认超时（默认 30 秒）
    pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 注册 RPC 处理器（处理服务端主动调用）
    pub fn add_rpc<H: DynRpcHandler>(self, handler: H) -> Self {
        self.router.add_rpc(handler);
        self
    }

    /// 注册事件处理器（接收服务端推送的事件）
    pub fn add_event<H: DynEventHandler>(self, handler: H) -> Self {
        self.router.add_event(handler);
        self
    }

    /// 注册流处理器（接收服务端推送的流）
    pub fn add_stream<H: StreamHandler>(self, handler: H) -> Self {
        self.router.add_stream(handler);
        self
    }

    /// 使用传输层连接（QUIC / WebTransport 等）
    pub fn from_endpoint(self, conn: Arc<dyn Endpoint>) -> Client {
        let ctx = Arc::new(ServerContext::new());
        let session = Session::with_timeout(ctx.next_session_id(), conn, ctx.clone(), self.timeout);
        let client = Client {
            session: Arc::new(std::sync::RwLock::new(session)),
            router: self.router,
            on_disconnect: Arc::new(std::sync::RwLock::new(self.on_disconnect)),
            closed: Arc::new(AtomicBool::new(false)),
        };
        tokio::spawn(receive_loop(client.clone()));
        client
    }

    /// 使用 QUIC 连接到服务端（feature = "quic"）
    #[cfg(feature = "quic")]
    pub async fn connect(self, addr: impl ToSocketAddrs) -> echostream_proto::Result<Client> {
        let addr = addr
            .to_socket_addrs()
            .map_err(|e| echostream_proto::Error::Io(e.to_string()))?
            .next()
            .ok_or_else(|| {
                echostream_proto::Error::InvalidParameter("无法解析服务端地址".into())
            })?;
        let conn = echostream_transport::connect(addr).await?;
        Ok(self.from_endpoint(Arc::new(conn)))
    }
}

/// 客户端接收循环：处理服务端主动发来的 RPC / 事件 / 流
async fn receive_loop(client: Client) {
    let conn = client.session().conn_arc();
    // 数据报接收任务（不可靠事件通道）
    if conn.supports_datagram() {
        let c = client.clone();
        tokio::spawn(async move {
            let conn = c.session().conn_arc();
            while let Ok(data) = conn.recv_datagram().await {
                if let Ok(msg) = postcard::from_bytes(&data) {
                    c.router.dispatch_inbound_datagram(&c.session(), msg).await;
                }
            }
        });
    }
    loop {
        tokio::select! {
            bi = conn.accept_bi() => {
                match bi {
                    Ok(mut stream) => match stream.read_message().await {
                        Ok(Some(Message::Request(req))) => {
                            client.router.dispatch_rpc(&client.session(), &mut *stream, req).await;
                            let _ = stream.finish().await;
                        }
                        Ok(Some(Message::Stream(frame))) => {
                            client.router.dispatch_stream(&client.session(), stream, frame).await;
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
                            client.router.dispatch_event(&client.session(), event).await;
                        }
                        Ok(Some(Message::Stream(frame))) => {
                            client.router.dispatch_stream(&client.session(), recv, frame).await;
                        }
                        Ok(Some(_)) => { /* 忽略不支持的帧类型 */ }
                        Ok(None) | Err(_) => break,
                    },
                    Err(_) => break,
                }
            }
        }
    }
    tracing::debug!("连接已断开");
    client.notify_disconnect();
}
