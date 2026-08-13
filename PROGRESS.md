# EchoStream 开发进展

> 定期更新的项目进展记录。目标：生产正式版本，支持 Rust / Node / Python / Web 四端。

## 总览

- **技术栈**：Rust 2024 + QUIC（quinn 0.11）+ postcard 序列化
- **架构**：proto（传输接口 + 协议类型）→ transport（QUIC）→ core（框架）→ ws/web（局域网传输）→ derive/discovery/middleware/plugin（可选扩展）→ echostream（统一入口）
- **通信模型**：RPC 每请求一条双向流（HTTP/3 语义）；事件走复用长连接单向流；不可靠事件走数据报；流走单向流
- **多端**：Rust 原生 + Node（napi-rs）+ Python（PyO3）+ Web（WASM 状态机 + WebSocket/WebTransport）

## 里程碑

### ✅ P0（2026-08）修复构建 + 垂直切片跑通

- 修复 workspace 构建；精简 `echostream-proto` 为协议类型 + 传输接口（零运行时依赖）
- 实现 `echostream-transport`（quinn 封装：自签证书、双向/单向流/数据报、帧编解码）
- 实现 `echostream-core` 垂直切片（Server/Client/Session/Router/Handler/StreamSender）
- 端到端示例 `simple_rpc` 跑通：RPC / Event / Stream 三种模式

### ✅ P1（2026-08）简化抽象落地

- [x] derive 宏：`#[rpc]` / `#[event]` / `#[stream]`（强类型 Handler 自动生成）
- [x] 生命周期钩子：`on_start` / `on_stop` / `on_connect` / `on_disconnect`
- [x] 双向主动调用（`bidi` 示例）；客户端事件监听
- [x] 中间件（数据面）：`Middleware` 洋葱链 + 拦截
- [x] 插件机制（控制面）：`ServerPlugin` / `ClientPlugin`

### ✅ P2 v0.1 功能

- [x] 服务发现（mDNS）：`advertise` / `discover` / `discover_stream`
- [x] 请求超时配置；优雅关闭 `Server::shutdown()`
- [x] 传输无关重构：`Endpoint`/`FrameIo`/`Listener` 接口入 proto，core 通过 `listener()`/`from_endpoint()` 注入任意传输（QUIC/WS/WebTransport 同线缆格式）
- [x] 事件通道复用：事件走长连接单向流批量帧（可靠事件 ~110k evt/s）
- [x] 不可靠事件：数据报通道（~573k evt/s，5.7x 于可靠通道）
- [x] 基准测试：RPC 延迟 ~55µs / 并发 ~65k req/s / 流 ~150 MiB/s（docs/BENCHMARK.md）
- [x] Web 局域网方案：WebSocket（ws:// 零证书）走通浏览器 E2E；证书调研结论归档 docs/WEB_E2E.md
- [x] CI 三 job 矩阵（Rust / WASM / Node E2E）；发布手册 RELEASE.md
- [x] 插件与中间件落地（本次）：
  - `echostream-plugin-auth`：连接 token 认证（认证事件 + 中间件拦截 + 轮询等待竞态防护）
  - `echostream-plugin-reconnect`：断线自动重连（指数退避，主动关闭不重连）
  - `echostream-middleware-logging`：结构化消息日志
  - `plugin_stack` 示例：三件套同用（拦截 → 认证 → 重连 → 重新认证全链路验证）
- [x] 服务发现通用化：`ServiceInfo{name, addr, metadata}` + `advertise_with` 携带 TXT 元数据
- [ ] 正式发布 v0.1（需 crates.io / npm / PyPI 凭据，按 RELEASE.md 执行）
- [ ] 性能调优（可选：多流并行/连接池/零拷贝）

### ✅ P3 多端支持

- [x] Web：WebTransport 服务端（`echostream-web`）+ WebSocket 服务端（`echostream-ws`）
- [x] Web：浏览器 SDK（双传输 + Rust WASM 编解码状态机，单一事实来源）
- [x] Node.js：完整 client + server 绑定（napi-rs），E2E 测试通过
- [x] Python：完整 client + server 绑定（PyO3），E2E 测试通过
- [x] 客户端核心 WASM 化（`echostream-client-core` 无 I/O 状态机，与 Rust 共享逻辑）

## 设计决策记录

- 传输接口（Endpoint/FrameIo/FrameRead/FrameWrite/Listener）放 proto 层，避免 core↔transport 依赖环
- 中间件拦截 RPC 时回 FORBIDDEN 错误响应（不静默关闭流，避免客户端误判断线）
- 服务端 accept 后 spawn 并行处理流（RPC 不阻塞 accept 循环，避免事件饥饿）
- 客户端主动关闭（`close()`）后断开回调不再触发（`is_closed`），重连插件据此停止
- 认证竞态：认证事件与业务请求在不同任务并发处理，中间件轮询等待认证状态生效
