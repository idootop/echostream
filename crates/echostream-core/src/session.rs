//! 会话：单个连接的上下文，支持双向主动通信

use std::any::Any;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use echostream_proto::endpoint::{Endpoint, FrameIo};
use echostream_proto::{Error, EventMsg, Message, RequestMsg, Result};
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
            }),
        }
    }

    /// 会话 ID
    pub fn id(&self) -> u64 {
        self.inner.id
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

    /// 发起 RPC 请求（载荷为已编码字节，不做二次序列化；供各语言绑定使用）
    pub async fn request_raw(&self, name: &str, payload: Bytes) -> Result<Bytes> {
        self.request_raw_with_timeout(name, payload, self.inner.timeout)
            .await
    }

    /// 发起 RPC 请求（指定超时，载荷为已编码字节）
    pub async fn request_raw_with_timeout(
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

    /// 发起 RPC 请求并等待响应（指定超时）
    pub async fn request_with_timeout<Req: Serialize + Send, Resp: DeserializeOwned + Send>(
        &self,
        name: &str,
        req: &Req,
        timeout: std::time::Duration,
    ) -> Result<Resp> {
        let mut stream = self.inner.conn.open_bi().await?;
        let id = self.inner.next_msg_id.fetch_add(1, Ordering::Relaxed);
        stream
            .write_message(&Message::Request(RequestMsg {
                id,
                name: name.to_string(),
                data: codec::encode(req)?,
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
            codec::decode(&resp.data)
        } else {
            Err(Error::Rpc(resp.code.0, resp.message.unwrap_or_default()))
        }
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
