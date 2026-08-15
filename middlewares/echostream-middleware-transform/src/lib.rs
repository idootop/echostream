//! EchoStream 数据转换中间件
//!
//! 数据面扩展：在中间件链前后对消息载荷做转换（压缩 / 加密 / 格式包装等），
//! 请求方向在进入处理器前应用 `on_request`，响应方向在返回前应用 `on_response`。

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use echostream_core::{Middleware, Next, Session};
use echostream_proto::{Message, Result};

/// 载荷转换函数：`Bytes -> Result<Bytes>`
pub type TransformFn = dyn Fn(Bytes) -> Result<Bytes> + Send + Sync;

/// 数据转换中间件：请求 / 响应载荷转换
pub struct TransformMiddleware {
    name: String,
    on_request: Option<Arc<TransformFn>>,
    on_response: Option<Arc<TransformFn>>,
}

impl TransformMiddleware {
    /// 创建转换中间件（至少配置一个转换器）
    pub fn new() -> Self {
        Self {
            name: "transform".to_string(),
            on_request: None,
            on_response: None,
        }
    }

    /// 请求方向转换：进入处理器前应用（RPC 请求 / 事件 / 流帧载荷）
    pub fn map_request<F>(mut self, f: F) -> Self
    where
        F: Fn(Bytes) -> Result<Bytes> + Send + Sync + 'static,
    {
        self.on_request = Some(Arc::new(f));
        self
    }

    /// 响应方向转换：处理器返回后应用（RPC 响应载荷）
    pub fn map_response<F>(mut self, f: F) -> Self
    where
        F: Fn(Bytes) -> Result<Bytes> + Send + Sync + 'static,
    {
        self.on_response = Some(Arc::new(f));
        self
    }

    /// 自定义中间件名称
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

impl Default for TransformMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

/// 就地修改消息载荷（按消息类型分派）
fn map_payload(msg: Message, f: &TransformFn) -> Result<Message> {
    match msg {
        Message::Request(mut m) => {
            m.data = f(m.data)?;
            Ok(Message::Request(m))
        }
        Message::Event(mut m) => {
            m.data = f(m.data)?;
            Ok(Message::Event(m))
        }
        Message::Stream(mut m) => {
            m.data = f(m.data)?;
            Ok(Message::Stream(m))
        }
        Message::Response(mut m) => {
            m.data = f(m.data)?;
            Ok(Message::Response(m))
        }
        other => Ok(other),
    }
}

#[async_trait]
impl Middleware for TransformMiddleware {
    fn name(&self) -> &str {
        &self.name
    }

    async fn handle(
        &self,
        _session: &Session,
        msg: Message,
        next: Next,
    ) -> Result<Option<Message>> {
        // 请求方向：进入下游前转换
        let msg = match &self.on_request {
            Some(f) => map_payload(msg, f.as_ref())?,
            None => msg,
        };
        // 响应方向：下游返回后转换（仅响应帧）
        let result = next.run(msg).await?;
        match (result, &self.on_response) {
            (Some(Message::Response(mut r)), Some(f)) => {
                r.data = f(r.data)?;
                Ok(Some(Message::Response(r)))
            }
            (other, _) => Ok(other),
        }
    }
}
