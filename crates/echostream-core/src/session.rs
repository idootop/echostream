//! 会话：单个连接的上下文，支持双向主动通信

use std::any::Any;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use echostream_proto::{Error, EventMsg, Message, RequestMsg, Result};
use echostream_transport::QuicConn;
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
    conn: QuicConn,
    ctx: Arc<ServerContext>,
    state: RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>,
    next_msg_id: AtomicU64,
}

impl Session {
    pub(crate) fn new(id: u64, conn: QuicConn, ctx: Arc<ServerContext>) -> Self {
        Self {
            inner: Arc::new(SessionInner {
                id,
                conn,
                ctx,
                state: RwLock::new(HashMap::new()),
                next_msg_id: AtomicU64::new(1),
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

    /// 发起 RPC 请求并等待响应
    pub async fn request<Req: Serialize + Send, Resp: DeserializeOwned + Send>(
        &self,
        name: &str,
        req: &Req,
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
        stream.finish()?;

        let resp = tokio::time::timeout(DEFAULT_TIMEOUT, async {
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
        stream.finish()
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

    /// 底层连接（内部使用）
    pub(crate) fn conn(&self) -> &QuicConn {
        &self.inner.conn
    }
}
