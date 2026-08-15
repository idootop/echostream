//! 客户端：连接、发起 RPC / 事件 / 流，并处理服务端主动调用
//!
//! 支持连接池（`ClientBuilder::pool(n)`）：多 QUIC 连接分摊流控窗口并
//! 跨核扩展吞吐（quinn 单连接为单任务处理），RPC 按轮询分发；事件与流走主连接。
//!
//! 生命周期：连接建立触发 `on_connect`、断开触发 `on_disconnect`（主动关闭除外），
//! 回调支持按 `HookId` 取消注册；`is_connected` 反映连接实时状态。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use echostream_proto::Message;
use echostream_proto::endpoint::Endpoint;

use crate::context::ServerContext;
use crate::handler::{DynEventHandler, DynRpcHandler, StreamHandler};
use crate::plugin::ClientPlugin;
use crate::router::Router;
use crate::session::Session;
use crate::stream::StreamSender;

/// 客户端生命周期回调（连接建立 / 断开时触发）
type LifecycleHook = Arc<dyn Fn(&Client) + Send + Sync>;

/// 回调注册 id（`add_on_*` 返回，供 `remove_on_*` 取消注册）
pub type HookId = u64;

/// 客户端（连接池：默认单连接）
#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    /// 连接池（RPC 轮询分发；事件/流走主连接 sessions[0]）
    sessions: std::sync::RwLock<Vec<Session>>,
    router: Arc<Router>,
    /// 连接是否在线（主连接接收循环运行中为 true）
    connected: AtomicBool,
    /// 连接建立回调（主连接建立 / 重连成功时触发）
    on_connect: std::sync::RwLock<Vec<(HookId, LifecycleHook)>>,
    /// 断开回调（连接断开时触发，主动关闭除外）
    on_disconnect: std::sync::RwLock<Vec<(HookId, LifecycleHook)>>,
    /// 回调 id 分配器
    next_hook_id: AtomicU64,
    /// 是否已主动关闭（关闭后不再触发断开回调）
    closed: AtomicBool,
    /// RPC 轮询游标
    next_session: AtomicUsize,
}

impl Client {
    /// 主会话快照（连接池首连接，可发起双向调用）
    pub fn session(&self) -> Session {
        self.inner
            .sessions
            .read()
            .unwrap()
            .first()
            .cloned()
            .unwrap()
    }

    /// 会话数（连接池大小）
    pub fn session_count(&self) -> usize {
        self.inner.sessions.read().unwrap().len()
    }

