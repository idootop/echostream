use crate::{EchoError, EchoResult};
use async_trait::async_trait;

/// 事件处理器类型（上下文 + 事件数据）
pub type ListenerHandler<EventContext> =
    Arc<dyn Fn(EventContext, Box<dyn Any + Send + Sync>) -> EchoResult<()> + Send + Sync>;

/// 事件管理实现接口
#[async_trait]
pub trait EventManagerTrait<EventContext: Send + Sync + 'static>: Send + Sync + 'static {
    async fn add_listener(&self, name: String, handler: ListenerHandler<EventContext>) -> String;
    async fn remove_listener(&self, name: &str, listener_id: &str) -> EchoResult<()>;
    async fn trigger_event<EventData: Send + Sync + 'static>(
        &self,
        context: EventContext,
        name: String,
        data: EventData,
    ) -> EchoResult<()>;
}

/// 事件管理抽象接口
#[async_trait]
pub trait Listenable: Send + Sync + 'static {
    type EventContext: Clone + Send + Sync + 'static;
    type EventManager: EventManagerTrait<Self::EventContext>;

    // ===== 外部需要实现的接口 =====

    /// 获取事件管理器实例
    fn get_event_manager(&self) -> &Self::EventManager;
    /// 获取事件上下文
    fn get_event_context(&self) -> Self::EventContext;

    // ===== 代理给具体的 Manager 实现=====
    async fn add_listener<EventData: Clone + Send + Sync + 'static>(
        &self,
        name: impl Into<String> + Send,
        handler: impl Fn(Self::EventContext, EventData) -> EchoResult<()> + Send + Sync + 'static,
    ) -> String {
        // 包装 handler 为通用类型（仅做类型转换，无业务逻辑）
        let wrapped_handler = Arc::new(
            move |ctx: Self::EventContext, data: Box<dyn Any + Send + Sync>| {
                // 1. 向下转型为具体的 EventData
                let data = data.downcast_ref::<EventData>().ok_or_else(|| {
                    format!(
                        "事件数据类型不匹配，期望 {}",
                        std::any::type_name::<EventData>()
                    )
                })?;
                handler(ctx.clone(), data.clone())
            },
        ) as ListenerHandler<Self::EventContext>;

        // 代理给抽象的 EventManagerTrait
        self.get_event_manager()
            .add_listener(name.into(), wrapped_handler)
            .await
    }

    async fn remove_listener(&self, name: &str, listener_id: &str) -> EchoResult<()> {
        self.get_event_manager()
            .remove_listener(name, listener_id)
            .await
    }

    async fn trigger_event<EventData: Send + Sync + 'static>(
        &self,
        name: impl Into<String> + Send,
        data: EventData,
    ) -> EchoResult<()> {
        self.get_event_manager()
            .trigger_event(self.get_event_context(), name.into(), data)
            .await
    }
}
