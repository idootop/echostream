use crate::{
    EchoError, EchoResult,
    bytes::Bytes,
    context::Context,
    listener::Listenable,
    message::Message,
    state::Stateful,
    types::{FrameSeq, RequestId, SessionStatus, StateKey, StreamId, String, Timestamp},
};
use async_trait::async_trait;
use std::any::Any;

/// 会话生命周期
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionStatus {
    /// 连接中
    Connecting,
    /// 已连接
    Connected,
    /// 断开中
    Disconnecting,
    /// 已断开
    Disconnected,
}

impl Default for SessionStatus {
    fn default() -> Self {
        Self::Disconnected
    }
}

/// 会话抽象
#[async_trait]
pub trait Session: Listenable<Context = Self> + Stateful + Send + Sync + 'static {
    /// 获取会话ID
    fn id(&self) -> &str;

    /// 获取当前会话状态
    async fn status(&self) -> SessionStatus;

    /// 获取关联的上下文
    fn context(&self) -> &dyn Context;

    // ===================== 核心通信方法 =====================
    /// 发消息
    fn send_message(&self, msg: Message) -> EchoResult<()>;

    /// 发送RPC请求（异步等待响应）
    async fn send_request(
        &self,
        handler: String,
        data: Bytes,
        metadata: Metadata,
        timeout: u64,
    ) -> EchoResult<Bytes>;

    /// 发送事件（无响应）
    async fn send_event(&self, name: String, data: Bytes) -> EchoResult<()>;

    /// 发送流数据帧
    async fn send_stream(
        &self,
        stream_id: StreamId,
        handler: String,
        seq: FrameSeq,
        timestamp: Timestamp,
        data: Bytes,
        metadata: Metadata,
        is_fin: bool,
    ) -> EchoResult<()>;
}

/// 上下文生命周期扩展
#[async_trait]
pub trait SessionLifecycle: Session {
    /// 连接会话
    async fn connect(&self) -> EchoResult<()>;

    /// 断开会话
    async fn disconnect(&self) -> EchoResult<()>;

    /// 注册连接成功回调
    fn on_connected<F: Fn(&dyn Session) + Send + Sync + 'static>(&self, f: F);

    /// 注册断开连接回调
    fn on_disconnected<F: Fn(&dyn Session) + Send + Sync + 'static>(&self, f: F);

    /// 注册错误回调
    fn on_error<F: Fn(&dyn Session, &EchoError) + Send + Sync + 'static>(&self, f: F);
}
