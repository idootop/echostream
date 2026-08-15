# EchoStream 开发进展

> 定期更新的项目进展记录。目标：生产正式版本，支持 Rust / Node / Python / Web 四端。

## 总览

- **技术栈**：Rust 2024 + QUIC（quinn 0.11）+ postcard 序列化
- **架构**：proto（协议 + 传输接口 + 动态值编解码）→ core（框架，传输无关）→ transport（quic/ws/web 三 feature）→ 扩展与绑定
- **通信模型**：RPC 复用通道（默认，按 id 多路复用）+ 大载荷独立流；事件复用通道 / 数据报；流独立单向流
- **多端**：Rust 原生 + Node（napi-rs，ESM）+ Python（PyO3）+ Web（WASM 状态机 + WebSocket/WebTransport），
  各端自动编解码（i64 ZigZag 统一约定）

## 里程碑

### ✅ P0-P2（2026-08）基础能力

- 协议层 echostream-proto：Message/Error/传输接口/帧编解码（零运行时依赖）
- 框架层 echostream-core：Server/Client/Session/Router/Handler/中间件/插件
- derive 宏 #[rpc] / #[event] / #[stream]；生命周期钩子；双向主动调用
- mDNS 服务发现；请求超时；优雅关闭；事件通道复用 + 数据报
- 基准测试与负载矩阵（docs/BENCHMARK.md）
- Web 局域网方案（WebSocket 零证书）+ WebTransport 服务端；浏览器 E2E 调研结论归档 docs/WEB_E2E.md
- CI 矩阵；发布手册 RELEASE.md
- 插件与中间件：auth / reconnect / retry / logging

### ✅ P3 多端支持

- Web：浏览器 SDK（双传输 + WASM 状态机，单一事实来源）
- Node.js：完整 client + server 绑定（napi-rs），E2E 通过
- Python：完整 client + server 绑定（PyO3），E2E 通过
- 客户端核心 WASM 化（无 I/O 状态机）

### ✅ P4 重构与 DX 打磨（2026-08）

- [x] 自动编解码 DX：proto::dynamic 定义跨语言载荷约定（i64 ZigZag 等），四端实现字节级一致：
      Rust derive 宏、WASM encode/decode_payload（+schema）、Node 纯 JS postcard.js、Python postcard.py
- [x] 浏览器 SDK 全链路 DX：request("add", 10, 20) → 30；事件/流/双向 RPC/入站流自动编解码
- [x] Node 绑定 DX + ESM：addRpc("add", async (a, b) => a + b)；包与测试全部 ESM
- [x] Python 绑定 DX：echostream 包包装层（_native + postcard.py）
- [x] 流收发强类型化：StreamSender::send<T> / StreamReceiver::recv<T>
- [x] 服务端广播修复（broadcast_raw 防二次编码）；服务发现 local_ipv4 跳过代理 fake-IP 段
- [x] 清理遗留物：删除 history/、tools/e2e/（跨端矩阵迁入 scripts/cross_e2e.mjs，6 组合全绿）

### ✅ P5 性能优化（2026-08）

- [x] RPC 复用通道（默认）：并发吞吐 80k → 127k req/s（+58%），顺序延迟 50 → 43µs（-15%）
- [x] 连接池：ClientBuilder::pool(n) —— 4 连接并发吞吐 148k req/s（+85%）
- [x] 传输窗口调优：16MiB/8MiB 窗口 + 4096 并发流 —— 并发 RPC +30%、事件 +30%

### ✅ P6 分包重构 v4（2026-08，本次）

- [x] **echostream-transport 独立包**：quic（默认）/ ws / web 三 feature，QUIC 便捷 bind/connect 扩展 trait；
      ws/web 服务端并入（原 echostream-ws / echostream-web 删除），examples 随包
- [x] **core 传输无关**：移除内置 quic 模块与 bind/connect，新增 listener_factory 延迟注入；
      ServerBuilder::listener / ClientBuilder::from_endpoint 注入任意传输
- [x] **client-core 并入 core**：ClientCore 无 I/O 状态机进 core（io feature 门控 tokio 模块，
      default-features=false 可编译 WASM）；原 echostream-client-core 删除
- [x] **discovery DX 对齐 README**：ServiceInfo builder（new/set_property/get_property/address）+
      Discovery 门面（advertise/discover/discover_stream）
