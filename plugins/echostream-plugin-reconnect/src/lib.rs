//! EchoStream 自动重连插件
//!
//! 客户端断线后按指数退避自动重连，保留已注册的处理器与会话状态。
//!
//! ```rust
//! use echostream_plugin_reconnect::ReconnectPlugin;
//!
//! ClientBuilder::new()
//!     .plugin(ReconnectPlugin::new("127.0.0.1:5000"))
//!     .connect("127.0.0.1:5000").await?;
//! ```

use std::sync::Arc;
use std::time::Duration;

use echostream_core::{Client, ClientBuilder, ClientPlugin};

/// 重连插件：断线自动重连（QUIC）
pub struct ReconnectPlugin {
    addr: String,
    max_retries: u32,
    base_delay: Duration,
}

impl ReconnectPlugin {
    /// 创建重连插件（默认最多 10 次，基础间隔 1 秒指数退避）
    pub fn new(addr: impl Into<String>) -> Self {
        Self {
            addr: addr.into(),
            max_retries: 10,
            base_delay: Duration::from_secs(1),
        }
    }

    /// 设置最大重试次数
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    /// 设置基础重试间隔（指数退避：base * 2^n，上限 64 * base）
    pub fn base_delay(mut self, d: Duration) -> Self {
        self.base_delay = d;
        self
    }
}

impl ClientPlugin for ReconnectPlugin {
    fn name(&self) -> &str {
        "reconnect"
    }

    fn install(self: Box<Self>, builder: ClientBuilder) -> ClientBuilder {
        let addr = self.addr.clone();
        let max_retries = self.max_retries;
        let base_delay = self.base_delay;
        builder.on_disconnect(move |client: &Client| {
            let addr = addr.clone();
            let client = client.clone();
            tokio::spawn(async move {
                for attempt in 1..=max_retries {
                    if client.is_closed() {
                        return; // 主动关闭，放弃重连
                    }
                    tracing::info!(%addr, attempt, "重连尝试");
                    let socket: std::net::SocketAddr = match addr.parse() {
                        Ok(a) => a,
                        Err(_) => {
                            tracing::error!(%addr, "地址格式非法");
                            return;
                        }
                    };
                    match echostream_transport::connect(socket).await {
                        Ok(conn) => {
                            tracing::info!(%addr, attempt, "重连成功");
                            client.reconnect(Arc::new(conn));
                            return;
                        }
                        Err(e) => {
                            tracing::warn!(%addr, attempt, error = %e, "重连失败");
                            let delay = base_delay * (1 << attempt.min(6));
                            tokio::time::sleep(delay).await;
                        }
                    }
                }
                tracing::error!(%addr, "重连失败，已放弃");
            });
        })
    }
}
