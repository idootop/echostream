# EchoStream 模块职责清单

> 明确各子模块的边界、分工和依赖关系

## 一、架构分层

```
┌──────────────────────────────────────────┐
│         echostream (统一入口)              │
│  - 重导出所有公共 API                      │
│  - 提供 prelude 模块                      │
└────────────┬─────────────────────────────┘
             │
      ┌──────┴──────┬──────────────┐
      ▼             ▼              ▼
┌─────────┐  ┌──────────┐  ┌────────────┐
│  derive │  │   core   │  │ discovery  │
│  过程宏  │  │  核心层   │  │  服务发现   │
└─────────┘  └─────┬────┘  └─────┬──────┘
                   │              │
            ┌──────┴────┐         │
            ▼           ▼         │
      ┌──────────┐ ┌───────┐     │
      │transport │ │ proto │◄────┘
      │  传输层   │ │协议层 │
      └─────┬────┘ └───────┘
            │
            ▼
       (quinn/QUIC)
```

---

## 二、模块详细职责

### 2.1 echostream-proto (协议层)

**核心定位**: 最底层的协议定义,所有模块的共同基础

#### 职责范围

✅ **应该包含**:
- 消息帧格式 (`Frame`, `Header`, `MessageType`)
- 会话信息 (`SessionId`, `SessionInfo`)
- 时间戳类型 (`Timestamped<T>`)
- 统一错误类型 (`Error`, `Result<T>`)
- 序列化/反序列化 trait 定义
- 常量定义 (魔数、版本号、限制值)

❌ **不应该包含**:
- 任何网络 I/O 操作
- 异步运行时 (tokio/async-std)
- 具体的序列化实现 (postcard)
- 业务逻辑和状态管理

#### 依赖约束

**只能依赖**:
- `serde` - 序列化 trait
- `bytes` - 字节操作
- `thiserror` - 错误派生宏

**禁止依赖**:
- `tokio`、`async-std` 等异步运行时
- `quinn`、网络库
- `postcard`、具体序列化库
- 任何内部 crate

#### 稳定性要求

- API 必须高度稳定,避免频繁变更
- 新增类型需谨慎评估是否属于"协议层"
- 修改会导致所有依赖模块重新编译

#### 对外接口

```rust
// 核心类型
pub struct Frame { ... }
pub struct Header { ... }
pub enum MessageType { ... }
pub struct SessionId(pub u64);
pub struct SessionInfo { ... }
pub struct Timestamped<T> { ... }

// 错误处理
pub enum Error { ... }
pub type Result<T> = std::result::Result<T, Error>;

// Trait 定义 (可选)
pub trait Serializer { ... }
```

---

### 2.2 echostream-transport (传输层)

**核心定位**: 封装底层传输协议,提供统一的网络抽象

#### 职责范围

✅ **应该包含**:
- QUIC 连接管理 (封装 Quinn)
- TLS 握手和证书配置
- 0-RTT 快速重连
- Streams/Datagrams 多路复用
- 传输层抽象 trait (`Transport`, `Connection`)
- 网络错误处理和重试

❌ **不应该包含**:
- RPC 路由和调度
- 消息序列化/反序列化
- 业务逻辑和处理器
- 插件和中间件系统
- 时间同步算法

#### 依赖约束

**可以依赖**:
- `echostream-proto` - 基础类型
- `quinn` - QUIC 实现
- `tokio` - 异步运行时
- `bytes` - 零拷贝操作
- `tracing` - 日志追踪
- `thiserror` - 错误处理

**禁止依赖**:
- `echostream-core` 或更上层模块
- `postcard` 等序列化库 (应在上层处理)

#### 扩展点设计

- 使用 trait 抽象,支持未来替换 QUIC
- 预留 WebTransport 扩展接口
- 支持自定义拥塞控制算法

#### 对外接口

