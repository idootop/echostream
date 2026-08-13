//! EchoStream Node.js binding（napi-rs）
//!
//! 载荷约定：所有 RPC/Event/Stream 载荷均为 postcard 编码字节（Buffer），
//! 与 Rust 侧线缆格式一致，Node 端可复用 `sdk/web/postcard.js` 编解码。

use std::sync::Arc;

use bytes::Bytes;
use echostream::prelude::*;
use echostream::{DynEventHandler, DynRpcHandler, Router};
use napi::bindgen_prelude::*;
use napi::threadsafe_function::ThreadsafeFunction;
use napi_derive::napi;

fn to_napi_err(e: echostream::Error) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

/// 连接服务端（QUIC）
#[napi]
pub async fn connect(url: String) -> napi::Result<JsClient> {
    let client = ClientBuilder::new()
        .connect(&url)
        .await
        .map_err(to_napi_err)?;
    Ok(JsClient {
        client: Arc::new(client),
    })
}

/// EchoStream 客户端
#[napi]
pub struct JsClient {
    client: Arc<Client>,
}

#[napi]
impl JsClient {
    /// 发起 RPC 请求，返回响应载荷（postcard 字节）
    #[napi]
    pub async fn request(&self, name: String, payload: Vec<u8>) -> napi::Result<Vec<u8>> {
        let data = Bytes::from(payload);
        let resp: Bytes = self
            .client
            .request_raw(&name, data)
            .await
            .map_err(to_napi_err)?;
        Ok(resp.to_vec())
    }

    /// 发送单向事件
    #[napi]
    pub async fn emit(&self, name: String, payload: Vec<u8>) -> napi::Result<()> {
        let data = Bytes::from(payload);
        self.client.emit_raw(&name, data).await.map_err(to_napi_err)
    }

    /// 创建流（推送连续数据）
    #[napi]
    pub async fn create_stream(&self, name: String) -> napi::Result<JsStream> {
        let stream = self
            .client
            .create_stream(&name)
            .await
            .map_err(to_napi_err)?;
        Ok(JsStream {
            inner: tokio::sync::Mutex::new(stream),
        })
    }

    /// 注册事件监听（回调收到事件载荷 Buffer）
    #[napi]
    pub fn on_event(&self, name: String, callback: ThreadsafeFunction<Vec<u8>>) {
        let handler = JsEventCallback {
            name: name.clone(),
            callback,
        };
        // 客户端事件处理器注册：通过构建器重建成本高，直接注册到内部 router
        // 使用 add_event 需要 ClientBuilder —— Client 内部 router 不可变，这里
        // 采用 Runtime 注册方式：core Client 提供 on_event 注册。
        self.client.add_event_handler(handler);
    }

    /// 关闭连接
    #[napi]
    pub fn close(&self) {
        self.client.close();
    }
}

/// 事件回调适配（ThreadsafeFunction → EventHandler）
struct JsEventCallback {
    name: String,
    callback: ThreadsafeFunction<Vec<u8>>,
}

#[async_trait::async_trait]
impl DynEventHandler for JsEventCallback {
    fn name(&self) -> &str {
        &self.name
    }

    async fn handle_encoded(&self, _session: &Session, data: Bytes) -> echostream::Result<()> {
        self.callback
            .call_async::<()>(Ok(data.to_vec()))
            .await
            .map_err(|e| echostream::Error::Io(e.to_string()))?;
        Ok(())
    }
}

/// 流发送器（Node 侧句柄）
#[napi]
pub struct JsStream {
    inner: tokio::sync::Mutex<StreamSender>,
}

#[napi]
impl JsStream {
    /// 发送一帧
    #[napi]
    pub async fn send(&self, payload: Vec<u8>) -> napi::Result<()> {
        let mut stream = self.inner.lock().await;
        stream.send(Bytes::from(payload)).await.map_err(to_napi_err)
    }

    /// 关闭流
    #[napi]
    pub async fn finish(&self) -> napi::Result<()> {
        let mut stream = self.inner.lock().await;
        stream.finish().await.map_err(to_napi_err)
    }
}

// ======================== 服务端绑定 ========================

/// Node 侧 RPC 处理器（JS 回调：payload Buffer → 返回 Buffer / Promise<Buffer>）
struct JsRpcHandler {
    name: String,
    callback: ThreadsafeFunction<Vec<u8>>,
}

#[async_trait::async_trait]
impl DynRpcHandler for JsRpcHandler {
    fn name(&self) -> &str {
        &self.name
    }

    async fn handle_encoded(
        &self,
        _session: &Session,
        payload: Bytes,
    ) -> echostream::Result<Bytes> {
        let resp: Promise<Buffer> = self
            .callback
            .call_async::<Promise<Buffer>>(Ok(payload.to_vec()))
            .await
            .map_err(|e| echostream::Error::Io(e.to_string()))?;
        let buf: Buffer = resp
            .await
            .map_err(|e| echostream::Error::Io(e.to_string()))?;
        Ok(Bytes::copy_from_slice(buf.as_ref()))
    }
}

