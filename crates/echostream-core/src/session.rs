//! 会话：单个连接的上下文，支持双向主动通信

use std::any::Any;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::oneshot;

use bytes::Bytes;
use echostream_proto::endpoint::{Endpoint, FrameIo};
use echostream_proto::{Error, EventMsg, Message, RPC_CHANNEL_NAME, RequestMsg, Result};
use serde::{Serialize, de::DeserializeOwned};

use crate::codec;
use crate::context::ServerContext;
use crate::stream::StreamSender;

/// 请求默认超时时间
const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// 会话（Clone 共享）
#[derive(Clone)]
pub struct Session {
    inner: Arc<SessionInner>,
}

struct SessionInner {
    id: u64,
    conn: Arc<dyn Endpoint>,
    ctx: Arc<ServerContext>,
    state: RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>,
    next_msg_id: AtomicU64,
    timeout: std::time::Duration,
    /// 事件通道：复用一条单向流批量发送事件帧（避免每次 emit 的开流开销）
    event_channel: tokio::sync::Mutex<Option<Box<dyn FrameIo>>>,
    /// RPC 复用通道：多路复用请求/响应于一条双向流（避免每次请求的开流开销）
    rpc_channel: tokio::sync::Mutex<Option<Arc<RpcChannel>>>,
}

/// 复用通道的载荷阈值：超过则走独立流（避免大响应在通道上造成队头阻塞）
const RPC_CHANNEL_MAX_PAYLOAD: usize = 64 * 1024;

/// RPC 复用通道：一条长连接双向流上按请求 id 多路复用（高频小请求优化，
/// 消除每次请求的流打开/关闭与包开销；大载荷请走独立流 request_raw）
///
/// 底层双向流拆分为读写半部：读循环独占接收端（阻塞等待响应帧），
/// 写入端并发发送请求，互不阻塞。
struct RpcChannel {
    send: tokio::sync::Mutex<Box<dyn FrameIo>>,
    next_id: AtomicU64,
    pending: RwLock<HashMap<u64, oneshot::Sender<Result<Bytes>>>>,
}

impl RpcChannel {
    fn new(send: Box<dyn FrameIo>) -> Arc<Self> {
        Arc::new(Self {
            send: tokio::sync::Mutex::new(send),
            next_id: AtomicU64::new(1),
            pending: RwLock::new(HashMap::new()),
        })
    }

    /// 读取循环：路由响应帧到对应请求（独占接收半部）
    async fn reader_loop(self: Arc<Self>, mut recv: Box<dyn FrameIo>) {
        loop {
            let msg = match recv.read_message().await {
                Ok(Some(m)) => m,
                Ok(None) | Err(_) => break,
            };
            if let Message::Response(resp) = msg {
                if let Some(tx) = self.pending.write().unwrap().remove(&resp.id) {
                    let _ = tx.send(if resp.code.is_success() {
                        Ok(resp.data)
                    } else {
                        Err(Error::Rpc(resp.code.0, resp.message.unwrap_or_default()))
                    });
                }
            }
        }
        // 通道关闭：唤醒所有等待中的请求
        let pending = std::mem::take(&mut *self.pending.write().unwrap());
        for (_, tx) in pending {
            let _ = tx.send(Err(Error::SessionClosed));
        }
    }
}

impl Session {
    /// 创建会话（内部使用，供各传输实现调用）
    pub fn new(id: u64, conn: Arc<dyn Endpoint>, ctx: Arc<ServerContext>) -> Self {
        Self::with_timeout(id, conn, ctx, DEFAULT_TIMEOUT)
    }

    /// 创建会话并指定请求超时（内部使用）
    pub fn with_timeout(
        id: u64,
        conn: Arc<dyn Endpoint>,
        ctx: Arc<ServerContext>,
        timeout: std::time::Duration,
    ) -> Self {
        Self {
            inner: Arc::new(SessionInner {
                id,
                conn,
                ctx,
                state: RwLock::new(HashMap::new()),
                next_msg_id: AtomicU64::new(1),
                timeout,
                event_channel: tokio::sync::Mutex::new(None),
                rpc_channel: tokio::sync::Mutex::new(None),
            }),
        }
    }

    /// 会话 ID
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// 请求超时
    pub fn timeout(&self) -> std::time::Duration {
        self.inner.timeout
    }

    /// 底层连接（Arc 共享，供接收循环使用）
    pub fn conn_arc(&self) -> Arc<dyn Endpoint> {
        self.inner.conn.clone()
    }

