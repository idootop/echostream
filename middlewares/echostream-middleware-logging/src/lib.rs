//! EchoStream 日志中间件
//!
//! 数据面扩展：结构化记录每条入站消息（类型/名称/耗时）。

use async_trait::async_trait;
use echostream_core::{Middleware, Session};
use echostream_proto::{Message, Result};

/// 日志中间件：tracing 记录消息处理耗时
#[derive(Debug, Clone, Default)]
pub struct LoggingMiddleware;

impl LoggingMiddleware {
    /// 创建日志中间件
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Middleware for LoggingMiddleware {
    fn name(&self) -> &str {
        "logging"
    }

    async fn on_message(&self, session: &Session, msg: Message) -> Result<Option<Message>> {
        let start = std::time::Instant::now();
        let (kind, name) = match &msg {
            Message::Request(r) => ("rpc", r.name.clone()),
            Message::Event(e) => ("event", e.name.clone()),
            Message::Stream(s) => ("stream", s.name.clone()),
            Message::Response(r) => ("response", format!("id={}", r.id)),
            Message::StreamEnd(e) => ("stream_end", format!("id={}", e.id)),
        };
        tracing::info!(
            session = session.id(),
            kind,
            name = %name,
            "收到消息"
        );
        let _ = start;
        Ok(Some(msg))
    }
}
