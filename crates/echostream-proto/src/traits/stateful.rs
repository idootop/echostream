use crate::{EchoError, EchoResult, traits::container::DynamicMap};
use async_trait::async_trait;

/// 状态管理抽象接口
#[async_trait]
pub trait Stateful: Send + Sync + 'static {
    fn get_state_manager(&self) -> &DynamicMap;

    async fn set_state<T: Send + Sync + 'static>(&self, key: impl Into<String> + Send, value: T) {
        self.get_state_manager().set(key.into(), value).await
    }

    async fn get_state<T: Send + Sync + 'static>(&self, key: &str) -> Option<T> {
        self.get_state_manager().get(key).await
    }

    async fn remove_state(&self, key: &str) {
        self.get_state_manager().remove(key).await
    }

    async fn clear_all_states(&self) {
        self.get_state_manager().clear().await
    }
}
