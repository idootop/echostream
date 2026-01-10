# EchoStream 全局 API 设计

> 统一各模块接口,确保一致性和可扩展性

## 一、设计原则

### 1.1 命名一致性

- **Builder 模式**: 所有配置类使用 `builder()` + `build()` 模式
- **异步方法**: 所有 I/O 操作返回 `async` Future
- **Result 类型**: 可失败操作统一返回 `Result<T, Error>`
- **上下文传递**: 使用 `Context`/`Session` 作为第一参数

### 1.2 分层职责

```
echostream-proto     → 定义协议和基础类型 (零依赖)
echostream-transport → 封装传输层实现 (QUIC)
echostream-core      → 实现框架核心逻辑 (RPC/Stream/Plugin)
echostream-derive    → 提供声明式宏 (零成本抽象)
echostream-discovery → 提供服务发现 (独立可选)
echostream           → 统一入口 (重导出)
```

---

## 二、核心类型定义

### 2.1 echostream-proto (协议层)

#### 2.1.1 消息帧

```rust
/// 消息帧 - 传输的基本单位
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub header: Header,
    pub payload: Bytes,
}

/// 帧头 - 路由和控制信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub message_type: MessageType,  // 消息类型
    pub request_id: u64,             // 请求 ID (用于 RPC 匹配)
    pub method: String,              // 方法名/事件名/流名
    pub flags: u8,                   // 控制标志位
}

/// 消息类型
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MessageType {
    Request,   // RPC 请求
    Response,  // RPC 响应
    Event,     // 单向事件
    Stream,    // 流数据
}
```

#### 2.1.2 会话和上下文

```rust
/// 会话标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

/// 会话信息 - 单个连接的元数据
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: SessionId,
    pub peer_addr: SocketAddr,
    pub created_at: SystemTime,
}

/// 时间戳包装器 - 用于流数据的时间对齐
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timestamped<T> {
    pub wall_time: u64,  // 对齐后的绝对时间 (微秒)
    pub seq: u32,        // 序列号
    pub data: T,         // 实际数据
}
```

#### 2.1.3 错误定义

```rust
/// 统一错误类型
#[derive(Error, Debug)]
pub enum Error {
    #[error("序列化失败: {0}")]
    Serialization(String),

    #[error("协议错误: {0}")]
    Protocol(String),

    #[error("超时")]
    Timeout,

    #[error("连接已关闭")]
    ConnectionClosed,

    #[error("未找到方法: {0}")]
    MethodNotFound(String),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
```

---

### 2.2 echostream-transport (传输层)

#### 2.2.1 传输层抽象

```rust
/// 传输层 Trait - 支持未来扩展 WebTransport 等
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    type Connection: Connection;

    /// 监听指定地址
    async fn listen(&self, addr: SocketAddr) -> Result<Listener<Self::Connection>>;

    /// 连接到远程地址
    async fn connect(&self, addr: SocketAddr) -> Result<Self::Connection>;
}

/// 监听器 - 接受新连接
pub struct Listener<C> {
    // 内部实现
}

impl<C: Connection> Listener<C> {
    /// 接受下一个连接
    pub async fn accept(&mut self) -> Result<C> { /* ... */ }
}
```

#### 2.2.2 连接抽象

```rust
/// 连接 Trait
#[async_trait]
pub trait Connection: Send + Sync + 'static {
    /// 打开双向流
    async fn open_bi_stream(&self) -> Result<(SendStream, RecvStream)>;

    /// 接受双向流
    async fn accept_bi_stream(&self) -> Result<(SendStream, RecvStream)>;

    /// 发送数据报 (无序、不可靠)
    async fn send_datagram(&self, data: Bytes) -> Result<()>;

    /// 接收数据报
    async fn recv_datagram(&self) -> Result<Bytes>;

    /// 获取对端地址
    fn peer_addr(&self) -> SocketAddr;

    /// 关闭连接
    async fn close(&self) -> Result<()>;
}

/// 发送流
#[async_trait]
pub trait SendStream: Send + Sync {
    async fn write(&mut self, data: &[u8]) -> Result<()>;
    async fn finish(&mut self) -> Result<()>;
}

/// 接收流
#[async_trait]
pub trait RecvStream: Send + Sync {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    async fn read_to_end(&mut self, max_size: usize) -> Result<Bytes>;
}
```

