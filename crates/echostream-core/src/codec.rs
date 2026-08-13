//! 载荷编解码：统一使用 postcard（零拷贝、紧凑）

use bytes::Bytes;
use echostream_proto::{Error, Result};
use serde::{Serialize, de::DeserializeOwned};

/// 序列化为载荷字节
pub fn encode<T: Serialize>(value: &T) -> Result<Bytes> {
    postcard::to_allocvec(value)
        .map(Bytes::from)
        .map_err(|e| Error::Serialization(e.to_string()))
}

/// 反序列化载荷字节
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    postcard::from_bytes(bytes).map_err(|e| Error::Serialization(e.to_string()))
}
