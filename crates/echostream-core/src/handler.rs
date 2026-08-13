//! 处理器抽象：强类型接口 + 框架层统一编解码
//!
//! 业务处理器只面对具体类型（`Self::Req` / `Self::Resp` / `Self::Data`），
//! 编解码由框架通过 `*_encoded` 入口统一完成，避免重复样板代码。

use async_trait::async_trait;
use bytes::Bytes;
use echostream_proto::Result;
use serde::{Serialize, de::DeserializeOwned};

use crate::{codec, session::Session, stream::StreamReceiver};

// ======================== RPC ========================

/// RPC 处理器（强类型接口）
#[async_trait]
pub trait RpcHandler: Send + Sync + 'static {
    /// 请求类型
    type Req: DeserializeOwned + Send;
    /// 响应类型
    type Resp: Serialize + Send;

    /// 方法名
    fn name(&self) -> &str;

    /// 业务处理
    async fn handle(&self, session: &Session, req: Self::Req) -> Result<Self::Resp>;
}

/// RPC 处理器（对象安全，Router 分派入口）
#[async_trait]
pub trait DynRpcHandler: Send + Sync + 'static {
    fn name(&self) -> &str;
    async fn handle_encoded(&self, session: &Session, payload: Bytes) -> Result<Bytes>;
}

#[async_trait]
impl<T: RpcHandler> DynRpcHandler for T {
    fn name(&self) -> &str {
        T::name(self)
    }

    async fn handle_encoded(&self, session: &Session, payload: Bytes) -> Result<Bytes> {
        let req: T::Req = codec::decode(&payload)?;
        let resp = self.handle(session, req).await?;
        codec::encode(&resp)
    }
}

// ======================== Event ========================

/// 事件处理器（强类型接口）
#[async_trait]
pub trait EventHandler: Send + Sync + 'static {
    /// 事件数据类型
    type Data: DeserializeOwned + Send;

    /// 事件名
    fn name(&self) -> &str;

    /// 业务处理
    async fn handle(&self, session: &Session, data: Self::Data) -> Result<()>;
}

/// 事件处理器（对象安全，Router 分派入口）
#[async_trait]
pub trait DynEventHandler: Send + Sync + 'static {
    fn name(&self) -> &str;
    async fn handle_encoded(&self, session: &Session, payload: Bytes) -> Result<()>;
}

#[async_trait]
impl<T: EventHandler> DynEventHandler for T {
    fn name(&self) -> &str {
        T::name(self)
    }

    async fn handle_encoded(&self, session: &Session, payload: Bytes) -> Result<()> {
        let data: T::Data = codec::decode(&payload)?;
        self.handle(session, data).await
    }
}

// ======================== Stream ========================

/// 流处理器
#[async_trait]
pub trait StreamHandler: Send + Sync + 'static {
    /// 流名
    fn name(&self) -> &str;

    /// 业务处理（持续读取流帧直到结束）
    async fn handle(&self, session: &Session, stream: StreamReceiver) -> Result<()>;
}
