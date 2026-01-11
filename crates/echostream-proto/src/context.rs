use crate::{
    EchoError, EchoResult,
    listener::Listenable,
    state::Stateful,
    types::{ContextStatus, StateKey},
};
use async_trait::async_trait;
use std::any::Any;

/// 上下文生命周期
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextStatus {
    /// 初始化中
    Initializing,
    /// 运行中
    Running,
    /// 停止中
    Stopping,
    /// 已停止
    Stopped,
}

impl Default for ContextStatus {
    fn default() -> Self {
        Self::Initializing
    }
}

/// 上下文抽象
#[async_trait]
pub trait Context: Listenable<Context = Self> + Stateful + Send + Sync + 'static {
    /// 获取当前生命周期阶段
    async fn status(&self) -> ContextStatus;
}

/// 上下文生命周期扩展
#[async_trait]
pub trait ContextLifecycle: Context {
    /// 启动上下文
    async fn start(&self) -> EchoResult<()>;

    /// 停止上下文
    async fn stop(&self) -> EchoResult<()>;

    /// 注册连接成功回调
    fn on_started<F: Fn(&dyn Session) + Send + Sync + 'static>(&self, f: F);

    /// 注册断开连接回调
    fn on_stopped<F: Fn(&dyn Session) + Send + Sync + 'static>(&self, f: F);
}
