# EchoStream 开发进展

> 定期更新的项目进展记录。目标：生产正式版本，支持 Rust / Node / Python / Web 四端。

## 总览

- **技术栈**：Rust 2024 + QUIC（quinn 0.11）+ postcard 序列化
- **架构**：proto（协议类型）→ transport（QUIC 封装）→ core（框架）→ derive/discovery（可选）→ echostream（统一入口）
- **通信模型**：每条消息（RPC/事件/流）使用独立 QUIC 流，天然多路复用、背压隔离

## 里程碑

### ✅ P0（2026-08）修复构建 + 垂直切片跑通

- 修复 workspace 构建：`async_trait` 依赖名不一致导致全 workspace 无法编译
- 精简 `echostream-proto`：删除过度抽象的 trait 层（Context/Session/Platform/DynamicMap/Listenable 等），只保留协议类型（`Message`/`Error`/`Timestamp`/`StatusCode`），保持零运行时依赖
- 实现 `echostream-transport`：quinn 封装（自签证书自动生成、客户端跳过验证、双向流/单向流/数据报、帧编解码：长度前缀 + postcard）
- 实现 `echostream-core` 垂直切片：
  - `Server`/`Client`（Builder 模式，开箱即用）
  - `Session`（双向主动通信：request/emit/create_stream + 会话级状态）
  - `Router`（RPC/Event/Stream 处理器注册与分发）
  - 强类型 Handler（`RpcHandler`/`EventHandler`/`StreamHandler`），框架层统一编解码
  - `ServerContext`（全局状态 + 会话管理 + 广播）
  - `StreamSender`/`StreamReceiver`（帧级收发，自动序号 + 时间戳）
- 端到端示例 `simple_rpc` 跑通：RPC（`add(10,20)=30`）、Event、Stream 三种模式验证通过
- 修复：流首帧在分派时丢失的 bug（`StreamReceiver` 缓存首帧）
- workspace 全量编译通过，clippy 零警告

### ⏳ P1 简化抽象落地（进行中）

- [x] derive 宏：`#[rpc]` / `#[event]` / `#[stream]`（强类型 Handler 自动生成，消除手动 trait 样板）
  - 支持 Session 可选、数据参数可选、`Result<T>` 或直接返回值
  - 宏生成 PascalCase 零大小结构体（`add` → `Add`），注册时 `add_rpc(Add)`
- [x] 生命周期钩子：`on_start` / `on_stop` / `on_connect` / `on_disconnect`（服务端）
- [x] 双向主动调用示例（`bidi`）：服务端通过 `session.request()` 主动调用客户端 RPC
- [x] 客户端事件监听：`ClientBuilder::add_event` 接收服务端推送
- [x] 中间件（数据面）：`Middleware` trait，洋葱链拦截/修改入站消息，`LogMiddleware` 示例
- [x] 插件机制（控制面）：`ServerPlugin` trait，`install(Box<Self>, ServerBuilder)` 打包处理器与钩子
- [ ] 客户端钩子（on_connect/on_disconnect）
- [ ] 请求超时可配置
- [ ] 服务端事件广播到单会话/全体的完整示例

### ⏳ P2 v0.1 功能

- [ ] 服务发现（mDNS）接入
- [ ] 连接生命周期与优雅关闭
- [ ] 请求超时配置、重连
- [ ] 文档收敛（以 MODULE_RESPONSIBILITIES 为准）
- [ ] CI（cargo check + clippy）
- [ ] 发布 v0.1

### ⏳ P3 多端支持

- [ ] Web：WebTransport 端点（浏览器原生直连）
- [ ] Node.js：Rust binding（napi-rs）
- [ ] Python：Rust binding（PyO3）
- [ ] Web：WASM 客户端

## 设计决策记录

- 删除了原设计中的三层 trait 继承树（BaseContext/BaseSession/Platform）：OO 式抽象在 Rust 中退化为此路不通的 `Arc<dyn>` + 类型擦除，改为具体类型 + 强类型 Handler
- 序列化由框架统一处理（`handle_encoded` 入口），业务 Handler 只面对具体类型
- 错误转换在 transport 层用本地 trait（`ToEcho`）完成，避免孤儿规则
- 事件/流走单向流，RPC 走双向流；流首帧由分派层读出后缓存进 `StreamReceiver`