#### 2.2.3 QUIC 配置

```rust
/// QUIC 传输层实现
pub struct QuicTransport {
    config: QuicConfig,
}

impl QuicTransport {
    pub fn new(config: QuicConfig) -> Self { /* ... */ }
}

/// QUIC 配置构建器
pub struct QuicConfig {
    // 内部字段
}

impl QuicConfig {
    pub fn builder() -> QuicConfigBuilder {
        QuicConfigBuilder::default()
    }
}

pub struct QuicConfigBuilder {
    // 内部字段
}

impl QuicConfigBuilder {
    /// 使用自签名证书 (开发环境)
    pub fn with_self_signed_cert(mut self) -> Self { /* ... */ }

    /// 使用 CA 证书 (生产环境)
    pub fn with_cert_chain(mut self, cert: impl AsRef<Path>, key: impl AsRef<Path>) -> Self { /* ... */ }

    /// 跳过证书验证 (开发环境)
    pub fn skip_cert_verification(mut self) -> Self { /* ... */ }

    /// 启用 0-RTT
    pub fn enable_0rtt(mut self) -> Self { /* ... */ }

    /// 设置拥塞控制算法
    pub fn congestion_control(mut self, algo: CongestionControl) -> Self { /* ... */ }

    /// 最大并发流数量
    pub fn max_concurrent_streams(mut self, count: usize) -> Self { /* ... */ }

    pub fn build(self) -> Result<QuicConfig> { /* ... */ }
}

/// 拥塞控制算法
pub enum CongestionControl {
    Cubic,
    BBR,
    NewReno,
}
```

---

### 2.3 echostream-core (核心框架)

#### 2.3.1 服务端 API

```rust
/// 服务端
pub struct EchoServer {
    // 内部实现
}

impl EchoServer {
    /// 创建 Builder
    pub fn builder() -> ServerBuilder {
        ServerBuilder::default()
    }

    /// 运行服务器 (阻塞直到关闭)
    pub async fn run(self) -> Result<()> { /* ... */ }

    /// 获取服务端上下文
    pub fn context(&self) -> &ServerContext { /* ... */ }
}

/// 服务端构建器
pub struct ServerBuilder {
    // 内部字段
}

impl ServerBuilder {
    /// 绑定监听地址
    pub fn bind(mut self, addr: impl ToSocketAddrs) -> Self { /* ... */ }

    /// 设置传输层 (默认使用 QUIC)
    pub fn transport<T: Transport>(mut self, transport: T) -> Self { /* ... */ }

    /// 注册 RPC 处理器
    pub fn add_rpc<H: RpcHandler>(mut self, handler: H) -> Self { /* ... */ }

    /// 注册事件处理器
    pub fn add_event<H: EventHandler>(mut self, handler: H) -> Self { /* ... */ }

    /// 注册流处理器
    pub fn add_stream<H: StreamHandler>(mut self, handler: H) -> Self { /* ... */ }

    /// 添加插件
    pub fn plugin<P: ServerPlugin>(mut self, plugin: P) -> Self { /* ... */ }

    /// 添加中间件
    pub fn middleware<M: Middleware>(mut self, middleware: M) -> Self { /* ... */ }

    /// 服务启动钩子
    pub fn on_start<F>(mut self, f: F) -> Self
    where
        F: Fn(ServerContext) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    { /* ... */ }

    /// 服务关闭钩子
    pub fn on_stop<F>(mut self, f: F) -> Self
    where
        F: Fn(ServerContext) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    { /* ... */ }

    /// 客户端连接钩子
    pub fn on_connect<F>(mut self, f: F) -> Self
    where
        F: Fn(Session) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    { /* ... */ }

    /// 客户端断开钩子
    pub fn on_disconnect<F>(mut self, f: F) -> Self
    where
        F: Fn(Session) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    { /* ... */ }

    /// 构建服务端
    pub fn build(self) -> Result<EchoServer> { /* ... */ }
}
```

#### 2.3.2 客户端 API

