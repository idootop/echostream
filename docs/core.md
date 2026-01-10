# EchoStream

一个基于 QUIC 协议的 Rust 高性能异步双向 RPC 与流传输框架。

## 项目结构

```text
src/
├── core/           # 协议核心：Frame, Codec
├── transport/      # QUIC 封装：Client, Server, Connection
├── router/         # 路由与分发：Handler, Middleware
├── context/        # 上下文管理：Context, Session, Plugin
├── macros/         # 过程宏：derive(Rpc), derive(Event), derive(Stream)
└── error.rs        # 统一错误处理
```

---

## 2. 消息模型：Frame 与 Payload

为了兼顾效率与扩展性，Frame 采用 Header + Body 的结构。

### 2.1 基础帧定义

```rust
pub struct Frame {
    pub metadata: Metadata,
    pub payload: Payload,
}

pub struct Metadata(HashMap<String, String>);

pub enum Payload {
    Request(RpcRequest),
    Response(RpcResponse),
    Event(EventPack),
    Stream(StreamFrame),
    Ping(u64), // 时间戳
    Pong(u64), // 时间戳回传
}

```

### 2.2 具体负载

```rust
pub struct RpcRequest {
    pub id: u32,
    pub name: String,
    pub data: Vec<u8>,
}

pub struct RpcResponse {
    pub id: u32,
    pub code: u16,
    pub message: String,
    pub data: Vec<u8>,
}

pub struct EventPack {
    pub name: String,
    pub data: Vec<u8>,
}

pub struct StreamFrame {
    pub stream_id: u64,
    pub seq: u64,
    pub timestamp: i64,
    pub data: Vec<u8>,
}

```

---

## 3. 状态管理：Context 与 Session

### 3.1 Context (生命周期与全局状态)

Context 代表应用级的环境，管理 Server/Client 的启停钩子和全局 K-V 存储。

```rust
pub struct Context {
    state: DashMap<String, Box<dyn Any + Send + Sync>>,
}

impl Context {
    pub async fn start(&self) { ... }
    pub async fn stop(&self) { ... }
    pub fn set<T: Send + Sync + 'static>(&self, key: &str, val: T);
    pub fn get<T: 'static>(&self, key: &str) -> Option<Ref<String, Box<dyn Any>>>;
}

```

### 3.2 Session (会话级操作)

Session 是对具体 QUIC 连接的抽象，是用户操作的主入口。

```rust
#[derive(Clone)]
pub struct Session {
    id: u64,
    conn: quinn::Connection,
    state: DashMap<String, Box<dyn Any + Send + Sync>>,
    ctx: Arc<Context>,
}

impl Session {
    // 发送 RPC 并等待响应
    pub async fn request(&self, name: &str, data: Vec<u8>) -> Result<RpcResponse>;

    // 发送单向事件
    pub async fn emit(&self, name: &str, data: Vec<u8>) -> Result<()>;

    // 发送流数据帧（通常用于音视频或实时传感器数据）
    pub async fn write_stream(&self, stream_id: u64, data: Vec<u8>) -> Result<()>;
}

```

---

## 4. 路由与处理器：Handler

### 4.1 处理器定义

通过 Trait 定义不同类型的处理逻辑，支持异步 `async_trait`。

```rust
#[async_trait]
pub trait RpcHandler: Send + Sync {
    fn name(&self) -> &str;
    async fn handle(&self, session: Session, req: RpcRequest) -> RpcResponse;
}

#[async_trait]
pub trait EventHandler: Send + Sync {
    fn name(&self) -> &str;
    async fn handle(&self, session: Session, event: EventPack);
}

```

### 4.2 路由注册器

```rust
pub struct Router {
    rpc_handlers: HashMap<String, Box<dyn RpcHandler>>,
    event_handlers: HashMap<String, Vec<Box<dyn EventHandler>>>,
}

```

---

## 5. 插件与中间件 (控制面 vs 数据面)

### 5.1 Middleware (数据面)

作用于每一帧的编解码前后，适合：鉴权、日志、压缩、统计。

```rust
#[async_trait]
pub trait Middleware: Send + Sync {
    // 返回 Ok(Some(frame)) 继续传递，Ok(None) 拦截，Err 报错
    async fn on_frame_inbound(&self, session: &Session, frame: Frame) -> Result<Option<Frame>>;
    async fn on_frame_outbound(&self, session: &Session, frame: Frame) -> Result<Option<Frame>>;
}

```

### 5.2 Plugin (控制面)

作用于整个生命周期，可以动态向 Session/Context 注入 Handler。

```rust
pub trait Plugin: Send + Sync {
    fn on_server_start(&self, ctx: &Context) {}
    fn on_new_session(&self, session: &Session) {}
    fn on_session_closed(&self, session: &Session) {}
}

```

---

## 6. 开发体验：Derive 宏

为了让“写 RPC 像写本地函数”，通过宏自动生成 Stub。

```rust
#[quark_rpc]
pub trait ChatService {
    // 定义 RPC
    #[rpc]
    async fn login(&self, user: String) -> Response<u64>;

    // 定义事件订阅
    #[event]
    async fn on_new_message(&self, msg: Message);

    // 定义实时流
    #[stream]
    async fn video_feed(&self, frame: VideoFrame);
}

```

---

## 7. 系统实施细节 (Flow)

### 7.1 请求响应流 (RPC)

1. **Client**: `session.request("login", data)`。
2. **Internal**: 生成全局唯一 `id`，创建一个 `oneshot::channel`，将 `id` 与 `sender` 存入等待队列，将 `Frame` 序列化并写入 QUIC Bi-stream。
3. **Server**: 监听流，解析 `Frame`，根据 `name` 找到 `RpcHandler`，执行后返回 `RpcResponse`。
4. **Client**: 收到响应帧，根据 `id` 唤醒等待中的 Future。

### 7.2 实时流 (Stream)

1. 利用 QUIC 的 **Unreliable Datagram** (若网络环境允许) 或开辟专用 **Uni-stream**。
2. 通过 `seq` 和 `timestamp` 在应用层处理抖动缓冲（Jitter Buffer），不阻塞控制信令。
