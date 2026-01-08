# EchoStream

一个基于 QUIC 的高性能异步双向 RPC 和流传输框架。

## 项目简介

EchoStream 是专为实时通信场景设计的 Rust RPC 框架，它融合了传统 RPC 的便利性和实时流传输的能力。通过 QUIC 协议的多路复用特性，EchoStream 能够在单个连接上同时处理控制信令和大量实时数据流，避免了传统 TCP 的队头阻塞问题。

### 核心特性

- **🔄 异步双向通信**: 客户端和服务端都可以主动发起请求、发送事件和推送流数据
- **📡 多模态信令**: 支持 Request/Response、Event 和 Stream 三种通信模式
- **⏱ 时间同步**: 内置类 NTP 时钟同步协议，确保分布式节点间的时间对齐
- **🎵 流式传输**: 支持音视频等实时数据的低延迟传输，配备抖动缓冲和时间戳对齐
- **🚀 基于 QUIC**: 利用 QUIC 的 0-RTT 握手、多路复用和自动拥塞控制
- **🔍 服务发现**: 基于 mDNS 的零配置局域网服务发现（可选）
- **🛡 安全传输**: 内置 TLS 1.3 加密，支持自签名和 CA 证书
- **🦀 开发友好**: 提供声明式 API 和过程宏，最小化样板代码

## 使用场景

EchoStream 特别适用于需要同时处理控制指令和实时数据的场景：

- **实时音视频通信**: 低延迟音视频传输，支持多路复用和时间同步
- **物联网设备控制**: 命令下发、状态上报和数据流采集
- **游戏网络**: 游戏状态同步、事件广播和语音通信
- **远程桌面**: 屏幕共享、输入控制和音频转发
- **分布式系统**: 节点间通信、数据同步和事件总线

## 项目架构

EchoStream 采用分层架构设计：

```
Application Layer    ← 用户代码（Handlers, Services, Streams）
       ↓
API Layer            ← RPC 框架（Request, Event, Stream Manager）
       ↓
Protocol Layer       ← 帧定义、编解码、时间同步协议
       ↓
Transport Layer      ← QUIC 连接管理和多路复用
       ↓
Discovery Layer      ← mDNS 服务发现（可选）
```


### 核心依赖

- **quinn**: QUIC 协议实现
- **tokio**: 异步运行时
- **serde** + **bincode**: 序列化/反序列化
- **bytes**: 零拷贝字节操作
- **mdns-sd**: mDNS 服务发现
- **tracing**: 结构化日志

### 项目结构

```
echostream/
├── echostream-core/        # 核心框架
│   ├── connection/         # QUIC 连接管理
│   ├── protocol/           # 协议定义和编解码
│   ├── rpc/                # RPC 框架
│   └── stream/             # 流管理和时间同步
├── echostream-discovery/   # 服务发现
├── echostream-derive/      # 过程宏
├── echostream-types/       # 公共类型
└── examples/               # 示例代码
```

## 快速开始

> **⚠️ 开发中**: EchoStream 正在积极开发中，API 可能会发生变化。

### 安装

```toml
[dependencies]
echostream = "0.1"
```

### 服务端示例

```rust
use echostream::prelude::*;

#[echostream::handler("audio.play")]
async fn handle_play(ctx: Context, file: String) -> Result<()> {
    println!("播放音频文件: {}", file);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = RpcServer::builder()
        .bind("0.0.0.0:5000")
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
    let client = RpcClient::connect("127.0.0.1:5000").await?;

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

### 服务发现示例

```rust
use echostream::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // 服务端自动广播
    let server = RpcServer::builder()
        .bind("0.0.0.0:5000")
        .enable_discovery("AudioService")
        .build()?;

    // 客户端自动发现
    let client = RpcClient::discover("AudioService").await?;

    client.request("audio.play", "music.mp3").await?;

    Ok(())
}
```

## 核心概念

### 1. Request/Response（请求/响应）

标准的 RPC 调用模式，客户端发送请求，等待服务端响应：

```rust
// 服务端
#[echostream::handler("user.login")]
async fn login(ctx: Context, username: String) -> Result<Session> {
    // 验证用户并返回会话
    Ok(Session::new(username))
}

// 客户端
let session: Session = client.request("user.login", "alice").await?;
```

### 2. Event（事件）

单向的消息通知，发送方不等待响应：

```rust
// 发送方
client.emit("user.logged_out", user_id).await?;

// 接收方
#[echostream::listener("user.logged_out")]
async fn on_logout(ctx: Context, user_id: u64) {
    println!("用户 {} 已登出", user_id);
}
```

### 3. Stream（流）

双向的实时数据传输通道，支持大量数据的持续传输：

```rust
// 发送端
let stream = client.create_stream("audio.stream").await?;
loop {
    let audio_frame = capture_audio().await;
    stream.send(audio_frame).await?;
}

// 接收端
#[echostream::stream_handler("audio.stream")]
async fn handle_audio_stream(ctx: Context, stream: StreamReceiver) {
    while let Some(frame) = stream.recv().await {
        play_audio(frame);
    }
}
```

### 4. 时间同步

对于需要时间对齐的流（如音频同步），EchoStream 提供自动时间同步：

```rust
let stream = client.create_stream("audio.sync_stream")
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

