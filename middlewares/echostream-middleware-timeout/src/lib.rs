//! EchoStream 超时中间件
//!
//! 数据面扩展：为整条中间件链（含业务处理器）设置超时上限，
//! 超时后 RPC 返回 TIMEOUT 错误响应、事件/流被终止并记录日志。

use std::time::Duration;

use async_trait::async_trait;
use echostream_core::{Middleware, Next, Session};
use echostream_proto::{Error, Message, Result};

/// 超时中间件：包裹下游处理链（含处理器执行）
#[derive(Debug, Clone)]
pub struct TimeoutMiddleware {
    name: String,
    timeout: Duration,
}

impl TimeoutMiddleware {
    /// 创建超时中间件（超时后 RPC 回 TIMEOUT、事件/流终止）
    pub fn new(timeout: Duration) -> Self {
        Self {
            name: "timeout".to_string(),
            timeout,
        }
    }

    /// 自定义中间件名称（多个超时中间件场景）
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

impl Default for TimeoutMiddleware {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

#[async_trait]
impl Middleware for TimeoutMiddleware {
    fn name(&self) -> &str {
        &self.name
    }

    async fn handle(
        &self,
        _session: &Session,
        msg: Message,
        next: Next,
    ) -> Result<Option<Message>> {
        match tokio::time::timeout(self.timeout, next.run(msg.clone())).await {
            Ok(result) => result,
            Err(_) => {
                // 超时：RPC 回 TIMEOUT 错误（dispatch 映射错误码），事件/流传播错误
                let id = match &msg {
                    Message::Request(r) => r.id,
                    Message::Event(e) => e.id,
                    Message::Stream(s) => s.id,
                    Message::Response(r) => r.id,
                    Message::StreamEnd(e) => e.id,
                };
                tracing::warn!(
                    middleware = self.name,
                    id,
                    timeout_ms = self.timeout.as_millis() as u64,
                    "处理超时"
                );
                Err(Error::Timeout(id))
            }
        }
    }
}