    /// 连接是否在线（主连接存活；`close` / 断线后为 false）
    pub fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::Relaxed)
    }

    /// 轮询选取会话（RPC 分发；连接池场景跨连接扩展）
    fn pick(&self) -> Session {
        let sessions = self.inner.sessions.read().unwrap();
        if sessions.len() <= 1 {
            return sessions.first().cloned().unwrap();
        }
        let idx = self.inner.next_session.fetch_add(1, Ordering::Relaxed) % sessions.len();
        sessions.get(idx).cloned().unwrap()
    }

    /// 替换会话（断线重连后调用；旧会话的连接将被关闭）
    ///
    /// 注意：重连后连接池收敛为单连接（新连接），可重新构建客户端恢复池。
    pub fn reconnect(&self, conn: Arc<dyn Endpoint>) {
        let (ctx, timeout) = {
            let guard = self.inner.sessions.read().unwrap();
            (
                guard.first().unwrap().ctx().clone(),
                guard.first().unwrap().timeout(),
            )
        };
        self.inner.closed.store(false, Ordering::Relaxed);
        let new_session = Session::with_timeout(ctx.next_session_id(), conn, ctx, timeout);
        let old = {
            let mut guard = self.inner.sessions.write().unwrap();
            std::mem::replace(&mut *guard, vec![new_session.clone()])
        };
        for s in old {
            s.close();
        }
        self.inner.connected.store(true, Ordering::Relaxed);
        tokio::spawn(receive_loop(self.clone(), new_session, true));
        self.notify_connect();
    }

    // ==================== 生命周期回调（可取消注册） ====================

    /// 注册连接建立回调，返回注册 id（`remove_on_connect` 取消注册）
    pub fn add_on_connect(&self, f: impl Fn(&Client) + Send + Sync + 'static) -> HookId {
        let id = self.inner.next_hook_id.fetch_add(1, Ordering::Relaxed);
        self.inner
            .on_connect
            .write()
            .unwrap()
            .push((id, Arc::new(f)));
        id
    }

    /// 取消注册连接建立回调
    pub fn remove_on_connect(&self, id: HookId) {
        self.inner
            .on_connect
            .write()
            .unwrap()
            .retain(|(i, _)| *i != id);
    }

    /// 注册断开回调（连接断开时触发；自动重连插件等使用），返回注册 id
    pub fn add_on_disconnect(&self, f: impl Fn(&Client) + Send + Sync + 'static) -> HookId {
        let id = self.inner.next_hook_id.fetch_add(1, Ordering::Relaxed);
        self.inner
            .on_disconnect
            .write()
            .unwrap()
            .push((id, Arc::new(f)));
        id
    }

    /// 取消注册断开回调
    pub fn remove_on_disconnect(&self, id: HookId) {
        self.inner
            .on_disconnect
            .write()
            .unwrap()
            .retain(|(i, _)| *i != id);
    }

    /// 触发连接建立回调（连接建立 / 重连成功时调用）
    fn notify_connect(&self) {
        if !self.is_connected() {
            return;
        }
        let hooks = self.inner.on_connect.read().unwrap().clone();
        for (_, h) in hooks {
            h(self);
        }
    }

    /// 触发断开回调（主连接接收循环结束时调用；主动关闭后不触发）
    fn notify_disconnect(&self) {
        if self.is_closed() {
            return;
        }
        let hooks = self.inner.on_disconnect.read().unwrap().clone();
        for (_, h) in hooks {
            h(self);
        }
    }

    /// 发起 RPC 请求（连接池场景自动轮询）
    pub async fn request<Req: serde::Serialize + Send, Resp: serde::de::DeserializeOwned + Send>(
        &self,
        name: &str,
        req: &Req,
    ) -> echostream_proto::Result<Resp> {
        self.pick().request(name, req).await
    }

    /// 发送单向事件（主连接）
    pub async fn emit<T: serde::Serialize + Send>(
        &self,
        name: &str,
        data: &T,
    ) -> echostream_proto::Result<()> {
        self.session().emit(name, data).await
    }

    /// 创建流（推送连续数据；主连接）
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
        self.pick().request_raw(name, payload).await
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
        self.inner.closed.store(true, Ordering::Relaxed);
        self.inner.connected.store(false, Ordering::Relaxed);
        let sessions = self.inner.sessions.read().unwrap().clone();
        for s in sessions {
            s.close();
        }
    }

    /// 是否已主动关闭
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Relaxed)
    }

    // ==================== 运行时注册（动态添加处理器） ====================

    /// 运行时注册事件监听
    pub fn add_event_handler<H: DynEventHandler>(&self, handler: H) {
        self.inner.router.add_event(handler);
    }

    /// 运行时注册 RPC 处理器（处理服务端主动调用）
    pub fn add_rpc_handler<H: DynRpcHandler>(&self, handler: H) {
        self.inner.router.add_rpc(handler);
    }

    /// 运行时注册流处理器
    pub fn add_stream_handler<H: StreamHandler>(&self, handler: H) {
        self.inner.router.add_stream(handler);
    }
}

/// 客户端构建器
pub struct ClientBuilder {
    router: Arc<Router>,
    timeout: std::time::Duration,
    on_connect: Vec<LifecycleHook>,
    on_disconnect: Vec<LifecycleHook>,
    pool_size: usize,
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
            on_connect: Vec::new(),
            on_disconnect: Vec::new(),
            pool_size: 1,
        }
    }

    /// 连接池大小（默认 1）：多 QUIC 连接分摊流控窗口并跨核扩展吞吐
    pub fn pool(mut self, n: usize) -> Self {
        self.pool_size = n.max(1);
        self
    }

    /// 添加客户端插件（重连/认证等控制面扩展）
    pub fn plugin<P: ClientPlugin>(self, plugin: P) -> Self {
        (Box::new(plugin) as Box<dyn ClientPlugin>).install(self)
    }

    /// 注册连接建立回调（连接建立 / 重连成功时触发）
    pub fn on_connect<F>(mut self, f: F) -> Self
    where
        F: Fn(&Client) + Send + Sync + 'static,
    {
        self.on_connect.push(Arc::new(f));
        self
    }

    /// 注册断开回调（连接断开时触发；主动关闭不触发）
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

    /// 使用传输层连接（QUIC / WebTransport 等；单连接）
    pub fn from_endpoint(self, conn: Arc<dyn Endpoint>) -> Client {
        self.from_endpoints(vec![conn])
    }

    /// 使用多个传输层连接（连接池）
    pub fn from_endpoints(self, conns: Vec<Arc<dyn Endpoint>>) -> Client {
        let ctx = Arc::new(ServerContext::new());
        let sessions: Vec<Session> = conns
            .into_iter()
            .map(|conn| {
                Session::with_timeout(ctx.next_session_id(), conn, ctx.clone(), self.timeout)
            })
            .collect();
        // 回调 id：构建期注册的钩子从 1 开始编号，运行时注册延续
        let mut next_hook_id = 1u64;
        let on_connect: Vec<(HookId, LifecycleHook)> = self
            .on_connect
            .into_iter()
            .map(|f| {
                let id = next_hook_id;
                next_hook_id += 1;
                (id, f)
            })
            .collect();
        let on_disconnect: Vec<(HookId, LifecycleHook)> = self
            .on_disconnect
            .into_iter()
            .map(|f| {
                let id = next_hook_id;
                next_hook_id += 1;
                (id, f)
            })
            .collect();
        let client = Client {
            inner: Arc::new(ClientInner {
                sessions: std::sync::RwLock::new(sessions.clone()),
                router: self.router,
                connected: AtomicBool::new(true),
                on_connect: std::sync::RwLock::new(on_connect),
                on_disconnect: std::sync::RwLock::new(on_disconnect),
                next_hook_id: AtomicU64::new(next_hook_id),
                closed: AtomicBool::new(false),
                next_session: AtomicUsize::new(0),
            }),
        };
        for (i, session) in sessions.into_iter().enumerate() {
            // 主连接（sessions[0]）驱动生命周期；池中辅助连接断开仅静默移除
            tokio::spawn(receive_loop(client.clone(), session, i == 0));
        }
        client.notify_connect();
        client
    }

    /// 连接池大小（供 echostream-transport 便捷 connect 使用）
    pub fn pool_size(&self) -> usize {
        self.pool_size
    }
}

