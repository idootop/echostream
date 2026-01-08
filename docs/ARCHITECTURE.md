# EchoStream 架构设计

## 概述

EchoStream 是一个基于 QUIC 协议的高性能异步双向 RPC 框架，专为实时通信场景设计。它提供了统一的接口来处理请求/响应、事件通知和实时数据流传输。

## 核心特性

### 1. 多模态通信
- **Request/Response**: 标准的请求-响应模式，支持超时控制
- **Event**: 单向事件通知，无需响应
- **Stream**: 双向流式传输，支持音视频等实时数据

### 2. 时间同步
- 基于 NTP 风格的时钟同步协议
- 自动补偿网络延迟和时钟漂移
- 支持流数据的时间戳对齐

### 3. 服务发现
- 基于 mDNS 的零配置局域网服务发现
- 支持手动指定连接地址
- 动态服务注册和注销

## 技术栈

### 传输层
- **quinn**: QUIC 协议实现，提供多路复用和内置加密
- **rustls**: TLS 1.3 加密支持
- **rcgen**: 证书生成（用于开发和测试）

### 编解码
- **bincode**: 高性能二进制序列化
- **serde**: 序列化/反序列化框架
- **bytes**: 高效的字节操作

### 服务发现
- **mdns-sd**: mDNS 服务发现和广播
- **dns-sd**: DNS-SD 服务类型定义

### 异步运行时
- **tokio**: 异步运行时，提供定时器、通道等基础设施
- **futures**: 异步编程抽象

### 开发者体验
- **echostream-derive**: 过程宏，简化服务定义和处理器注册
- **tracing**: 结构化日志和诊断

## 系统架构

```
┌─────────────────────────────────────────────────────────┐
│                    Application Layer                    │
│  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐  │
│  │ Handlers │  │ Services │  │ Stream Processors    │  │
│  └────┬─────┘  └────┬─────┘  └──────────┬───────────┘  │
└───────┼─────────────┼────────────────────┼──────────────┘
        │             │                    │
┌───────┼─────────────┼────────────────────┼──────────────┐
│       │             │   EchoStream Core  │              │
│  ┌────▼─────────────▼────────────────────▼───────────┐  │
│  │            RPC Framework (API Layer)              │  │
│  │  ┌──────────┐  ┌──────────┐  ┌─────────────────┐ │  │
│  │  │ Request  │  │  Event   │  │  Stream Manager │ │  │
│  │  │ Handler  │  │ Emitter  │  │   & Sync        │ │  │
│  │  └──────────┘  └──────────┘  └─────────────────┘ │  │
│  └───────────────────────┬──────────────────────────┘  │
│  ┌───────────────────────▼──────────────────────────┐  │
│  │         Protocol Layer (Frame & Codec)           │  │
│  │  ┌──────────┐  ┌──────────┐  ┌────────────────┐ │  │
│  │  │  Frame   │  │  Codec   │  │  Time Protocol │ │  │
│  │  │  Parser  │  │ (bincode)│  │     (NTP-like) │ │  │
│  │  └──────────┘  └──────────┘  └────────────────┘ │  │
│  └───────────────────────┬──────────────────────────┘  │
│  ┌───────────────────────▼──────────────────────────┐  │
│  │          Transport Layer (QUIC)                  │  │
│  │  ┌─────────────────┐  ┌──────────────────────┐  │  │
│  │  │ Connection Pool │  │  Stream Multiplexer  │  │  │
│  │  └─────────────────┘  └──────────────────────┘  │  │
│  └───────────────────────┬──────────────────────────┘  │
└────────────────────────────┼────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────┐
│                  Discovery Layer (Optional)             │
│  ┌──────────────────┐  ┌───────────────────────────┐   │
│  │  mDNS Advertiser │  │  mDNS Browser             │   │
│  │  (Server)        │  │  (Client)                 │   │
│  └──────────────────┘  └───────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

## 模块设计

### 1. echostream-core (核心模块)

#### 1.1 连接管理 (connection)
- `ConnectionManager`: 管理 QUIC 连接池
- `Endpoint`: 封装 quinn 的端点，支持客户端和服务器模式
- `Connection`: 单个连接的抽象

#### 1.2 协议层 (protocol)
- `Frame`: 定义帧类型（Request, Response, Event, Stream, TimeSync）
- `Codec`: 序列化/反序列化
- `MessageId`: 消息标识符生成和管理

#### 1.3 RPC 框架 (rpc)
- `RpcServer`: 服务端 RPC 管理器
- `RpcClient`: 客户端 RPC 调用器
- `Router`: 路由表，管理命令到处理器的映射
- `Context`: 请求上下文，包含连接信息和元数据

#### 1.4 流管理 (stream)
- `StreamManager`: 管理所有流的生命周期
- `StreamSender`/`StreamReceiver`: 流的发送和接收端
- `TimeSync`: 时间同步协议实现
- `JitterBuffer`: 抖动缓冲区

### 2. echostream-discovery (服务发现模块)
- `ServiceAdvertiser`: mDNS 服务广播
- `ServiceBrowser`: mDNS 服务发现
- `ServiceInfo`: 服务元信息

### 3. echostream-derive (过程宏模块)
- `#[echostream::handler]`: 自动注册请求处理器
- `#[echostream::service]`: 定义服务接口
- `#[echostream::event]`: 定义事件类型