```rust
// 传输层抽象
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    type Connection: Connection;
    async fn listen(&self, addr: SocketAddr) -> Result<Listener<Self::Connection>>;
    async fn connect(&self, addr: SocketAddr) -> Result<Self::Connection>;
}

#[async_trait]
pub trait Connection: Send + Sync + 'static {
    async fn open_bi_stream(&self) -> Result<(SendStream, RecvStream)>;
    async fn accept_bi_stream(&self) -> Result<(SendStream, RecvStream)>;
    async fn send_datagram(&self, data: Bytes) -> Result<()>;
    async fn recv_datagram(&self) -> Result<Bytes>;
    fn peer_addr(&self) -> SocketAddr;
    async fn close(&self) -> Result<()>;
}

// QUIC 实现
pub struct QuicTransport { ... }
pub struct QuicConfig { ... }
pub struct QuicConfigBuilder { ... }
```

---

### 2.3 echostream-core (核心框架)

**核心定位**: 实现 RPC 框架的核心逻辑和扩展机制

#### 职责范围

✅ **应该包含**:
- RPC 调度器 (方法路由、请求匹配)
- 事件分发系统
- 流管理和抖动缓冲
- 时间同步算法 (类 NTP)
- 插件系统 (生命周期 hook)
- 中间件系统 (Tower-like)
- 服务端/客户端实现 (`EchoServer`, `EchoClient`)
- 上下文管理 (`ServerContext`, `ClientContext`, `Session`)
- 处理器 trait (`RpcHandler`, `EventHandler`, `StreamHandler`)

❌ **不应该包含**:
- 具体的传输协议实现
- 过程宏实现
- 服务发现逻辑
- 业务逻辑代码

#### 依赖约束

**可以依赖**:
- `echostream-proto` - 基础类型
- `echostream-transport` - 传输抽象
- `tokio` - 异步运行时
- `postcard` - 序列化实现
- `serde` - 序列化 trait
- `bytes` - 字节操作
- `tracing` - 日志追踪
- `anyhow` - 错误处理

**禁止依赖**:
- `echostream-derive` (过程宏)
- `echostream-discovery` (服务发现)
- `quinn` (应通过 transport 抽象)

#### 核心模块拆分

建议内部子模块:
- `connection/` - 连接生命周期管理
- `rpc/` - RPC 调度器和路由
- `event/` - 事件分发
- `stream/` - 流管理和缓冲
- `sync/` - 时间同步
- `plugin/` - 插件系统
- `middleware/` - 中间件系统
- `server/` - 服务端实现
- `client/` - 客户端实现
- `context/` - 上下文管理

#### 对外接口

```rust
// 服务端/客户端
pub struct EchoServer { ... }
pub struct EchoClient { ... }
pub struct ServerBuilder { ... }
pub struct ClientBuilder { ... }

// 上下文
pub struct ServerContext { ... }
pub struct ClientContext { ... }
pub struct Session { ... }

// 处理器 trait
pub trait RpcHandler { ... }
pub trait EventHandler { ... }
pub trait StreamHandler { ... }

// 插件系统
pub trait ServerPlugin { ... }
pub trait ClientPlugin { ... }

// 中间件系统
pub trait Middleware { ... }

// 流操作
pub struct StreamReceiver { ... }
pub struct StreamSender { ... }
```

---

### 2.4 echostream-derive (过程宏)

**核心定位**: 提供声明式 API,减少样板代码

#### 职责范围

✅ **应该包含**:
- `#[rpc]` 宏 - 生成 `RpcHandler` 实现
- `#[event]` 宏 - 生成 `EventHandler` 实现
- `#[stream]` 宏 - 生成 `StreamHandler` 实现
- 函数签名解析和验证
- 参数提取策略 (Session、Payload)
- 零成本抽象 (ZST 结构体)

❌ **不应该包含**:
- 运行时逻辑
- 网络通信
- 状态管理

#### 依赖约束