/// Node 侧事件处理器（JS 回调：payload Buffer → 无返回值）
struct JsEventHandler {
    name: String,
    callback: ThreadsafeFunction<Vec<u8>>,
}

#[async_trait::async_trait]
impl DynEventHandler for JsEventHandler {
    fn name(&self) -> &str {
        &self.name
    }

    async fn handle_encoded(&self, _session: &Session, payload: Bytes) -> echostream::Result<()> {
        self.callback
            .call_async::<()>(Ok(payload.to_vec()))
            .await
            .map_err(|e| echostream::Error::Io(e.to_string()))?;
        Ok(())
    }
}

/// 服务端（Node 侧句柄）
#[napi]
pub struct JsServer {
    server: Server,
    ctx: Arc<ServerContext>,
}

#[napi]
impl JsServer {
    /// 运行服务（Promise 在 `shutdown` 后 resolve）
    #[napi]
    pub async fn run(&self) -> napi::Result<()> {
        self.server.run().await.map_err(to_napi_err)
    }

    /// 优雅关闭
    #[napi]
    pub fn shutdown(&self) {
        self.server.shutdown();
    }

    /// 本地监听地址
    #[napi]
    pub fn addr(&self) -> Option<String> {
        self.server.endpoint_addr().map(|a| a.to_string())
    }

    /// 广播事件到所有连接客户端
    #[napi]
    pub async fn broadcast(&self, name: String, payload: Vec<u8>) -> napi::Result<()> {
        self.ctx
            .broadcast(&name, &Bytes::from(payload))
            .await
            .map_err(to_napi_err)
    }

    /// 所有在线会话（可主动调用客户端）
    #[napi]
    pub fn sessions(&self) -> Vec<JsSession> {
        self.ctx
            .sessions()
            .into_iter()
            .map(JsSession::new)
            .collect()
    }
}

/// 会话（服务端视角：可主动调用客户端）
#[napi]
pub struct JsSession {
    session: Session,
}

impl JsSession {
    fn new(session: Session) -> Self {
        Self { session }
    }
}

#[napi]
impl JsSession {
    /// 会话 ID
    #[napi]
    pub fn id(&self) -> u64 {
        self.session.id()
    }

    /// 对端地址
    #[napi]
    pub fn peer_addr(&self) -> String {
        self.session.peer_addr().to_string()
    }

    /// 主动调用客户端 RPC
    #[napi]
    pub async fn request(&self, name: String, payload: Vec<u8>) -> napi::Result<Vec<u8>> {
        let resp: Bytes = self
            .session
            .request_raw(&name, Bytes::from(payload))
            .await
            .map_err(to_napi_err)?;
        Ok(resp.to_vec())
    }

    /// 向客户端发送事件
    #[napi]
    pub async fn emit(&self, name: String, payload: Vec<u8>) -> napi::Result<()> {
        self.session
            .emit_raw(&name, Bytes::from(payload))
            .await
            .map_err(to_napi_err)
    }

    /// 关闭连接
    #[napi]
    pub fn close(&self) {
        self.session.close();
    }
}

/// 服务端构建器
#[napi]
pub struct JsServerBuilder {
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
    addr: Option<String>,
}

impl Default for JsServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[napi]
impl JsServerBuilder {
    /// 创建构建器
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            router: Arc::new(Router::default()),
            ctx: Arc::new(ServerContext::new()),
            addr: None,
        }
    }

    /// 绑定监听地址
    #[napi]
    pub fn bind(&mut self, addr: String) {
        self.addr = Some(addr);
    }

    /// 注册 RPC 处理器（回调：payload Buffer → Buffer / Promise<Buffer>）
    #[napi]
    pub fn add_rpc(&self, name: String, callback: ThreadsafeFunction<Vec<u8>>) {
        self.router.add_rpc(JsRpcHandler { name, callback });
    }

    /// 注册事件处理器（回调：payload Buffer）
    #[napi]
    pub fn add_event(&self, name: String, callback: ThreadsafeFunction<Vec<u8>>) {
        self.router.add_event(JsEventHandler { name, callback });
    }

    /// 构建服务端
    #[napi]
    pub async fn build(&self) -> napi::Result<JsServer> {
        let addr = self
            .addr
            .clone()
            .ok_or_else(|| napi::Error::from_reason("未指定监听地址"))?;
        let server = ServerBuilder::new()
            .with_router(self.router.clone())
            .with_ctx(self.ctx.clone())
            .bind(addr)
            .build()
            .await
            .map_err(to_napi_err)?;
        Ok(JsServer {
            server,
            ctx: self.ctx.clone(),
        })
    }
}
