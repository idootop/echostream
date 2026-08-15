//! 中间件：数据面的消息拦截与转换（洋葱链）
//!
//! 与业界主流（tower / axum / koa）一致：中间件通过 `next.run(msg)` 将控制权
//! 交给下游，可在此前后做增强 ——
//! - 修改消息：`next.run(修改后的 msg)`
//! - 拦截：不调用 next，直接返回 `Ok(None)`
//! - 包裹下游：`let result = next.run(msg).await; ...; result`（计时 / 超时 / 错误归一化）
//!
//! 连接生命周期钩子（`on_connect` / `on_disconnect`）默认为空实现。

use std::sync::Arc;

use async_trait::async_trait;
use echostream_proto::{Message, Result};
use futures::future::BoxFuture;

use crate::session::Session;

/// 中间件链的后续处理（洋葱模型）
///
/// 在 `Middleware::handle` 中调用 `next.run(msg)` 继续执行链；
/// 链的终点是路由分派（RPC 处理器 / 事件监听器 / 流处理器）。
/// 注意：每个中间件至多调用一次 `run`（调用多次属中间件 bug）。
#[derive(Clone)]
pub struct Next {
    pub(crate) chain: Arc<Vec<Arc<dyn Middleware>>>,
    pub(crate) session: Session,
    pub(crate) idx: usize,
    pub(crate) terminal:
        Arc<dyn Fn(Message) -> BoxFuture<'static, Result<Option<Message>>> + Send + Sync>,
}

impl Next {
    /// 继续执行中间件链（最终到达终端处理器）
    pub async fn run(self, msg: Message) -> Result<Option<Message>> {
        if self.idx >= self.chain.len() {
            return (self.terminal)(msg).await;
        }
        let mw = self.chain[self.idx].clone();
        let next = Next {
            chain: self.chain.clone(),
            session: self.session.clone(),
            idx: self.idx + 1,
            terminal: self.terminal.clone(),
        };
        mw.handle(&self.session, msg, next).await
    }
}

/// 中间件：数据面的消息拦截与转换（洋葱链）
///
/// 返回 `Ok(None)` 表示拦截该消息（不再继续分发）；
/// 返回 `Ok(Some(msg))` 可修改消息内容后继续（或直接返回响应帧）；
/// 返回 `Err` 将终止链并向上传播（RPC 回错误响应、事件丢弃并记录）。
#[async_trait]
pub trait Middleware: Send + Sync + 'static {
    /// 中间件名称
    fn name(&self) -> &str;

    /// 处理消息（调用 `next.run` 继续链；不调用即拦截）
    async fn handle(&self, session: &Session, msg: Message, next: Next) -> Result<Option<Message>>;

    /// 连接建立钩子（会话建立时调用；默认为空实现）
    async fn on_connect(&self, _session: &Session) -> Result<()> {
        Ok(())
    }

    /// 连接断开钩子（会话结束时调用；默认为空实现）
    async fn on_disconnect(&self, _session: &Session) -> Result<()> {
        Ok(())
    }
}