```rust
/// 客户端
pub struct EchoClient {
    // 内部实现
}

impl EchoClient {
    /// 快速连接 (使用默认配置)
    pub async fn connect(addr: impl ToSocketAddrs) -> Result<Self> { /* ... */ }

    /// 创建 Builder
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// 发送 RPC 请求
    pub async fn request<Req, Resp>(&self, method: &str, req: Req) -> Result<Resp>
    where
        Req: Serialize,
        Resp: DeserializeOwned,
    { /* ... */ }

    /// 发送事件
    pub async fn emit<T: Serialize>(&self, event: &str, data: T) -> Result<()> { /* ... */ }

    /// 创建流
    pub async fn create_stream(&self, name: &str) -> Result<StreamSender> { /* ... */ }

    /// 获取客户端上下文
    pub fn context(&self) -> &ClientContext { /* ... */ }

    /// 关闭连接
    pub async fn close(&self) -> Result<()> { /* ... */ }
}

/// 客户端构建器
pub struct ClientBuilder {
    // 内部字段
}

impl ClientBuilder {
    /// 设置传输层
    pub fn transport<T: Transport>(mut self, transport: T) -> Self { /* ... */ }

    /// 添加插件
    pub fn plugin<P: ClientPlugin>(mut self, plugin: P) -> Self { /* ... */ }

    /// 添加中间件
    pub fn middleware<M: Middleware>(mut self, middleware: M) -> Self { /* ... */ }

    /// 连接成功钩子
    pub fn on_connect<F>(mut self, f: F) -> Self
    where
        F: Fn(ClientContext) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    { /* ... */ }

    /// 连接断开钩子
    pub fn on_disconnect<F>(mut self, f: F) -> Self
    where
        F: Fn(ClientContext) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    { /* ... */ }

    /// 连接到服务器
    pub async fn connect(self, addr: impl ToSocketAddrs) -> Result<EchoClient> { /* ... */ }
}
```

#### 2.3.3 上下文和会话

```rust
/// 服务端全局上下文
#[derive(Clone)]
pub struct ServerContext {
    // 内部实现
}

impl ServerContext {
    /// 存储数据
    pub fn set<T: Send + Sync + 'static>(&self, key: &str, value: T) { /* ... */ }

    /// 获取数据
    pub fn get<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> { /* ... */ }

    /// 移除数据
    pub fn remove<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> { /* ... */ }

    /// 广播事件到所有客户端
    pub async fn broadcast<T: Serialize>(&self, event: &str, data: T) -> Result<()> { /* ... */ }

    /// 获取所有会话
    pub fn sessions(&self) -> Vec<Session> { /* ... */ }
}

/// 客户端全局上下文
#[derive(Clone)]
pub struct ClientContext {
    // 内部实现
}

impl ClientContext {
    /// 存储数据
    pub fn set<T: Send + Sync + 'static>(&self, key: &str, value: T) { /* ... */ }

    /// 获取数据
    pub fn get<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> { /* ... */ }

    /// 移除数据
    pub fn remove<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> { /* ... */ }
}

/// 会话 - 单个客户端连接的上下文
#[derive(Clone)]
pub struct Session {
    // 内部实现
}

impl Session {
    /// 获取会话 ID
    pub fn id(&self) -> SessionId { /* ... */ }

    /// 获取对端地址
    pub fn peer_addr(&self) -> SocketAddr { /* ... */ }

    /// 获取创建时间
    pub fn created_at(&self) -> SystemTime { /* ... */ }

    /// 访问服务端全局上下文
    pub fn server_ctx(&self) -> &ServerContext { /* ... */ }

    /// 存储会话级数据
    pub fn set<T: Send + Sync + 'static>(&self, key: &str, value: T) { /* ... */ }

    /// 获取会话级数据
    pub fn get<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> { /* ... */ }

    /// 移除会话级数据
    pub fn remove<T: Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> { /* ... */ }

    /// 向该客户端发送事件
    pub async fn emit<T: Serialize>(&self, event: &str, data: T) -> Result<()> { /* ... */ }

    /// 向该客户端发送 RPC 请求 (服务端主动调用客户端)
    pub async fn request<Req, Resp>(&self, method: &str, req: Req) -> Result<Resp>
    where
        Req: Serialize,
        Resp: DeserializeOwned,
    { /* ... */ }

    /// 关闭连接
    pub async fn close(&self) -> Result<()> { /* ... */ }
}
```

#### 2.3.4 处理器 Trait

