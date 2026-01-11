use crate::EchoResult;
use async_trait::async_trait;

/// 动态类型的 HashMap Container 实现接口
#[async_trait]
pub trait DynamicMap: Send + Sync + 'static {
    // 写入操作通常需要所有权
    async fn set<T: Send + Sync + 'static>(&self, key: String, value: T);

    // 查询操作建议统一使用 &str，因为查询不应消耗 Key 的所有权
    async fn get<T: Send + Sync + 'static>(&self, key: &str) -> Option<T>;

    async fn remove(&self, key: &str);

    async fn clear(&self);
}
