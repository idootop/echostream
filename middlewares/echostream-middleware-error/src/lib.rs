//! EchoStream 错误处理中间件
//!
//! 数据面扩展：捕获下游（处理器/其他中间件）错误，统一转换为错误响应：
//! RPC 请求回结构化错误响应（错误码 + 消息），事件/流记录日志并继续传播。

use async_trait::async_trait;
use bytes::Bytes;
use echostream_core::{Middleware, Next, Session};
use echostream_proto::{Error, Message, ResponseMsg, Result, StatusCode};

/// 错误处理中间件：统一错误归一化
#[derive(Debug, Clone)]
pub struct ErrorMiddleware {
    name: String,
    /// 归一化错误码（默认 ERROR）
    code: StatusCode,
    /// 是否隐藏内部错误细节（默认 false：透传错误消息）
    hide_internal: bool,
}

impl ErrorMiddleware {
    /// 创建错误处理中间件
    pub fn new() -> Self {
        Self {
            name: "error".to_string(),
            code: StatusCode::ERROR,
            hide_internal: false,
        }
    }

    /// 自定义归一化错误码
    pub fn with_code(mut self, code: u16) -> Self {
        self.code = StatusCode::new(code);
        self
    }

    /// 隐藏内部错误细节（响应只回通用消息，避免泄露内部实现）
    pub fn hide_internal(mut self) -> Self {
        self.hide_internal = true;
        self
    }

    /// 自定义中间件名称
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

impl Default for ErrorMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for ErrorMiddleware {
    fn name(&self) -> &str {
        &self.name
    }

    async fn handle(
        &self,
        _session: &Session,
        msg: Message,
        next: Next,
    ) -> Result<Option<Message>> {
        match next.run(msg.clone()).await {
            Ok(result) => Ok(result),
            Err(e) => {
                // 仅 RPC 需要回响应；事件/流由上层记录
                if let Message::Request(req) = msg {
                    // 业务错误码透传；内部错误按配置归一化
                    let (code, message) = match &e {
                        Error::Rpc(code, msg) => (StatusCode::new(*code), msg.clone()),
                        other => (
                            self.code,
                            if self.hide_internal {
                                "internal error".to_string()
                            } else {
                                other.to_string()
                            },
                        ),
                    };
                    tracing::error!(middleware = self.name, code = code.0, error = %e, "处理出错，归一化错误响应");
                    Ok(Some(Message::Response(ResponseMsg {
                        id: req.id,
                        code,
                        message: Some(message),
                        data: Bytes::new(),
                    })))
                } else {
                    Err(e)
                }
            }
        }
    }
}