### 4. echostream-types (公共类型模块)
- `Error`: 统一的错误类型
- `Result`: 统一的结果类型
- `Config`: 配置结构

## 通信流程

### Request/Response 流程
```
Client                     Server
  │                          │
  ├──── Request Frame ──────▶│
  │    (msg_id, cmd, data)   │
  │                          ├─ Route to Handler
  │                          ├─ Process
  │◀──── Response Frame ─────┤
  │    (msg_id, result)      │
  │                          │
```

### Event 流程
```
Sender                     Receiver
  │                          │
  ├──── Event Frame ────────▶│
  │    (event_type, data)    │
  │                          ├─ Trigger Listeners
  │                          │
```

### Stream 流程
```
Initiator                  Responder
  │                          │
  ├──── Stream Init ────────▶│
  │    (stream_id, meta)     │
  │◀──── Stream Accept ──────┤
  │                          │
  ├──── Time Sync Req ──────▶│
  │◀──── Time Sync Resp ─────┤
  │    (calculate offset)    │
  │                          │
  ├──── Data Frame 1 ───────▶│
  ├──── Data Frame 2 ───────▶│
  │    (with timestamp)      ├─ Jitter Buffer
  │                          ├─ Time Alignment
  │                          ├─ Deliver to App
  │                          │
```

## 时间同步协议

采用简化的 NTP 算法：

```
Client                     Server
  │                          │
  │ T1: 发送时间             │
  ├──── TimeSync Req ───────▶│
  │                          │ T2: 接收时间
  │                          │ T3: 发送时间
  │◀──── TimeSync Resp ──────┤
  │ T4: 接收时间             │
  │                          │

延迟 = ((T4 - T1) - (T3 - T2)) / 2
偏移 = ((T2 - T1) + (T3 - T4)) / 2
```

## 安全性

### 传输安全
- 基于 QUIC 的 TLS 1.3 加密
- 支持自签名证书（开发）和 CA 证书（生产）
- 可选的客户端证书验证

### 应用安全
- 支持自定义认证中间件
- 连接级和消息级的访问控制
- 速率限制和防 DDoS

## 性能优化

### 零拷贝
- 使用 `bytes::Bytes` 进行引用计数的零拷贝传输
- 避免不必要的序列化/反序列化

### 连接复用
- 单个 QUIC 连接支持多个并发流
- 避免 TCP 的 Head-of-Line Blocking

### 背压控制
- 基于 tokio 通道的自然背压
- 流控制防止内存溢出

## 错误处理

### 错误分类
- `TransportError`: 传输层错误（连接中断、超时等）
- `ProtocolError`: 协议错误（帧解析失败、版本不匹配等）
- `ApplicationError`: 应用层错误（处理器错误、业务逻辑错误等）

### 重连策略
- 指数退避重连
- 最大重试次数限制
- 连接状态通知

## 可扩展性

### 自定义编解码器
- 支持替换默认的 bincode 编解码器
- 插件式架构

### 中间件支持
- 请求/响应中间件链
- 支持认证、日志、追踪等横切关注点

### 传输层抽象
- 虽然当前基于 QUIC，但保持传输层抽象
- 未来可支持其他传输协议

## 开发者体验

### 声明式 API
```rust
#[echostream::service]
trait AudioService {
    async fn play(&self, file: String) -> Result<()>;
    async fn stop(&self) -> Result<()>;

    #[stream]
    async fn stream_audio(&self) -> AudioStream;
}
```

### 最小化样板代码
```rust
#[echostream::handler("audio.play")]
async fn handle_play(ctx: Context, file: String) -> Result<()> {
    // 业务逻辑
    Ok(())
}
```

### 丰富的诊断信息
- 基于 `tracing` 的结构化日志
- 连接和流的状态监控
- 性能指标收集

## 部署模式

### 点对点 (P2P)
- 客户端和服务端都运行 EchoStream
- 通过 mDNS 自动发现
- 适用于局域网场景

### 客户端-服务器 (C/S)
- 服务端监听固定地址
- 客户端手动指定服务端地址
- 适用于公网场景

### 混合模式
- 支持同时作为客户端和服务端
- 动态角色切换
- 适用于分布式场景

## 测试策略

### 单元测试
- 每个模块的独立测试
- 模拟网络条件

### 集成测试
- 端到端的通信测试
- 多客户端并发测试

### 压力测试
- 高并发连接测试
- 大数据量传输测试
- 长时间运行稳定性测试

## 文档规划

### 用户文档
- 快速开始指南
- API 参考
- 示例代码

### 开发者文档
- 架构设计文档（本文档）
- 协议规范
- 贡献指南

## 未来展望

### 短期（v0.1 - v0.3）
- 完成核心 RPC 功能
- 实现基本的流传输
- mDNS 服务发现

### 中期（v0.4 - v0.6）
- 完善时间同步协议
- 优化性能
- 增加中间件支持

### 长期（v1.0+）
- 跨语言支持（通过 FFI）
- 更多传输协议支持
- 分布式追踪集成
- 云原生特性（服务网格集成）
