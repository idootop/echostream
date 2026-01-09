// 重导出核心类型
pub use echostream_core::{ClientContext, EchoClient, EchoServer, ServerContext, Session};
pub use echostream_derive::{rpc, event, stream};
pub use echostream_types::{Error, Result};

// Prelude 模块
pub mod prelude {
    pub use super::*;
}