```rust
/// RPC 处理器
#[async_trait]
pub trait RpcHandler: Send + Sync + 'static {
    /// 方法名
    fn name(&self) -> &'static str;

    /// 处理请求
    async fn handle(&self, session: Session, payload: Bytes) -> Result<Bytes>;
}

/// 事件处理器
#[async_trait]
pub trait EventHandler: Send + Sync + 'static {
    /// 事件名
    fn name(&self) -> &'static str;

    /// 处理事件
    async fn handle(&self, session: Session, payload: Bytes);
}

/// 流处理器
#[async_trait]
pub trait StreamHandler: Send + Sync + 'static {
    /// 流名
    fn name(&self) -> &'static str;

    /// 处理流
    async fn handle(&self, session: Session, stream: StreamReceiver);
}

/// 流接收器
pub struct StreamReceiver {
    // 内部实现
}

impl StreamReceiver {
    /// 接收下一个数据帧
    pub async fn recv(&mut self) -> Option<Timestamped<Bytes>> { /* ... */ }

    /// 关闭流
    pub async fn close(&mut self) -> Result<()> { /* ... */ }
}

/// 流发送器
pub struct StreamSender {
    // 内部实现
}

impl StreamSender {
    /// 发送数据
    pub async fn send(&mut self, data: impl Into<Bytes>) -> Result<()> { /* ... */ }

    /// 发送带时间戳的数据
    pub async fn send_timestamped(&mut self, data: Timestamped<Bytes>) -> Result<()> { /* ... */ }

    /// 关闭流
    pub async fn finish(&mut self) -> Result<()> { /* ... */ }
}
```

#### 2.3.5 插件系统

```rust
/// 服务端插件
pub trait ServerPlugin: Send + Sync + 'static {
    /// 插件名称
    fn name(&self) -> &str;

    /// 安装插件 (修改 ServerBuilder)
    fn install(self, builder: ServerBuilder) -> Result<ServerBuilder>;
}

/// 客户端插件
pub trait ClientPlugin: Send + Sync + 'static {
    /// 插件名称
    fn name(&self) -> &str;

    /// 安装插件 (修改 ClientBuilder)
    fn install(self, builder: ClientBuilder) -> Result<ClientBuilder>;
}
```

#### 2.3.6 中间件系统

```rust
/// 中间件 - 处理消息流 (类 Tower Layer)
#[async_trait]
pub trait Middleware: Send + Sync + 'static {
    /// 处理请求 (返回 None 表示拦截)
    async fn process_request(&self, frame: &Frame) -> Result<Option<Frame>> {
        Ok(Some(frame.clone()))
    }

    /// 处理响应
    async fn process_response(&self, frame: &Frame) -> Result<Option<Frame>> {
        Ok(Some(frame.clone()))
    }
}
```

---

### 2.4 echostream-derive (过程宏)

#### 2.4.1 宏定义

```rust
/// RPC 处理器宏
///
/// # 支持的函数签名
/// - `async fn(Session, Req) -> Result<Resp>`
/// - `async fn(Req) -> Result<Resp>`
/// - `async fn(Session) -> Result<Resp>`
/// - `async fn() -> Result<Resp>`
#[proc_macro_attribute]
pub fn rpc(attr: TokenStream, item: TokenStream) -> TokenStream {
    // 实现...
}

/// 事件处理器宏
///
/// # 支持的函数签名
/// - `async fn(Session, Data)`
/// - `async fn(Data)`
/// - `async fn(Session)`
/// - `async fn()`
#[proc_macro_attribute]
pub fn event(attr: TokenStream, item: TokenStream) -> TokenStream {
    // 实现...
}

/// 流处理器宏
///
/// # 支持的函数签名
/// - `async fn(Session, StreamReceiver)`
#[proc_macro_attribute]
pub fn stream(attr: TokenStream, item: TokenStream) -> TokenStream {
    // 实现...
}
```

#### 2.4.2 使用示例

```rust
use echostream::prelude::*;

// RPC 处理器
#[rpc("user.login")]
async fn login(session: Session, req: LoginReq) -> Result<LoginResp> {
    // 业务逻辑
    Ok(LoginResp { token: "...".into() })
}

// 自动使用函数名
#[rpc]
async fn add(a: u32, b: u32) -> Result<u32> {
    Ok(a + b)
}

// 事件处理器
#[event("user.logout")]
async fn on_logout(session: Session, user_id: u64) {
    println!("用户 {} 已登出", user_id);
}

// 流处理器
#[stream("audio.stream")]
async fn handle_audio(session: Session, mut stream: StreamReceiver) {
    while let Some(frame) = stream.recv().await {
        // 处理音频帧
    }
}
```

