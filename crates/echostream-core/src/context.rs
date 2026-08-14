//! 应用级上下文：全局状态存储 + 会话管理

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use echostream_proto::Result;
use serde::Serialize;

use crate::session::Session;

/// 服务端全局上下文（Clone 共享）
#[derive(Clone)]
pub struct ServerContext {
    inner: Arc<ServerContextInner>,
}

struct ServerContextInner {
    state: RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>,
    sessions: RwLock<HashMap<u64, Session>>,
    next_session_id: AtomicU64,
}

impl Default for ServerContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerContext {
    /// 创建上下文（内部使用）
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ServerContextInner {
                state: RwLock::new(HashMap::new()),
                sessions: RwLock::new(HashMap::new()),
                next_session_id: AtomicU64::new(1),
            }),
        }
    }

    /// 存储全局数据
    pub fn set<T: Send + Sync + 'static>(&self, key: impl Into<String>, value: T) {
        self.inner
            .state
            .write()
            .unwrap()
            .insert(key.into(), Arc::new(value));
    }

    /// 获取全局数据
    pub fn get<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> {
        self.inner
            .state
            .read()
            .unwrap()
            .get(key)
            .and_then(|v| v.clone().downcast::<T>().ok())
    }

    /// 移除全局数据
    pub fn remove(&self, key: &str) {
        self.inner.state.write().unwrap().remove(key);
    }

    /// 所有在线会话
    pub fn sessions(&self) -> Vec<Session> {
        self.inner
            .sessions
            .read()
            .unwrap()
            .values()
            .cloned()
            .collect()
    }

    /// 广播事件到所有在线会话（自动序列化）
    pub async fn broadcast<T: Serialize + Send>(&self, name: &str, data: &T) -> Result<()> {
        for session in self.sessions() {
            session.emit(name, data).await?;
        }
        Ok(())
    }

    /// 广播事件到所有在线会话（载荷为已编码字节，不做二次序列化）
    pub async fn broadcast_raw(&self, name: &str, payload: Bytes) -> Result<()> {
        for session in self.sessions() {
            session.emit_raw(name, payload.clone()).await?;
        }
        Ok(())
    }

    // ---------- 内部接口（供各传输实现调用） ----------

    /// 分配下一个会话 ID（内部使用）
    pub fn next_session_id(&self) -> u64 {
        self.inner
            .next_session_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// 注册会话（内部使用）
    pub fn register_session(&self, session: Session) {
        self.inner
            .sessions
            .write()
            .unwrap()
            .insert(session.id(), session);
    }

    /// 注销会话（内部使用）
    pub fn unregister_session(&self, id: u64) {
        self.inner.sessions.write().unwrap().remove(&id);
    }
}