/// 客户端接收循环：处理服务端主动发来的 RPC / 事件 / 流
///
/// primary：主连接（sessions[0]）——负责生命周期状态与回调；
/// 连接池辅助连接断开时仅从池中移除，不触发断开回调。
async fn receive_loop(client: Client, session: Session, primary: bool) {
    let conn = session.conn_arc();
    // 数据报接收任务（不可靠事件通道）
    if conn.supports_datagram() {
        let c = client.clone();
        let s = session.clone();
        tokio::spawn(async move {
            let conn = s.conn_arc();
            while let Ok(data) = conn.recv_datagram().await {
                if let Ok(msg) = postcard::from_bytes(&data) {
                    c.inner.router.dispatch_inbound_datagram(&s, msg).await;
                }
            }
        });
    }
    loop {
        tokio::select! {
            bi = conn.accept_bi() => {
                match bi {
                    Ok(stream) => {
                        // spawn 处理：RPC 复用通道等长连接场景不得阻塞 accept 主循环
                        let c = client.clone();
                        let s = session.clone();
                        tokio::spawn(async move {
                            let mut stream = stream;
                            match stream.read_message().await {
                                // RPC 复用通道：长连接双向流上按 id 多路复用请求/响应
                                Ok(Some(Message::Request(req)))
                                    if req.name == echostream_proto::RPC_CHANNEL_NAME =>
                                {
                                    loop {
                                        match stream.read_message().await {
                                            Ok(Some(Message::Request(req))) => {
                                                c.inner
                                                    .router
                                                    .dispatch_rpc(&s, &mut *stream, req)
                                                    .await;
                                            }
                                            Ok(Some(_)) => continue,
                                            Ok(None) | Err(_) => break,
                                        }
                                    }
                                }
                                Ok(Some(Message::Request(req))) => {
                                    c.inner.router.dispatch_rpc(&s, &mut *stream, req).await;
                                    let _ = stream.finish().await;
                                }
                                Ok(Some(Message::Stream(frame))) => {
                                    c.inner.router.dispatch_stream(&s, stream, frame).await;
                                }
                                Ok(Some(_)) => { /* 忽略不支持的帧类型 */ }
                                Ok(None) | Err(_) => {}
                            }
                        });
                    }
                    Err(_) => break,
                }
            }
            uni = conn.accept_uni() => {
                match uni {
                    Ok(mut recv) => match recv.read_message().await {
                        Ok(Some(Message::Event(event))) => {
                            client.inner.router.dispatch_event(&session, event).await;
                        }
                        Ok(Some(Message::Stream(frame))) => {
                            client.inner.router.dispatch_stream(&session, recv, frame).await;
                        }
                        Ok(Some(_)) => { /* 忽略不支持的帧类型 */ }
                        Ok(None) | Err(_) => break,
                    },
                    Err(_) => break,
                }
            }
        }
    }
    tracing::debug!("连接已断开 (session {})", session.id());
    if primary {
        client.inner.connected.store(false, Ordering::Relaxed);
        client.notify_disconnect();
    } else if !client.is_closed() {
        // 连接池辅助连接断开：静默移除，不触发生命周期回调
        client
            .inner
            .sessions
            .write()
            .unwrap()
            .retain(|s| s.id() != session.id());
    }
}