---

### 2.5 echostream-discovery (服务发现)

#### 2.5.1 核心 API

```rust
/// 服务发现门面
pub struct Discovery;

impl Discovery {
    /// 广播服务 (返回 guard,drop 时停止广播)
    pub async fn advertise(service: ServiceInfo) -> Result<Advertiser> { /* ... */ }

    /// 发现服务 (超时返回已发现的服务列表)
    pub async fn discover(service_name: &str, timeout: Duration) -> Result<Vec<ServiceInfo>> { /* ... */ }

    /// 流式发现服务 (返回 Stream,持续接收新服务)
    pub fn discover_stream(service_name: &str) -> impl Stream<Item = ServiceInfo> { /* ... */ }
}

/// 服务广播器 (RAII guard)
pub struct Advertiser {
    // 内部实现
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        // 停止广播
    }
}

/// 服务信息
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub address: SocketAddr,
    metadata: HashMap<String, String>,
}

impl ServiceInfo {
    /// 创建服务信息
    pub fn new(name: impl Into<String>) -> Self { /* ... */ }

    /// 设置端口 (默认自动选择)
    pub fn with_port(mut self, port: u16) -> Self { /* ... */ }

    /// 添加元数据
    pub fn with_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self { /* ... */ }

    /// 获取元数据
    pub fn get_property(&self, key: &str) -> Option<&str> { /* ... */ }
}
```

#### 2.5.2 使用示例

```rust
use echostream_discovery::{Discovery, ServiceInfo};

// 广播服务
let service = ServiceInfo::new("echo-server")
    .with_port(5000)
    .with_property("protocol", "quic")
    .with_property("version", "0.1.0");

let _advertiser = Discovery::advertise(service).await?;

// 发现服务 (超时 3 秒)
let services = Discovery::discover("echo-server", Duration::from_secs(3)).await?;
for service in services {
    println!("发现服务: {} at {}", service.name, service.address);
}

// 流式发现
let mut stream = Discovery::discover_stream("echo-server");
while let Some(service) = stream.next().await {
    println!("新服务上线: {} at {}", service.name, service.address);
}
```

---

## 三、API 一致性检查清单

### 3.1 命名规范

- [ ] Builder 模式统一使用 `builder()` + `build()`
- [ ] 配置方法统一使用 `with_*` 前缀
- [ ] 添加操作统一使用 `add_*` 前缀
- [ ] 钩子方法统一使用 `on_*` 前缀
- [ ] 异步方法统一返回 `async` Future
- [ ] 错误类型统一使用 `Result<T, Error>`

### 3.2 参数传递

- [ ] 上下文参数放在第一位: `fn(Session, ...)`
- [ ] 泛型参数使用 `impl Trait` 简化签名
- [ ] 字符串参数使用 `impl Into<String>`
- [ ] 地址参数使用 `impl ToSocketAddrs`

### 3.3 生命周期管理

- [ ] RAII guard 模式 (如 `Advertiser`)
- [ ] `Drop` 自动清理资源
- [ ] `Clone` 支持共享上下文
- [ ] `Send + Sync + 'static` 约束异步组件

### 3.4 错误处理

- [ ] 统一使用 `echostream_proto::Result<T>`
- [ ] 错误类型实现 `std::error::Error`
- [ ] 使用 `thiserror` 派生宏
- [ ] 区分可恢复错误和致命错误

---

## 四、模块间接口契约

### 4.1 proto → transport

- `proto` 定义 `Result<T>` 和 `Error`
- `transport` 使用 `proto::Result<T>` 作为返回值

### 4.2 transport → core

- `transport` 提供 `Transport` 和 `Connection` trait
- `core` 使用 `Transport::Connection` 创建连接

### 4.3 core → derive

- `core` 定义 `RpcHandler`、`EventHandler`、`StreamHandler` trait
- `derive` 生成实现这些 trait 的零大小类型 (ZST)

### 4.4 core ← discovery

- `discovery` 独立运作,不依赖 `core`
- 用户代码手动集成两者 (通过配置地址)

---

## 五、扩展性考虑

### 5.1 传输层扩展

```rust
// 未来支持 WebTransport
pub struct WebTransport { ... }

impl Transport for WebTransport {
    // 实现相同接口
}
```

