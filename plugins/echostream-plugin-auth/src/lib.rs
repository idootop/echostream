//! EchoStream 认证插件
//!
//! 连接认证：客户端连接后需先发送认证事件（约定名 `__echostream.auth`，载荷为
//! token 字符串的 postcard 编码），认证通过前所有其他消息被中间件拦截。
//!
//! ```rust
//! use echostream_plugin_auth::AuthPlugin;
//!
//! ServerBuilder::new()
//!     .plugin(AuthPlugin::new("my-secret-token"))
//!     .build().await?;
//! ```

use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use echostream_core::{Middleware, Next, ServerBuilder, ServerPlugin, Session};
use echostream_proto::{Error, Message, Result};

/// 认证事件名（客户端连接后发送）
pub const AUTH_EVENT: &str = "__echostream.auth";

/// 会话认证状态键
const AUTH_KEY: &str = "__echostream_authed";

/// 未认证消息等待认证的窗口（认证事件通常紧随连接建立）
const AUTH_WAIT: Duration = Duration::from_secs(1);

/// 认证插件
pub struct AuthPlugin {
    token: String,
}

impl AuthPlugin {
    /// 创建认证插件
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

impl ServerPlugin for AuthPlugin {
    fn name(&self) -> &str {
        "auth"
    }

    fn install(self: Box<Self>, builder: ServerBuilder) -> ServerBuilder {
        let token = self.token.clone();
        builder
            .add_event(AuthEventHandler {
                token: self.token.clone(),
            })
            .middleware(AuthMiddleware { token })
            .on_connect(|_: &Session| {})
    }
}

/// 认证事件处理器：校验 token 并标记会话已认证
struct AuthEventHandler {
    token: String,
}

#[async_trait::async_trait]
impl echostream_core::EventHandler for AuthEventHandler {
    type Data = String;

    fn name(&self) -> &str {
        AUTH_EVENT
    }

    async fn handle(&self, session: &Session, data: String) -> Result<()> {
        if data == self.token {
            session.set(AUTH_KEY, true);
            tracing::info!(session = session.id(), "认证成功");
            Ok(())
        } else {
            Err(Error::Rpc(4, "认证失败：token 无效".to_string()))
        }
    }
}

/// 认证中间件：未认证会话只放行认证事件
struct AuthMiddleware {
    token: String,
}

#[async_trait]
impl Middleware for AuthMiddleware {
    fn name(&self) -> &str {
        "auth"
    }

    async fn handle(&self, session: &Session, msg: Message, next: Next) -> Result<Option<Message>> {
        // 认证事件本身放行（token 校验在 handler）
        if matches!(&msg, Message::Event(e) if e.name == AUTH_EVENT) {
            return next.run(msg).await;
        }
        // 已认证会话放行
        if session.get::<bool>(AUTH_KEY).is_some() {
            return next.run(msg).await;
        }
        // 等待认证完成（认证事件与业务请求并发到达时的竞态防护：
        // 事件与 RPC 在不同任务处理，轮询状态保证认证先于业务生效）
        for _ in 0..10 {
            if session.get::<bool>(AUTH_KEY).is_some() {
                return next.run(msg).await;
            }
            tokio::time::sleep(AUTH_WAIT / 10).await;
        }
        // 未认证：拦截
        tracing::warn!(session = session.id(), "拦截未认证消息");
        let _ = &self.token;
        Ok(None)
    }
}

/// 客户端辅助：发送认证事件
pub async fn authenticate(session: &Session, token: impl Into<String>) -> Result<()> {
    let token = token.into();
    session
        .emit_raw(
            AUTH_EVENT,
            Bytes::from(
                postcard::to_allocvec(&token).map_err(|e| Error::Serialization(e.to_string()))?,
            ),
        )
        .await
}