    /// 对端地址
    pub fn peer_addr(&self) -> SocketAddr {
        self.inner.conn.peer_addr()
    }

    /// 服务端全局上下文
    pub fn ctx(&self) -> &Arc<ServerContext> {
        &self.inner.ctx
    }

    // ==================== 会话级状态 ====================

    /// 存储会话数据
    pub fn set<T: Send + Sync + 'static>(&self, key: impl Into<String>, value: T) {
        self.inner
            .state
            .write()
            .unwrap()
            .insert(key.into(), Arc::new(value));
    }

    /// 获取会话数据
    pub fn get<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> {
        self.inner
            .state
            .read()
            .unwrap()
            .get(key)
            .and_then(|v| v.clone().downcast::<T>().ok())
    }

    /// 移除会话数据
    pub fn remove(&self, key: &str) {
        self.inner.state.write().unwrap().remove(key);
    }

    // ==================== 双向主动通信 ====================

    /// 发起 RPC 请求（走复用通道：一条长连接双向流多路复用，高频小请求性能更好）
    ///
    /// 通道按需建立（首请求时开启），响应按请求 id 路由；大载荷请求建议用
    /// `request_raw`（独立流，无队头阻塞）。
    pub async fn request_muxed(&self, name: &str, payload: Bytes) -> Result<Bytes> {
        self.request_muxed_with_timeout(name, payload, self.inner.timeout)
            .await
    }

    /// 发起 RPC 请求（复用通道 + 指定超时）
    pub async fn request_muxed_with_timeout(
        &self,
        name: &str,
        payload: Bytes,
        timeout: std::time::Duration,
    ) -> Result<Bytes> {
        let chan = {
            let mut guard = self.inner.rpc_channel.lock().await;
            if guard.is_none() {
                let io = self.inner.conn.open_bi().await?;
                // 拆分为读写半部：读循环与写入并发，互不阻塞
                let (mut send, recv) = io.split()?;
                // 通道开启标记：保留方法名，对端据此进入通道模式
                send.write_message(&Message::Request(RequestMsg {
                    id: 0,
                    name: RPC_CHANNEL_NAME.into(),
                    data: Bytes::new(),
                }))
                .await?;
                let chan = RpcChannel::new(send);
                tokio::spawn(chan.clone().reader_loop(recv));
                *guard = Some(chan);
            }
            guard.as_ref().unwrap().clone()
        };
        let id = chan.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        chan.pending.write().unwrap().insert(id, tx);
        {
            let mut send = chan.send.lock().await;
            send.write_message(&Message::Request(RequestMsg {
                id,
                name: name.to_string(),
                data: payload,
            }))
            .await?;
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::SessionClosed),
            Err(_) => {
                chan.pending.write().unwrap().remove(&id);
                Err(Error::Timeout(id))
            }
        }
    }

    /// 发起 RPC 请求（载荷为已编码字节，不做二次序列化；供各语言绑定使用）
    ///
    /// 默认走复用通道（高频小请求性能更好）；载荷超过通道阈值（64KiB）自动
    /// 切换到独立流（避免大响应在通道上造成队头阻塞）。
    pub async fn request_raw(&self, name: &str, payload: Bytes) -> Result<Bytes> {
        self.request_raw_with_timeout(name, payload, self.inner.timeout)
            .await
    }

    /// 发起 RPC 请求（指定超时，载荷为已编码字节；自动选择通道/独立流）
    pub async fn request_raw_with_timeout(
        &self,
        name: &str,
        payload: Bytes,
        timeout: std::time::Duration,
    ) -> Result<Bytes> {
        if payload.len() <= RPC_CHANNEL_MAX_PAYLOAD {
            self.request_muxed_with_timeout(name, payload, timeout)
                .await
        } else {
            self.request_raw_stream_with_timeout(name, payload, timeout)
                .await
        }
    }

    /// 发起 RPC 请求（独立双向流；适合大载荷与流式响应场景）
    pub async fn request_raw_stream(&self, name: &str, payload: Bytes) -> Result<Bytes> {
        self.request_raw_stream_with_timeout(name, payload, self.inner.timeout)
            .await
    }

    /// 发起 RPC 请求（独立双向流 + 指定超时）
    pub async fn request_raw_stream_with_timeout(
        &self,
        name: &str,
        payload: Bytes,
        timeout: std::time::Duration,
    ) -> Result<Bytes> {
        let mut stream = self.inner.conn.open_bi().await?;
        let id = self.inner.next_msg_id.fetch_add(1, Ordering::Relaxed);
        stream
            .write_message(&Message::Request(RequestMsg {
                id,
                name: name.to_string(),
                data: payload,
            }))
            .await?;
        stream.finish().await?;

        let resp = tokio::time::timeout(timeout, async {
            loop {
                match stream.read_message().await? {
                    Some(Message::Response(r)) if r.id == id => return Ok(r),
                    Some(_) => continue, // 忽略不匹配的帧
                    None => return Err(Error::SessionClosed),
                }
            }
        })
        .await
        .map_err(|_| Error::Timeout(id))??;

        if resp.code.is_success() {
            Ok(resp.data)
        } else {
            Err(Error::Rpc(resp.code.0, resp.message.unwrap_or_default()))
        }
    }

    /// 发送单向事件（载荷为已编码字节）
    ///
    /// 复用事件通道（长连接单向流批量发送），避免每次 emit 的开流开销。
    pub async fn emit_raw(&self, name: &str, payload: Bytes) -> Result<()> {
        let msg = Message::Event(EventMsg {
            id: self.inner.next_msg_id.fetch_add(1, Ordering::Relaxed),
            name: name.to_string(),
            data: payload,
        });
        let mut chan = self.inner.event_channel.lock().await;
        // 通道不存在或已失效时重建
        if chan.is_none() {
            *chan = Some(self.inner.conn.open_uni().await?);
        }
        if let Err(e) = chan.as_mut().unwrap().write_message(&msg).await {
            // 通道失效（对端关闭）：重建后重试一次
            tracing::debug!("事件通道失效，重建: {e}");
            *chan = Some(self.inner.conn.open_uni().await?);
            chan.as_mut().unwrap().write_message(&msg).await?;
        }
        Ok(())
    }

    /// 发起 RPC 请求并等待响应（使用默认超时）
    pub async fn request<Req: Serialize + Send, Resp: DeserializeOwned + Send>(
        &self,
        name: &str,
        req: &Req,
    ) -> Result<Resp> {
        self.request_with_timeout(name, req, self.inner.timeout)
            .await
    }

    /// 发起 RPC 请求并等待响应（指定超时；默认走复用通道，大载荷自动切独立流）
    pub async fn request_with_timeout<Req: Serialize + Send, Resp: DeserializeOwned + Send>(
        &self,
        name: &str,
        req: &Req,
        timeout: std::time::Duration,
    ) -> Result<Resp> {
        let data = codec::encode(req)?;
        let resp = self.request_raw_with_timeout(name, data, timeout).await?;
        codec::decode(&resp)
    }

    /// 发送单向事件
    pub async fn emit<T: Serialize + Send>(&self, name: &str, data: &T) -> Result<()> {
        let mut stream = self.inner.conn.open_uni().await?;
        stream
            .write_message(&Message::Event(EventMsg {
                id: self.inner.next_msg_id.fetch_add(1, Ordering::Relaxed),
                name: name.to_string(),
                data: codec::encode(data)?,
            }))
            .await?;
        stream.finish().await
    }

    /// 发送不可靠事件（数据报通道；不保证到达与顺序，吞吐更高）
    ///
    /// 载荷为已编码字节；传输不支持数据报时返回错误（调用方可降级为 `emit_raw`）。
    pub async fn emit_unreliable_raw(&self, name: &str, payload: Bytes) -> Result<()> {
        if !self.inner.conn.supports_datagram() {
            return Err(Error::Protocol("传输不支持数据报通道".into()));
        }
        let msg = Message::Event(EventMsg {
            id: self.inner.next_msg_id.fetch_add(1, Ordering::Relaxed),
            name: name.to_string(),
            data: payload,
        });
        let data = postcard::to_allocvec(&msg).map_err(|e| Error::Serialization(e.to_string()))?;
        self.inner.conn.send_datagram(Bytes::from(data))
    }

    /// 创建流（推送连续数据）
    pub async fn create_stream(&self, name: &str) -> Result<StreamSender> {
        let send = self.inner.conn.open_uni().await?;
        let id = self.inner.next_msg_id.fetch_add(1, Ordering::Relaxed);
        Ok(StreamSender::new(send, id, name.to_string()))
    }

    /// 关闭连接
    pub fn close(&self) {
        self.inner.conn.close();
    }

    /// 底层连接（内部使用，供传输实现调用）
    pub fn conn(&self) -> &dyn Endpoint {
        self.inner.conn.as_ref()
    }
}
