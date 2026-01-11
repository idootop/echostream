use crate::{EchoError, EchoResult};
use async_trait::async_trait;

/// 状态管理实现接口
#[async_trait]
pub trait StateManagerTrait: Send + Sync + 'static {
    // 写入操作通常需要所有权
    async fn set_state<T: Send + Sync + 'static>(&self, key: String, value: T);

    // 查询操作建议统一使用 &str，因为查询不应消耗 Key 的所有权
    async fn get_state<T: Send + Sync + 'static>(&self, key: &str) -> Option<T>;

    async fn remove_state(&self, key: &str) -> EchoResult<()>;

    async fn clear_state(&self);
}

/// 状态管理抽象接口
#[async_trait]
pub trait Stateful: Send + Sync + 'static {
    type StateManager: StateManagerTrait;

    // ===== 外部需要实现的接口 =====

    /// 获取状态管理器实例
    fn get_state_manager(&self) -> &Self::StateManager;

    // ===== 代理给具体的 Manager 实现=====
    async fn set_state<T: Send + Sync + 'static>(&self, key: impl Into<String> + Send, value: T) {
        // 调用 into() 转换成具体的 String 传递给 manager
        self.get_state_manager().set_state(key.into(), value).await;
    }

    async fn get_state<T: Send + Sync + 'static>(&self, key: &str) -> Option<T> {
        self.get_state_manager().get_state(key).await
    }

    async fn remove_state(&self, key: &str) -> EchoResult<()> {
        self.get_state_manager().remove_state(key).await
    }

    async fn clear_state(&self) {
        self.get_state_manager().clear_state().await;
    }
}