**可以依赖**:
- `syn` - AST 解析
- `quote` - 代码生成
- `proc-macro2` - 宏基础设施

**禁止依赖**:
- 任何运行时库 (tokio、quinn)
- 内部 crate (core、proto、transport)

**注意**: 过程宏是编译期工具,不能依赖运行时类型

#### 宏展开示例

```rust
// 输入
#[rpc("user.login")]
async fn login(session: Session, req: LoginReq) -> Result<LoginResp> { ... }

// 展开后
pub struct login;  // ZST

impl RpcHandler for login {
    fn name(&self) -> &'static str { "user.login" }

    fn handle(&self, session: Session, payload: Bytes) -> BoxFuture<'static, Result<Bytes>> {
        Box::pin(async move {
            let req: LoginReq = postcard::from_bytes(&payload)?;
            let result = super::login(session, req).await?;
            Ok(Bytes::from(postcard::to_allocvec(&result)?))
        })
    }
}
```

#### 参数提取模式

| 签名模式 | 提取策略 |
|---------|---------|
| `fn(Session, Req)` | 注入 Session + 反序列化 Payload |
| `fn(Session)` | 仅注入 Session,忽略 Payload |
| `fn(Req)` | 仅反序列化 Payload |
| `fn()` | 无参数,纯逻辑触发 |

---

### 2.5 echostream-discovery (服务发现)

**核心定位**: 提供局域网零配置服务发现

#### 职责范围

✅ **应该包含**:
- mDNS 服务广播 (基于 `mdns-sd`)
- 服务发现和解析
- 服务信息模型 (`ServiceInfo`)
- 属性编解码 (TXT record)
- 冲突检测和重命名

❌ **不应该包含**:
- RPC 框架集成 (应由用户手动集成)
- 连接管理
- 负载均衡
- 健康检查 (应在上层实现)

#### 依赖约束

**可以依赖**:
- `echostream-proto` - 基础类型 (可选,仅用于共享错误类型)
- `mdns-sd` - mDNS 实现
- `tokio` - 异步运行时
- `anyhow` - 错误处理

**禁止依赖**:
- `echostream-core` (保持独立性)
- `echostream-transport`
- `quinn`

#### 独立性设计

- 完全独立于 RPC 框架
- 可单独使用,不强制依赖 `echostream-core`
- 用户手动集成两者

#### 对外接口

```rust
// 服务发现门面
pub struct Discovery;

impl Discovery {
    pub async fn advertise(service: ServiceInfo) -> Result<Advertiser>;
    pub async fn discover(service_name: &str, timeout: Duration) -> Result<Vec<ServiceInfo>>;
    pub fn discover_stream(service_name: &str) -> impl Stream<Item = ServiceInfo>;
}

// 服务信息
pub struct ServiceInfo {
    pub name: String,
    pub address: SocketAddr,
    metadata: HashMap<String, String>,
}

impl ServiceInfo {
    pub fn new(name: impl Into<String>) -> Self;
    pub fn with_port(self, port: u16) -> Self;
    pub fn with_property(self, key: impl Into<String>, value: impl Into<String>) -> Self;
    pub fn get_property(&self, key: &str) -> Option<&str>;
}

// RAII guard
pub struct Advertiser { ... }
```

---

### 2.6 echostream (统一入口)

**核心定位**: 重导出所有公共 API,提供统一入口

#### 职责范围

✅ **应该包含**:
- 重导出 `core` 的所有公共类型
- 重导出 `proto` 的核心类型
- 重导出 `derive` 宏 (可选 feature)
- 重导出 `discovery` API (可选 feature)
- 提供 `prelude` 模块

❌ **不应该包含**:
- 任何实现逻辑
- 新的类型定义

#### Feature 管理

```toml
[features]
default = ["derive", "discovery"]
derive = ["dep:echostream-derive"]
discovery = ["dep:echostream-discovery"]
```

