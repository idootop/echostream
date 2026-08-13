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

### ✅ P1（2026-08）简化抽象落地

- [x] derive 宏：`#[rpc]` / `#[event]` / `#[stream]`（强类型 Handler 自动生成）
  - 支持 Session 可选、数据参数可选、`Result<T>` 或直接返回值
  - 宏生成 PascalCase 零大小结构体（`add` → `Add`），注册时 `add_rpc(Add)`
- [x] 生命周期钩子：`on_start` / `on_stop` / `on_connect` / `on_disconnect`
- [x] 双向主动调用（`bidi` 示例）：服务端通过 `session.request()` 主动调用客户端 RPC
- [x] 客户端事件监听：`ClientBuilder::add_event`
- [x] 中间件（数据面）：`Middleware` 洋葱链拦截/修改，`LogMiddleware` 示例
- [x] 插件机制（控制面）：`ServerPlugin::install` 打包处理器与钩子

### ⏳ P2 v0.1 功能（进行中）

- [x] 服务发现（mDNS）：`advertise` / `discover` / `discover_stream`，`discovery` 示例跑通
- [x] 请求超时配置：`ClientBuilder::timeout` + `Session::request_with_timeout`
- [x] 优雅关闭：`Server::shutdown()` 停止接受 + 触发 `on_stop`
- [x] 文档收敛：README 重写为可用示例，删除过时的 API_DESIGN/core.md，MODULE_RESPONSIBILITIES 对齐当前实现
- [x] CI：GitHub Actions（fmt + clippy -D warnings + build）
- [ ] 重连与断线自动重连
- [ ] 客户端钩子（on_connect/on_disconnect）
- [ ] 示例补齐：广播、认证中间件
- [ ] 发布 v0.1（crates.io）

### ✅ P3 多端支持（核心部分完成）

- [x] Web：WebTransport 服务端（`echostream-web`，复用 core Router/Session/Handler）
- [x] Web：浏览器 SDK（WebTransport 网络层 + **Rust 编译 WASM 编解码**，单一事实来源）
- [x] Node.js：完整 client + server 绑定（napi-rs），端到端测试通过
- [x] Python：完整 client + server 绑定（PyO3，同步 API + GIL 释放），端到端测试通过
- [x] Web：客户端核心逻辑 WASM 化 —— 新增 `echostream-client-core`（无 I/O 状态机：
      RPC id 匹配、事件路由、服务端主动调用、流序号管理），编译进 WASM，
      JS SDK 只剩网络层（读帧喂状态机、写帧出状态机），与 Rust 原生客户端共享同一份逻辑
- [x] 状态机交叉验证：RPC 响应匹配（错误 id 忽略）、事件路由、主动调用、流序号全过

## 设计决策记录

- 删除了原设计中的三层 trait 继承树（BaseContext/BaseSession/Platform）：OO 式抽象在 Rust 中退化为此路不通的 `Arc<dyn>` + 类型擦除，改为具体类型 + 强类型 Handler
- 序列化由框架统一处理（`handle_encoded` 入口），业务 Handler 只面对具体类型
- 错误转换在 transport 层用本地 trait（`ToEcho`）完成，避免孤儿规则
- 事件/流走单向流，RPC 走双向流；流首帧由分派层读出后缓存进 `StreamReceiver`
