//! EchoStream 协议层
//!
//! 定义通信协议的核心类型和线缆格式（Wire Format）。
//! 本模块零运行时依赖，不涉及任何网络 I/O、异步运行时或具体序列化实现。

pub mod dynamic;
pub mod endpoint;
pub mod error;
pub mod message;

pub use dynamic::{Dynamic, Schema, decode, encode};
pub use endpoint::{
    Endpoint, FrameIo, FrameRead, FrameWrite, Listener, MAX_FRAME_SIZE, encode_message,
    read_message_frame,
};
pub use error::{Error, Result};
pub use message::{
    EventMsg, Message, RPC_CHANNEL_NAME, RequestMsg, ResponseMsg, StatusCode, StreamEndMsg,
    StreamMetaEntry, StreamMsg, StreamOpenMsg, Timestamp,
};
