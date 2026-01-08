// 重导出核心类型
pub use echostream_core::{RpcServer, RpcClient, ServerContext, ClientContext, Session};
pub use echostream_derive::{handler, listener, stream_handler};
pub use echostream_types::{Result, Error};

// 可选的服务发现
#[cfg(feature = "discovery")]
pub use echostream_discovery::{ServiceDiscovery, ServiceInfo};

// Prelude 模块
pub mod prelude {
    pub use super::*;
}