//! EchoStream RPC 重试插件
//!
//! 请求失败自动重试（指数退避），适合网络抖动 / 服务瞬时错误场景。
//!
//! ```rust
//! use echostream_plugin_retry::{RetryPolicy, request_with_retry};
//!
//! let policy = RetryPolicy::default();
//! let sum: i64 = request_with_retry(&client, "add", &(1, 2), &policy).await?;
//! ```

use std::time::Duration;

use echostream_core::Client;
use echostream_proto::{Error, Result};

/// 重试策略
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// 最大尝试次数（含首次，默认 3）
    pub max_attempts: u32,
    /// 基础重试间隔（指数退避：base * 2^n，默认 200ms）
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(200),
        }
    }
}

impl RetryPolicy {
    /// 创建自定义策略
    pub fn new(max_attempts: u32, base_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
        }
    }
}

/// 判断错误是否可重试（超时与连接错误可重试，业务错误不重试）
pub fn is_retryable(err: &Error) -> bool {
    matches!(err, Error::Timeout(_) | Error::SessionClosed | Error::Io(_))
}

/// 带重试的 RPC 调用
pub async fn request_with_retry<Req, Resp>(
    client: &Client,
    name: &str,
    req: &Req,
    policy: &RetryPolicy,
) -> Result<Resp>
where
    Req: serde::Serialize + Send,
    Resp: serde::de::DeserializeOwned + Send,
{
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match client.request(name, req).await {
            Ok(resp) => return Ok(resp),
            Err(e) if attempt < policy.max_attempts && is_retryable(&e) => {
                let delay = policy.base_delay * (1 << (attempt - 1).min(6));
                tracing::warn!(name, attempt, error = %e, "RPC 失败，准备重试");
                tokio::time::sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}