#### 对外接口

```rust
// 重导出核心类型
pub use echostream_core::{
    EchoServer, EchoClient,
    ServerBuilder, ClientBuilder,
    ServerContext, ClientContext, Session,
    RpcHandler, EventHandler, StreamHandler,
    ServerPlugin, ClientPlugin, Middleware,
    StreamReceiver, StreamSender,
};

// 重导出协议类型
pub use echostream_proto::{
    Frame, Header, MessageType,
    SessionId, SessionInfo,
    Timestamped,
    Error, Result,
};

// 重导出宏 (feature = "derive")
#[cfg(feature = "derive")]
pub use echostream_derive::{rpc, event, stream};

// 重导出发现 API (feature = "discovery")
#[cfg(feature = "discovery")]
pub use echostream_discovery::{Discovery, ServiceInfo, Advertiser};

// Prelude 模块
pub mod prelude {
    pub use crate::*;
}
```

---

## 三、模块边界检查清单

### 3.1 依赖方向检查

| 模块 | 可以依赖 | 禁止依赖 |
|------|---------|---------|
| proto | serde, bytes, thiserror | 任何内部 crate、异步运行时 |
| transport | proto, quinn, tokio | core, derive, discovery, postcard |
| core | proto, transport, tokio, postcard | derive, discovery, quinn |
| derive | syn, quote, proc-macro2 | 任何运行时库、任何内部 crate |
| discovery | proto(可选), mdns-sd, tokio | core, transport, derive |
| echostream | 所有内部 crate | 无 (仅重导出) |

### 3.2 职责越界检查

**常见错误示例**:

❌ **错误**: 在 `proto` 中实现网络 I/O
```rust
// echostream-proto/src/lib.rs
impl Frame {
    pub async fn send(&self, conn: &Connection) -> Result<()> { ... }  // ❌ 越界
}
```

✅ **正确**: 在 `transport` 或 `core` 中实现
```rust
// echostream-core/src/rpc/mod.rs
impl RpcDispatcher {
    pub async fn send_frame(&self, frame: Frame) -> Result<()> { ... }  // ✅
}
```

---

❌ **错误**: 在 `transport` 中实现 RPC 路由
```rust
// echostream-transport/src/lib.rs
impl QuicTransport {
    pub fn register_handler(&mut self, name: &str, handler: RpcHandler) { ... }  // ❌ 越界
}
```

✅ **正确**: 在 `core` 中实现
```rust
// echostream-core/src/rpc/dispatcher.rs
impl RpcDispatcher {
    pub fn register(&mut self, handler: impl RpcHandler) { ... }  // ✅
}
```

---

❌ **错误**: 在 `derive` 中依赖运行时类型
```rust
// echostream-derive/src/lib.rs
use echostream_core::Session;  // ❌ 过程宏不能依赖运行时
```

✅ **正确**: 生成引用用户代码中的类型
```rust
// 展开后的代码
use echostream::Session;  // ✅ 在用户代码中引用
```

---

❌ **错误**: 在 `discovery` 中集成 RPC 框架
```rust
// echostream-discovery/src/lib.rs
impl Discovery {
    pub fn create_server(service: ServiceInfo) -> EchoServer { ... }  // ❌ 过度耦合
}
```

✅ **正确**: 用户手动集成
```rust
// 用户代码
let service = ServiceInfo::new("my-service").with_port(5000);
let _advertiser = Discovery::advertise(service).await?;
let server = EchoServer::builder().bind("0.0.0.0:5000").build()?;  // ✅
```

---

## 四、跨模块协作示例

### 4.1 RPC 调用完整流程

