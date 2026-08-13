use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// 消息帧 —— 传输的基本单位
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Message {
    /// RPC 请求
    Request(RequestMsg),
    /// RPC 响应
    Response(ResponseMsg),
    /// 单向事件
    Event(EventMsg),
    /// 流数据
    Stream(StreamMsg),
}

/// RPC 请求载荷
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RequestMsg {
    /// 请求 ID（用于匹配响应）
    pub id: u64,
    /// 处理器名称
    pub name: String,
    /// 请求数据（序列化后的载荷）
    pub data: Bytes,
}

/// RPC 响应载荷
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponseMsg {
    /// 对应的请求 ID
    pub id: u64,
    /// 状态码
    pub code: StatusCode,
    /// 错误信息（失败时）
    pub message: Option<String>,
    /// 响应数据（成功时）
    pub data: Bytes,
}

/// 事件载荷
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventMsg {
    /// 事件 ID（用于去重和排序）
    pub id: u64,
    /// 事件名称
    pub name: String,
    /// 事件数据
    pub data: Bytes,
}

/// 流数据载荷
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamMsg {
    /// 所属流的 ID
    pub id: u64,
    /// 流名称
    pub name: String,
    /// 帧序列号（单调递增，用于丢包检测和排序）
    pub seq: u64,
    /// 发送方时间戳（毫秒，用于时间对齐）
    pub sender_ts: Timestamp,
    /// 流数据
    pub data: Bytes,
}

/// 毫秒级时间戳（用于时间同步）
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// 从系统时间创建
    pub fn now() -> Self {
        Self(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or_default(),
        )
    }

    /// 转换为毫秒数
    pub fn as_millis(&self) -> u64 {
        self.0
    }
}

/// 状态码
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StatusCode(pub u16);

impl StatusCode {
    /// 成功（默认）
    pub const SUCCESS: Self = Self(0);
    /// 通用错误
    pub const ERROR: Self = Self(1);
    /// 超时
    pub const TIMEOUT: Self = Self(2);
    /// 处理器未找到
    pub const NOT_FOUND: Self = Self(3);
    /// 权限不足
    pub const FORBIDDEN: Self = Self(4);
    /// 参数错误
    pub const INVALID_PARAM: Self = Self(5);

    /// 快速创建自定义状态码
    pub const fn new(code: u16) -> Self {
        Self(code)
    }

    /// 是否为成功状态
    pub const fn is_success(&self) -> bool {
        self.0 == 0
    }
}
