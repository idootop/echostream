# echostream-proto

> 底层协议定义、Wire Format 和基础类型

## 模块职责

`echostream-proto` 是整个框架最底层的模块,定义了通信协议的核心类型和数据结构:

- **协议格式**: 定义消息的 Wire Format (线缆格式)
- **基础类型**: Session、Header、错误码等核心数据结构
- **零依赖**: 仅依赖 `serde`、`bytes`、`thiserror` 等基础库,不引入任何异步运行时

## 设计原则

### 最小化依赖

该模块是所有其他模块的共同依赖,因此必须保持极简:
- 不依赖异步运行时 (tokio/async-std)
- 不依赖网络层实现 (quinn)
- 不依赖具体序列化方案 (仅定义 trait,不强制 postcard)

### 稳定 API

由于是底层协议层,API 应保持高度稳定:
- 修改此模块会导致所有上层模块重新编译
- 插件、中间件、derive 宏都依赖这里的类型定义
- 版本升级时需特别谨慎,确保向后兼容

## 核心类型

### Wire Format

```rust
// 消息帧格式
pub struct Frame {
    pub header: Header,
    pub payload: Bytes,
}

pub struct Header {
    pub message_type: MessageType,
    pub request_id: u64,
    pub flags: u8,
}

pub enum MessageType {
    Request,
    Response,
    Event,
    Stream,
}
```

### Session 与上下文

```rust
// 连接会话信息
pub struct Session {
    pub id: SessionId,
    pub peer_addr: SocketAddr,
    pub created_at: SystemTime,
}

// 时间戳包装器 (用于流数据)
pub struct Timestamped<T> {
    pub wall_time: u64,  // 对齐后的绝对时间
    pub seq: u32,        // 序列号
    pub data: T,
}
```

### 错误定义

```rust
use echostream_proto::{Result, Error};

#[derive(Error, Debug)]
pub enum Error {
    #[error("序列化失败: {0}")]
    SerializationError(String),

    #[error("协议错误: {0}")]
    ProtocolError(String),

    #[error("超时")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, Error>;
```

## 使用场景

### 在插件中使用

```rust
use echostream_proto::{Session, Timestamped};

struct MetricsPlugin;

impl Plugin for MetricsPlugin {
    fn on_connect(&self, session: &Session) {
        println!("新连接: {} from {}", session.id, session.peer_addr);
    }
}
```

### 在中间件中使用

```rust
use echostream_proto::{Frame, Header, MessageType};

struct LoggingMiddleware;

impl Middleware for LoggingMiddleware {
    fn process(&self, frame: &Frame) -> Result<()> {
        match frame.header.message_type {
            MessageType::Request => log::info!("收到请求"),
            MessageType::Response => log::info!("收到响应"),
            _ => {}
        }
        Ok(())
    }
}
```

## 依赖关系

```
echostream-proto (本模块)
    ├── serde          - 序列化 trait
    ├── bytes          - 零拷贝字节操作
    └── thiserror      - 错误处理
```

被以下模块依赖:
- `echostream-transport` - 传输层封装
- `echostream-core` - 核心抽象层
- `echostream-derive` - 过程宏
- `echostream-discovery` - 服务发现

## 注意事项

- 该模块的类型定义要通用、稳定,避免频繁变更
- 新增类型时考虑是否真的属于"协议层",而非"业务层"
- 保持与具体实现解耦,只定义接口和数据结构
