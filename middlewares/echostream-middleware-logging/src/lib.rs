//! EchoStream 日志中间件
//!
//! 数据面扩展：结构化记录每条入站消息（类型/名称）与中间件链处理耗时。

use async_trait::async_trait;
use echostream_core::{Middleware, Next, Session};
use echostream_proto::{Message, Result};

/// 日志中间件：tracing 记录消息与处理耗时
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

    async fn handle(&self, session: &Session, msg: Message, next: Next) -> Result<Option<Message>> {
        let start = std::time::Instant::now();
        let (kind, name) = match &msg {
            Message::Request(r) => ("rpc", r.name.clone()),
            Message::Event(e) => ("event", e.name.clone()),
            Message::Stream(s) => ("stream", s.name.clone()),
            Message::Response(r) => ("response", format!("id={}", r.id)),
            Message::StreamEnd(e) => ("stream_end", format!("id={}", e.id)),
        };
        tracing::info!(session = session.id(), kind, name = %name, "收到消息");
        // 包裹下游：next 完成后记录整链耗时
        let result = next.run(msg).await;
        let elapsed = start.elapsed();
        match &result {
            Ok(Some(Message::Response(r))) => {
                tracing::info!(
                    session = session.id(),
                    kind,
                    name = %name,
                    code = r.code.0,
                    elapsed_us = elapsed.as_micros() as u64,
                    "处理完成"
                );
            }
            Ok(None) => tracing::info!(
                session = session.id(),
                kind,
                name = %name,
                elapsed_us = elapsed.as_micros() as u64,
                "已拦截"
            ),
            Err(e) => tracing::warn!(
                session = session.id(),
                kind,
                name = %name,
                elapsed_us = elapsed.as_micros() as u64,
                error = %e,
                "处理出错"
            ),
            _ => {}
        }
        result
    }
}
