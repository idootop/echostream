use async_trait::async_trait;

use crate::{
    error::EchoResult,
    message::{EventMsg, RequestMsg},
    traits::container::DynamicMap,
};

#[async_trait]
pub trait Handler: Send + Sync + 'static {
    fn name(&self) -> &str;
}

#[async_trait]
pub trait RpcHandler: Handler {
    async fn handle(&self, session: &dyn Session, request: RequestMsg) -> EchoResult<ResponseMsg>;
}

#[async_trait]
pub trait EventHandler: Handler {
    async fn handle(&self, session: &dyn Session, event: EventMsg) -> EchoResult<()>;
}

#[async_trait]
pub trait StreamHandler: Handler {
    async fn handle(&self, session: &dyn Session, stream: StreamMsg) -> EchoResult<()>;
}

/// 处理器注册表抽象
#[async_trait]
pub trait HandlerRegistry: Send + Sync + 'static {
    /// 获取状态管理器实例
    fn get_handler_manager(&self) -> &DynamicMap;

    /// 注册处理器
    async fn register_handler<H: Handler>(&self, handler: H) {
        let key = handler.name().to_string();
        let boxed_handler = Box::new(handler);
        self.get_handler_manager()
            .set::<Box<H>>(key, boxed_handler)
            .await
    }

    /// 获取处理器
    async fn get_handler<H: Handler>(&self, name: &str) -> EchoResult<&H> {
        let boxed_handler = self.get_handler_manager().get::<Box<H>>(name).await;
        match boxed_handler {
            Some(handler) => Ok(&*handler),
            None => Err(format!("处理器 '{}' 未找到", name).into()),
        }
    }

    /// 移除处理器
    async fn unregister_handler(&self, name: &str) {
        self.get_handler_manager().remove(name).await
    }

    /// 清空所有处理器
    async fn clear_handlers(&self) {
        self.get_handler_manager().clear().await
    }
}
