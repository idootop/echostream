# EchoStream

> 基于 QUIC 的双向 RPC 和流传输框架,让实时通信像写本地函数一样简单

## 项目简介

专为实时通信场景设计的 Rust RPC 框架,融合传统 RPC 的便利性和实时流传输能力,通过 QUIC 协议在单连接上同时处理控制信令和实时数据流。

## 项目亮点

- **双向 RPC**: 客户端和服务端都能主动发起请求、推送事件和创建流
- **多模态信令**: Request/Response、Event、Stream 三种通信模式
- **零配置发现**: 基于 mDNS 的局域网服务自动发现,也支持手动指定地址
- **时间同步**: 内置时钟同步协议,确保分布式节点时间对齐
- **流式传输**: 低延迟音视频数据传输,配备抖动缓冲和时间戳对齐
- **QUIC 优势**: 0-RTT 握手、多路复用、自动拥塞控制、无队头阻塞
- **安全传输**: 内置 TLS 1.3 加密
- **声明式 API**: 过程宏简化代码,最小化样板
- **插件系统**: 模块化扩展机制,支持生命周期 hook 和配置定制

## 项目架构

### 目录结构

采用 Cargo Workspace 管理的 monorepo 架构:

```
echostream/
├── Cargo.toml               # Workspace 定义
├── README.md                # 项目说明
├── CLAUDE.md                # AI 辅助开发指南
├── docs/                    # 文档目录
│   ├── README.md            # 详细设计文档
│   └── CHANGELOG.md         # 版本更新日志
├── crates/                  # 所有 Rust crates
│   ├── echostream/          # 统一入口,重导出所有公共 API
│   ├── echostream-core/     # 核心框架(RPC、流传输、连接管理)
│   ├── echostream-discovery/# 服务发现(mDNS)
│   ├── echostream-derive/   # 过程宏(handler、listener、stream_handler)
│   └── echostream-types/    # 公共类型和错误定义
├── examples/                # 示例代码
│   ├── simple_rpc.rs        # 基础 RPC 调用示例
│   ├── event_bus.rs         # 事件总线示例
│   ├── audio_stream.rs      # 音频流传输示例
│   └── service_discovery.rs # 服务发现示例
└── sdk/                     # 其他语言绑定(未来)
    ├── nodejs/              # Node.js 绑定
    └── python/              # Python 绑定
```

### 模块列表

#### 1. echostream-core

核心框架,实现 RPC 和流传输能力。

**子模块划分:**

- `connection/`: QUIC 连接生命周期管理
- `protocol/`: 帧定义、编解码、时间同步协议
- `rpc/`: RPC 框架(请求路由、处理器注册)
- `stream/`: 流管理、时间戳对齐、抖动缓冲
- `plugin/`: 插件系统(ServerPlugin、ClientPlugin trait 定义)
- `server/`: 服务端实现
- `client/`: 客户端实现

**外部依赖:**

- `quinn`: QUIC 协议实现
- `tokio`: 异步运行时
- `postcard`: 零拷贝序列化/反序列化
- `serde`: 序列化框架
- `bytes`: 零拷贝字节操作
- `tracing`: 结构化日志

**核心 API 设计:**

```rust
// 服务端
let server = RpcServer::builder()
    .bind("0.0.0.0:5000")
    .handler(handle_play)
    .build()?;
server.run().await?;

// 客户端
let client = RpcClient::connect("127.0.0.1:5000").await?;
let result: Response = client.request("method", payload).await?;
```

#### 2. echostream-discovery

基于 mDNS 的局域网服务发现。

**子模块划分:**

- `advertiser.rs`: 服务广播实现
- `resolver.rs`: 服务发现和解析
- `service.rs`: 服务信息定义

**外部依赖:**

- `mdns-sd`: mDNS 协议实现
- `tokio`: 异步运行时

**核心 API 设计:**

```rust
// 服务端广播
let server = RpcServer::builder()
    .bind("0.0.0.0:5000")
    .enable_discovery("MyService")
    .build()?;

// 客户端自动发现(局域网)
let client = RpcClient::discover("MyService").await?;

// 或手动指定地址(公网)
let client = RpcClient::connect("example.com:5000").await?;
```

#### 3. echostream-derive

过程宏,简化处理器定义。

**子模块划分:**

- `handler.rs`: 请求处理器宏
- `listener.rs`: 事件监听器宏
- `stream_handler.rs`: 流处理器宏

**外部依赖:**

- `syn`: 解析 Rust 语法
- `quote`: 生成 Rust 代码
- `proc-macro2`: 过程宏工具

**核心 API 设计:**

```rust
#[echostream::handler("user.login")]
async fn login(session: Session, req: LoginReq) -> Result<Session> {
    Ok(Session::new(req.username))
}

#[echostream::listener("user.logout")]
async fn on_logout(session: Session, user_id: u64) {
    println!("用户 {} 已登出", user_id);
}

#[echostream::stream_handler("audio.stream")]
async fn handle_stream(session: Session, stream: StreamReceiver) {
    while let Some(data) = stream.recv().await {
        process(data);
    }
}
```