```
1. 用户定义处理器 (使用 derive 宏)
   ↓
   #[rpc("add")] async fn add(a: u32, b: u32) -> Result<u32>
   ↓
2. derive 生成 ZST 实现 RpcHandler trait (core 定义)
   ↓
   struct add; impl RpcHandler for add { ... }
   ↓
3. 用户注册到 ServerBuilder (core)
   ↓
   EchoServer::builder().add_rpc(add).build()
   ↓
4. core 接收到 Frame (proto 定义)
   ↓
   Frame { header: Header { method: "add", ... }, payload: Bytes }
   ↓
5. core 使用 transport 发送响应
   ↓
   connection.send_frame(response_frame).await
   ↓
6. transport 通过 QUIC 传输字节流
```

### 4.2 服务发现 + RPC 集成

```
1. 用户创建服务信息 (discovery)
   ↓
   ServiceInfo::new("echo").with_port(5000)
   ↓
2. 广播服务 (discovery)
   ↓
   Discovery::advertise(service).await
   ↓
3. 启动 RPC 服务器 (core)
   ↓
   EchoServer::builder().bind("0.0.0.0:5000").build()
   ↓
4. 客户端发现服务 (discovery)
   ↓
   Discovery::discover("echo", timeout).await
   ↓
5. 客户端连接 (core + transport)
   ↓
   EchoClient::connect(service.address).await
```

---

## 五、常见问题

### Q1: 为什么 proto 不依赖 tokio?

**A**: `proto` 是协议定义层,应该保持零依赖,以便:
- 被所有模块共享,不引入额外依赖
- 支持不同的异步运行时 (tokio/async-std)
- 在非异步环境中使用 (如过程宏)

### Q2: 为什么 transport 不处理序列化?

**A**: 分离关注点:
- `transport` 专注于字节流传输 (QUIC/WebSocket)
- `core` 负责序列化/反序列化 (postcard/JSON)
- 未来可以替换序列化方案,无需修改传输层

### Q3: derive 宏如何知道 RpcHandler trait 的定义?

**A**: 过程宏不需要知道 trait 定义:
- 宏仅生成代码字符串
- 生成的代码在用户的 crate 中编译
- 用户的 crate 依赖 `echostream-core`,可以访问 trait

### Q4: discovery 为什么不依赖 core?

**A**: 保持独立性和灵活性:
- 用户可能只需要服务发现,不使用 RPC 框架
- 避免循环依赖
- 降低编译时间和模块复杂度

### Q5: 如何处理跨模块的类型转换?

**A**: 使用 `From`/`Into` trait:
```rust
// proto 定义基础类型
pub struct SessionInfo { ... }

// core 扩展为完整上下文
pub struct Session {
    info: SessionInfo,
    // 其他字段...
}

impl From<SessionInfo> for Session {
    fn from(info: SessionInfo) -> Self { ... }
}
```

---

## 六、下一步行动

### 6.1 立即执行

1. 根据本文档检查各模块的 `Cargo.toml` 依赖
2. 确保没有违反依赖方向规则
3. 整理各模块的 `lib.rs`,明确公开接口

### 6.2 开发阶段

1. 按照依赖顺序实现模块: proto → transport → core → derive/discovery
2. 每个模块实现后编写单元测试
3. 在 `examples/` 中编写集成示例

### 6.3 持续维护

1. 定期审查模块边界,防止职责泄漏
2. 更新文档,反映实际实现
3. 收集用户反馈,优化模块设计

---

## 七、总结

本文档明确了 EchoStream 各模块的职责边界:

| 模块 | 核心职责 | 关键约束 |
|------|---------|---------|
| proto | 协议定义 | 零依赖、高稳定性 |
| transport | 传输抽象 | 不处理业务逻辑、支持扩展 |
| core | RPC 框架 | 插件化、可扩展 |
| derive | 过程宏 | 零成本、编译期 |
| discovery | 服务发现 | 独立、可选 |
| echostream | 统一入口 | 仅重导出 |

遵循这些职责边界,可以确保:
- 模块间低耦合、高内聚
- 清晰的依赖关系
- 良好的扩展性
- 易于维护和测试
