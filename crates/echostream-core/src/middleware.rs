//! 中间件：数据面的消息拦截与转换（洋葱链）

use async_trait::async_trait;
use echostream_proto::{Message, Result};

use crate::session::Session;

/// 中间件：处理入站消息（RPC 请求 / 事件 / 流帧）
///
/// 返回 `Ok(None)` 表示拦截该消息（不再继续分发），
/// 返回 `Ok(Some(msg))` 可修改消息内容（如注入元数据、改写载荷）。
#[async_trait]
pub trait Middleware: Send + Sync + 'static {
    /// 中间件名称
    fn name(&self) -> &str;

    /// 处理消息
    async fn on_message(&self, session: &Session, msg: Message) -> Result<Option<Message>>;
}
