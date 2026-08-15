use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// RPC 复用通道的保留方法名：通道开启标记帧（首帧 Request，业务侧不可占用）
pub const RPC_CHANNEL_NAME: &str = "$channel";

/// 消息帧 —— 传输的基本单位
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Message {
    /// RPC 请求
    Request(RequestMsg),
    /// RPC 响应
    Response(ResponseMsg),
    /// 单向事件
    Event(EventMsg),
    /// 流开始帧：流协商（名称 + 元数据；必须为流首帧）
    StreamOpen(StreamOpenMsg),
    /// 流数据帧
    Stream(StreamMsg),
    /// 流结束帧（结束码 + 原因 + 结束元数据；必须为流末帧）
    StreamEnd(StreamEndMsg),
}

/// 流元数据项（键值对；值可为任意字节，字符串值直接 UTF-8 编码）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamMetaEntry {
    /// 元数据键（如 samplerate / bitrate / width / filename / size / clock-rate）
    pub key: String,
    /// 元数据值
    pub value: Bytes,
}

impl StreamMetaEntry {
    /// 字符串值元数据
    pub fn str(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: Bytes::from(value.into().into_bytes()),
        }
    }

    /// 数值值元数据
    pub fn num(key: impl Into<String>, value: u64) -> Self {
        Self {
            key: key.into(),
            value: Bytes::from(value.to_string().into_bytes()),
        }
    }

    /// 布尔值元数据（存储为 "true" / "false"，跨端一致；`get_metadata_bool` 快速解析）
    pub fn bool(key: impl Into<String>, value: bool) -> Self {
        Self {
            key: key.into(),
            value: Bytes::from(
                if value { "true" } else { "false" }
                    .to_string()
                    .into_bytes(),
            ),
        }
    }

    /// 原始字节值元数据
    pub fn bytes(key: impl Into<String>, value: Bytes) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }
}

/// 流开始帧：流协商（必须为流首帧）
///
/// metadata 为可扩展键值对，承载上层语义（核心协议本身不理解这些字段，
/// 由上层插件 / 示例定义约定）。常用约定（业界通用命名）：
/// - 音视频：codec=opus/h264、samplerate=48000、channels=2、bitrate=128000、
///   width=1920、height=1080、fps=30
/// - 时间同步：clock-rate=48000、timescale=1000000（采样时钟协商，帧内采样
///   时间由上层在载荷中实现）
/// - 文件传输：filename、size、mime
/// - 自定义扩展：任意键值
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamOpenMsg {
    /// 所属流的 ID
    pub id: u64,
    /// 流名称（数据帧不再携带，按 id 路由，省每帧字符串开销）
    pub name: String,
    /// 流元数据（音视频参数 / 文件信息 / 自定义扩展）
    pub metadata: Vec<StreamMetaEntry>,
}

/// 流数据帧（名称仅在 StreamOpen 携带，数据帧按 id 路由）
///
/// 核心协议只承载传输语义（有序可靠帧 + 序列号 + 墙钟时间戳）；
/// 上层语义（采样时钟 / 关键帧标记 / 文件分块等）通过 StreamOpen 的
/// 可扩展 metadata 协商，并由上层插件或示例在载荷内实现。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamMsg {
    /// 所属流的 ID
    pub id: u64,
    /// 帧序列号（单调递增，用于丢包检测和排序）
    pub seq: u64,
    /// 发送方墙钟时间戳（毫秒，用于时间对齐与延迟测量）
    pub sender_ts: Timestamp,
    /// 流数据
    pub data: Bytes,
}

/// 流结束帧（必须为流末帧；QUIC 上为流关闭前的最后一帧）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamEndMsg {
    /// 所属流的 ID
    pub id: u64,
    /// 结束码（0 = 正常结束；非 0 = 异常 / 取消 / 业务终止）
    pub code: u16,
    /// 结束原因（可选）
    pub message: Option<String>,
    /// 结束元数据（trailers 风格：统计信息、校验和、对端确认等）
    pub metadata: Vec<StreamMetaEntry>,
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
