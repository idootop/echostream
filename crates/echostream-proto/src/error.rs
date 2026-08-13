use thiserror::Error;

/// EchoStream 统一错误类型
#[derive(Error, Debug, Clone)]
pub enum Error {
    /// IO 错误
    #[error("IO error: {0}")]
    Io(String),

    /// 序列化/反序列化错误
    #[error("serialization error: {0}")]
    Serialization(String),

    /// 协议错误（无效帧、格式错误等）
    #[error("protocol error: {0}")]
    Protocol(String),

    /// RPC 请求超时
    #[error("request {0} timed out")]
    Timeout(u64),

    /// RPC 响应错误（对端返回的业务错误）
    #[error("rpc error (code {0}): {1}")]
    Rpc(u16, String),

    /// 处理器未找到
    #[error("handler not found: {0}")]
    HandlerNotFound(String),

    /// 会话已关闭或不可用
    #[error("session closed")]
    SessionClosed,

    /// 参数错误
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

/// 统一结果类型
pub type Result<T> = std::result::Result<T, Error>;
