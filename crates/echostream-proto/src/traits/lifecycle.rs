use async_trait::async_trait;

use crate::error::EchoResult;

/// 通用生命周期管理
#[async_trait]
pub trait Lifecycle: Send + Sync + 'static {
    /// 初始化
    async fn init(&self) -> EchoResult<()> {
        Ok(())
    }

    /// 清理资源
    async fn cleanup(&self) -> EchoResult<()> {
        Ok(())
    }
}