#### 4. echostream-types

公共类型、错误定义和工具函数。

**子模块划分:**

- `error.rs`: 统一错误类型
- `context.rs`: 请求上下文(ServerContext、ClientContext)
- `session.rs`: Session 会话定义
- `timestamp.rs`: 时间戳相关类型

**外部依赖:**

- `serde`: 序列化支持
- `thiserror`: 错误派生宏

#### 5. echostream

统一入口 crate,重导出所有公共 API。

**核心 API 设计:**

```rust
pub use echostream_core::{RpcServer, RpcClient, ServerContext, ClientContext, Session};
pub use echostream_derive::{handler, listener, stream_handler};
pub use echostream_types::{Result, Error};

pub mod prelude {
    pub use super::*;
}
```

## 快速上手

> **开发中**: API 可能会变化

### 安装

```toml
[dependencies]
echostream = "0.1"
```

### 服务端示例

```rust
use echostream::prelude::*;

#[echostream::handler("audio.play")]
async fn handle_play(session: Session, file: String) -> Result<()> {
    println!("客户端 {} 请求播放: {}", session.peer_addr(), file);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = RpcServer::builder()
        .bind("0.0.0.0:5000")
        .enable_discovery("AudioService") // 可选:启用局域网发现
        .handler(handle_play)
        .build()?;

    server.run().await
}
```

### 客户端示例

```rust
use echostream::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // 方式1: 自动发现(局域网)
    let client = RpcClient::discover("AudioService").await?;

    // 方式2: 手动指定(公网)
    // let client = RpcClient::connect("example.com:5000").await?;

    // 发送请求
    client.request("audio.play", "music.mp3").await?;

    // 发送事件
    client.emit("audio.stopped", ()).await?;

    // 创建流
    let stream = client.create_stream("audio.stream").await?;
    stream.send(audio_data).await?;

    Ok(())
}
```

## 核心概念

### 1. Request/Response

标准 RPC 调用,客户端发送请求并等待响应:

```rust
// 服务端
#[echostream::handler("user.login")]
async fn login(session: Session, username: String) -> Result<UserInfo> {
    Ok(UserInfo::new(username))
}

// 客户端
let user: UserInfo = client.request("user.login", "alice").await?;
```

### 2. Event

单向消息通知,发送方不等待响应:

```rust
// 发送方
client.emit("user.logged_out", user_id).await?;

// 接收方
#[echostream::listener("user.logged_out")]
async fn on_logout(ctx: ClientContext, user_id: u64) {
    println!("用户 {} 已登出", user_id);
}
```

### 3. Stream

双向实时数据传输通道:

```rust
// 发送端
let stream = client.create_stream("audio.stream").await?;
loop {
    let frame = capture_audio().await;
    stream.send(frame).await?;
}

// 接收端
#[echostream::stream_handler("audio.stream")]
async fn handle_stream(session: Session, stream: StreamReceiver) {
    while let Some(frame) = stream.recv().await {
        play_audio(frame);
    }
}
```

### 4. 时间同步

对于需要时间对齐的流(如音频同步),提供自动时间同步:

```rust
let stream = client.create_stream("audio.sync")
    .with_time_sync()
    .build()
    .await?;

// 发送时自动添加时间戳
stream.send_with_timestamp(audio_data, timestamp).await?;

// 接收时自动对齐到本地时钟
while let Some((data, aligned_time)) = stream.recv_aligned().await {
    schedule_playback(data, aligned_time);
}
```

### 5. Context 和 Session

框架提供不同层级的上下文类型:

#### ServerContext

服务端全局上下文,贯穿整个服务生命周期:

```rust
let server = RpcServer::builder()
    .bind("0.0.0.0:5000")
    .on_start(|ctx: ServerContext| async move {
        // 初始化全局资源
        ctx.set("db", Database::connect().await?);
        Ok(())
    })
    .build()?;
```

#### Session

服务端单个客户端会话上下文,每个连接独立:

```rust
#[echostream::handler("user.login")]
async fn login(session: Session, req: LoginReq) -> Result<LoginResp> {
    // session 包含当前客户端的连接信息
    println!("客户端地址: {}", session.peer_addr());

    // 从 session 访问服务端全局上下文
    let db = session.server_ctx().get::<Database>("db")?;

    // 存储会话级数据
    session.set("user_id", req.user_id);

    // 向该客户端发送消息
    session.emit("welcome", "欢迎登录").await?;

    Ok(LoginResp { token: "xxx".into() })
}
```

#### ClientContext

客户端全局上下文,贯穿整个客户端生命周期:

```rust
#[echostream::listener("server.notify")]
async fn on_notify(ctx: ClientContext, msg: String) {
    println!("收到服务端通知: {}", msg);
}
```

### 6. 生命周期 Hook

框架提供多层级的生命周期 hook:

#### Server 生命周期

```rust
let server = RpcServer::builder()
    .bind("0.0.0.0:5000")
    .on_start(|ctx: ServerContext| async move {
        // 服务启动时执行
        println!("服务启动,初始化资源...");
        ctx.set("db", Database::connect().await?);
        Ok(())
    })
    .on_shutdown(|ctx: ServerContext| async move {
        // 服务关闭时执行
        println!("服务关闭,清理资源...");
        if let Some(db) = ctx.get::<Database>("db") {
            db.close().await?;
        }
        Ok(())
    })
    .on_connect(|session: Session| async move {
        // 客户端连接时执行
        println!("客户端 {} 已连接", session.peer_addr());
        session.set("connect_time", Instant::now());
        Ok(())
    })
    .on_disconnect(|session: Session| async move {
        // 客户端断开时执行
        let duration = session.get::<Instant>("connect_time")
            .map(|t| t.elapsed())
            .unwrap_or_default();
        println!("客户端 {} 断开,连接时长: {:?}", session.peer_addr(), duration);
        Ok(())
    })
    .build()?;
```

#### Client 生命周期

```rust
let client = RpcClient::builder()
    .connect("127.0.0.1:5000")
    .on_connect(|ctx: ClientContext| async move {
        // 连接建立时执行
        println!("已连接到服务器");
        Ok(())
    })
    .on_disconnect(|ctx: ClientContext| async move {
        // 连接断开时执行
        println!("与服务器断开连接");
        Ok(())
    })
    .build()
    .await?;
```

### 7. 插件系统

插件系统分为 `ServerPlugin` 和 `ClientPlugin`,通过直接操作 builder 实现配置和注册。

#### 插件 Trait 定义

```rust
// Server 插件
pub trait ServerPlugin {
    fn name(&self) -> &str;
    fn install(self, builder: ServerBuilder) -> Result<ServerBuilder>;
}

// Client 插件
pub trait ClientPlugin {
    fn name(&self) -> &str;
    fn install(self, builder: ClientBuilder) -> Result<ClientBuilder>;
}
```

#### ServerPlugin 示例

```rust
use echostream::prelude::*;

struct AuthPlugin {
    secret_key: String,
}

impl ServerPlugin for AuthPlugin {
    fn name(&self) -> &str {
        "auth"
    }

    fn install(self, builder: ServerBuilder) -> Result<ServerBuilder> {
        // 直接操作 builder,注册 handler 和 hook
        Ok(builder
            .set("auth.secret", self.secret_key.clone())
            .handler("auth.login", handle_login)
            .listener("auth.logout", on_logout)
            .on_connect(|session| async move {
                println!("新连接,准备认证: {}", session.peer_addr());
                session.set("authenticated", false);
                Ok(())
            })
            .on_disconnect(|session| async move {
                if let Some(user_id) = session.get::<u64>("user_id") {
                    println!("用户 {} 断开连接", user_id);
                }
                Ok(())
            }))
    }
}

// Handler 实现
async fn handle_login(session: Session, req: LoginReq) -> Result<LoginResp> {
    let secret = session.server_ctx().get::<String>("auth.secret")?;

    // 验证逻辑
    if verify_password(&req.password, &secret) {
        session.set("authenticated", true);
        session.set("user_id", req.user_id);
        Ok(LoginResp {
            success: true,
            token: generate_token(req.user_id)
        })
    } else {
        Ok(LoginResp { success: false, token: String::new() })
    }
}

async fn on_logout(session: Session, _: ()) {
    session.set("authenticated", false);
    println!("用户已登出");
}
```

#### ClientPlugin 示例

```rust
struct AuthPlugin {
    secret_key: String,
}

impl ClientPlugin for AuthPlugin {
    fn name(&self) -> &str {
        "auth"
    }

    fn install(self, builder: ClientBuilder) -> Result<ClientBuilder> {
        Ok(builder
            .set("auth.secret", self.secret_key)
            .handler("auth.challenge", handle_challenge)
            .on_connect(|ctx| async move {
                println!("已连接,准备认证");
                // 自动发起登录
                let req = LoginReq {
                    user_id: 1,
                    password: "xxx".into()
                };
                ctx.request("auth.login", req).await?;
                Ok(())
            }))
    }
}

async fn handle_challenge(ctx: ClientContext, challenge: String) -> Result<String> {
    let secret = ctx.get::<String>("auth.secret")?;
    Ok(compute_response(&challenge, &secret))
}
```

#### 使用插件

```rust
// Server 端
let server = RpcServer::builder()
    .bind("0.0.0.0:5000")
    .plugin(AuthPlugin {
        secret_key: "my-secret".into(),
    })
    .plugin(LoggingPlugin::default())
    .build()?;

// Client 端
let client = RpcClient::builder()
    .connect("127.0.0.1:5000")
    .plugin(AuthPlugin {
        secret_key: "my-secret".into(),
    })
    .build()
    .await?;
```