- [x] 文档体系重构：docs/ARCHITECTURE.md（模块职责）+ docs/EXAMPLES.md（全仓示例导航，根 examples/ 并入）；
      主 README 系统性重写（文档索引 + 最新 Discovery API）；全部 README 代码块改为正式语言围栏格式；
      plugins/middlewares README 同步实际插件列表
- [x] 修复：客户端接收循环 RPC 复用通道未 spawn 导致阻塞事件/流接收的严重 bug
      （服务端主动 RPC 后广播/事件丢失）
- [ ] 发布 v0.1（需 crates.io / npm / PyPI 凭据，按 RELEASE.md 执行）

### 🔄 P7 API 体系化完善（2026-08，进行中）

目标：接口完备、一致、稳定，达到生产可发布标准。逐步提交，每步留痕。

- [x] **Step 1 客户端生命周期**：Client 补齐 on_connect 回调（与 on_disconnect 对称）、
      is_connected() 实时连接状态、回调按 HookId 注册/取消注册（add_on_connect/remove_on_connect 等）；
      连接池辅助连接断开仅静默移除，不再误触发断开回调；plugin_stack 示例改用 on_connect 重连后重新认证
- [x] **Step 2 ClientBuilder build 模式**：endpoint(s) 注入 + build()（未注入连接返回错误），
      transport connect 改为 endpoints().build() 对齐（from_endpoint(s) 保留为便捷方法）
- [x] **Step 3 流增强**：StreamReceiver::into_stream / into_stream_typed（futures::Stream 拉取模式，
      与组合子互通）；修复客户端流分发未 spawn 导致长流阻塞事件/主动 RPC 接收的 bug；
      流消费模式文档化（句柄拉取为主 / 回调推送为绑定侧原语）；单测覆盖（10 passed）
- [x] **Step 4 中间件洋葱重构**：Middleware::handle(session, msg, next) 洋葱链（tower/axum 同款），
      终端在链内执行（handler 错误可被中间件捕获/归一化）；新增 on_connect/on_disconnect 生命周期钩子
      （Server 每会话触发、Client 主连接触发）；logging/auth 中间件与示例全部迁移
- [x] **Step 5 Router 运行时管理**：add_* 返回 Token，remove_rpc/event/stream/middleware 按 token 精确移除；
      rpc_names/event_names/stream_names/middleware_names 注册表查询；Server/Client 暴露 router() + 运行时注册/移除；
      Server 钩子支持运行时注册与取消注册（add/remove_on_*）；router_test 覆盖链序/拦截/改名/生命周期（16 组全绿）
- [ ] **Step 6 新中间件**：timeout / error / transform（规划落地）
- [ ] **Step 7 新插件**：metrics / heartbeat（规划落地）
- [ ] **Step 8 文档同步**：README / ARCHITECTURE / EXAMPLES / plugins / middlewares
- [ ] **Step 9 全量验证**：workspace 构建 + 测试 + 示例 + 跨端矩阵
- [ ] **Step 10 PROGRESS 收尾 + 最终提交**

## 设计决策记录

- 传输接口（Endpoint/FrameIo/FrameRead/FrameWrite/Listener）放 proto 层，避免依赖环
- **core 传输无关 + transport 三 feature**：框架只依赖 proto 接口；QUIC 便捷 API 由 transport
  扩展 trait 提供（ServerBuilderExt::bind / ClientBuilderExt::connect），入口 prelude 重导出
- **client-core 并入 core**：无 I/O 状态机（RPC 匹配/事件路由/流管理）为 WASM 浏览器剥离；
  服务端是 accept→spawn→dispatch 的 I/O 循环，无法无 I/O 化 —— 故只有 client 状态机
- RPC 复用通道：默认高频小请求走通道，大载荷走独立流（>64KiB 阈值）；通道开启标记为保留方法名 $channel；
  客户端/服务端接收循环识别；**通道处理必须 spawn，否则阻塞 accept 主循环**
- 连接池：多 QUIC 连接分摊流控窗口并跨核扩展（quinn 单连接单任务）；重连后收敛为单连接
- 跨端载荷：所有整数走 i64 ZigZag（E5 约定）；BigInt/超 i64 走 u64 普通 varint；
  解码智能推断（字符串优先），歧义场景显式 schema
- 中间件拦截 RPC 回 FORBIDDEN 错误响应；流帧载荷自动编解码（跨端一致）
- 认证竞态：认证事件与业务请求并发处理，中间件轮询等待认证状态生效
