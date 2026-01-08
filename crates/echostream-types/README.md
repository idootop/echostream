# echostream-types

公共类型、错误定义和工具函数。

## 功能列表

- **错误类型**: 统一的错误类型定义和转换
- **上下文类型**: ServerContext、ClientContext、Session 定义
- **时间戳类型**: 时间戳相关类型和工具函数
- **公共类型**: 框架内部和用户使用的公共类型

## 子模块划分

- `error.rs`: 统一错误类型
- `context.rs`: 请求上下文（ServerContext、ClientContext）
- `session.rs`: Session 会话定义
- `timestamp.rs`: 时间戳相关类型

## 技术栈

- `serde`: 序列化支持
- `thiserror`: 错误派生宏
- `bytes`: 零拷贝字节操作

## 核心 API 设计

### 错误类型

```rust
use echostream_types::{Result, Error};

// 统一的错误类型
pub enum Error {
    ConnectionError(String),
    ProtocolError(String),
    SerializationError(String),
    TimeoutError,
    Custom(String),
}

// Result 类型别名
pub type Result<T> = std::result::Result<T, Error>;

// 使用示例
fn do_something() -> Result<()> {
    if error_condition {
        return Err(Error::Custom("错误信息".into()));
    }
    Ok(())
}
```

### Context 类型

```rust
use echostream_types::{ServerContext, ClientContext, Session};

// ServerContext: 服务端全局上下文
let ctx = ServerContext::new();
ctx.set("key", value);
let value = ctx.get::<ValueType>("key")?;

// ClientContext: 客户端全局上下文
let ctx = ClientContext::new();
ctx.set("key", value);
let value = ctx.get::<ValueType>("key")?;

// Session: 单个客户端会话
let session = Session::new(peer_addr);
session.set("user_id", 123u64);
let user_id = session.get::<u64>("user_id")?;

// 从 Session 访问 ServerContext
let global_data = session.server_ctx().get::<Data>("global")?;
```

### 时间戳类型

```rust
use echostream_types::{Timestamp, Duration};

// 创建时间戳
let ts = Timestamp::now();
let ts = Timestamp::from_micros(12345678);

// 时间戳运算
let later = ts + Duration::from_secs(10);
let diff = later - ts;

// 时间戳比较
if ts1 < ts2 {
    println!("ts1 更早");
}

// 格式化
println!("时间戳: {}", ts);
```

### 公共类型

```rust
use echostream_types::{StreamId, ConnectionId, FrameType};

// 流 ID
let stream_id = StreamId::new();
println!("流 ID: {}", stream_id);

// 连接 ID
let conn_id = ConnectionId::from_bytes(&bytes);
println!("连接 ID: {}", conn_id);

// 帧类型
match frame_type {
    FrameType::Request => { /* 请求帧 */ }
    FrameType::Response => { /* 响应帧 */ }
    FrameType::Event => { /* 事件帧 */ }
    FrameType::Stream => { /* 流数据帧 */ }
}
```