### 5.2 序列化扩展

```rust
// 默认使用 postcard,但支持自定义
pub trait Serializer {
    fn serialize<T: Serialize>(&self, value: &T) -> Result<Bytes>;
    fn deserialize<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T>;
}
```

### 5.3 插件扩展

```rust
// 用户自定义插件
struct MyPlugin { ... }

impl ServerPlugin for MyPlugin {
    fn name(&self) -> &str { "my-plugin" }
    fn install(self, builder: ServerBuilder) -> Result<ServerBuilder> {
        Ok(builder.add_rpc(...))
    }
}
```

---

## 六、完整示例

### 6.1 服务端

```rust
use echostream::prelude::*;

#[rpc("math.add")]
async fn add(a: u32, b: u32) -> Result<u32> {
    Ok(a + b)
}

#[event("log")]
async fn on_log(session: Session, message: String) {
    println!("[{}] {}", session.peer_addr(), message);
}

#[stream("audio")]
async fn handle_audio(session: Session, mut stream: StreamReceiver) {
    while let Some(frame) = stream.recv().await {
        // 处理音频
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = EchoServer::builder()
        .bind("0.0.0.0:5000")
        .add_rpc(add)
        .add_event(on_log)
        .add_stream(handle_audio)
        .on_start(|ctx| async move {
            println!("服务启动");
            Ok(())
        })
        .on_connect(|session| async move {
            println!("客户端连接: {}", session.peer_addr());
            Ok(())
        })
        .build()?;

    server.run().await
}
```

### 6.2 客户端

```rust
use echostream::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = EchoClient::connect("127.0.0.1:5000").await?;

    // RPC 调用
    let result: u32 = client.request("math.add", (10, 20)).await?;
    println!("10 + 20 = {}", result);

    // 发送事件
    client.emit("log", "Hello from client!").await?;

    // 创建流
    let mut stream = client.create_stream("audio").await?;
    stream.send(b"audio data...").await?;
    stream.finish().await?;

    Ok(())
}
```

### 6.3 服务发现集成

```rust
use echostream::prelude::*;
use echostream_discovery::{Discovery, ServiceInfo};

#[tokio::main]
async fn main() -> Result<()> {
    // 服务端: 广播服务
    let service = ServiceInfo::new("echo-server")
        .with_port(5000)
        .with_property("protocol", "quic");
    let _advertiser = Discovery::advertise(service).await?;

    let server = EchoServer::builder()
        .bind("0.0.0.0:5000")
        .build()?;

    server.run().await
}

// 客户端: 发现并连接
#[tokio::main]
async fn main() -> Result<()> {
    let services = Discovery::discover("echo-server", Duration::from_secs(3)).await?;
    let addr = services.first().ok_or("未找到服务")?.address;

    let client = EchoClient::connect(addr).await?;
    // ...
}
```

---

## 七、待明确问题

### 7.1 序列化方案

**问题**: 是否需要支持多种序列化格式?

**建议**:
- 默认使用 `postcard` (零拷贝、高性能)
- 预留扩展点,支持自定义 `Serializer` trait

### 7.2 流控制

**问题**: 如何实现背压 (backpressure)?

**建议**:
- `StreamSender::send()` 使用内部缓冲区
- 缓冲区满时 `send()` 阻塞,实现背压

### 7.3 重连机制

**问题**: 客户端断线后是否自动重连?

**建议**:
- 默认不自动重连 (保持简单)
- 提供 `ReconnectPlugin` 插件实现自动重连

### 7.4 时间同步精度

**问题**: 时间同步的精度要求?

**建议**:
- 使用类 NTP 算法,精度在 1-10ms
- 提供 `ClockSync` API 供用户查询时钟偏移

---

## 八、总结

本设计文档统一了 EchoStream 各模块的 API 接口,确保:

1. **一致性**: 命名、参数传递、错误处理统一
2. **简洁性**: Builder 模式、声明式宏减少样板代码
3. **扩展性**: Trait 抽象、插件系统支持自定义扩展
4. **类型安全**: 泛型约束、编译期检查
5. **异步友好**: 统一使用 `async/await`,避免回调地狱

下一步工作:
- 根据本设计文档实现各模块核心功能
- 编写单元测试和集成测试
- 补充文档和示例代码
