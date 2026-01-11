use thiserror::Error;

/// 核心抽象错误类型
#[derive(Error, Debug, Clone)]
pub enum EchoError {
    /// IO相关错误（抽象）
    #[error("IO error: {0}")]
    Io(String),

    /// 序列化/反序列化错误
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// 协议错误（如无效帧、错误的序列号等）
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// RPC超时
    #[error("RPC request {0} timed out after {1}ms")]
    Timeout(RequestId, u64),

    /// RPC响应错误
    #[error("RPC error (code {0}): {1}")]
    RpcError(u16, String),

    /// 处理器未找到
    #[error("Handler not found: {0}")]
    HandlerNotFound(HandlerName),

    /// 流错误
    #[error("Stream error (id: {0}): {1}")]
    StreamError(StreamId, String),

    /// 会话错误（如未连接）
    #[error("Session error: {0}")]
    SessionError(String),

    /// 上下文错误（如状态获取失败）
    #[error("Context error: {0}")]
    ContextError(String),

    /// 中间件错误
    #[error("Middleware error: {0}")]
    MiddlewareError(String),

    /// 插件错误
    #[error("Plugin error: {0}")]
    PluginError(String),

    /// 无效参数
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// 不支持的操作
    #[error("Unsupported operation: {0}")]
    Unsupported(String),
}

/// 核心结果类型
pub type EchoResult<T> = Result<T, EchoError>;
