# EchoStream 开发进展

> 定期更新的项目进展记录。目标：生产正式版本，支持 Rust / Node / Python / Web 四端。

## 总览

- **技术栈**：Rust 2024 + QUIC（quinn 0.11）+ postcard 序列化
- **架构**：proto（协议 + 传输接口 + 动态值编解码）→ core（框架 + 内置 QUIC）→ ws/web（其它传输）→ 扩展与绑定
- **通信模型**：RPC 复用通道（默认，按 id 多路复用）+ 大载荷独立流；事件复用通道 / 数据报；流独立单向流
- **多端**：Rust 原生 + Node（napi-rs，ESM）+ Python（PyO3）+ Web（WASM 状态机 + WebSocket/WebTransport），
  各端自动编解码（i64 ZigZag 统一约定）

## 里程碑

### ✅ P0-P2（2026-08）基础能力

- 协议层 `echostream-proto`：Message/Error/传输接口/帧编解码（零运行时依赖）
- 框架层 `echostream-core`：Server/Client/Session/Router/Handler/中间件/插件
- derive 宏 `#[rpc]` / `#[event]` / `#[stream]`；生命周期钩子；双向主动调用
- mDNS 服务发现；请求超时；优雅关闭；事件通道复用 + 数据报
- 基准测试与负载矩阵（docs/BENCHMARK.md）
- Web 局域网方案（WebSocket 零证书）+ WebTransport 服务端；浏览器 E2E 调研结论归档 docs/WEB_E2E.md
- CI 三 job 矩阵；发布手册 RELEASE.md
- 插件与中间件：auth / reconnect / retry / logging

### ✅ P3 多端支持

- Web：浏览器 SDK（双传输 + WASM 状态机，单一事实来源）
- Node.js：完整 client + server 绑定（napi-rs），E2E 通过
- Python：完整 client + server 绑定（PyO3），E2E 通过
- 客户端核心 WASM 化（echostream-client-core 无 I/O 状态机）

### ✅ P4 重构与 DX 打磨（本次）

- [x] **分包重构**：echostream-transport 并入 echostream-core（`quic` 模块，默认开启）；
      QUIC / WebSocket / WebTransport 同级传输实现，传输接口归 proto
- [x] **自动编解码 DX**：`proto::dynamic` 定义跨语言载荷约定（i64 ZigZag 等），四端实现字节级一致：
      Rust derive 宏、WASM `encode_payload`/`decode_payload`（+schema）、Node 纯 JS postcard.js、Python postcard.py
- [x] **浏览器 SDK 全链路 DX**：`request("add", 10, 20)` → 30；事件/流/双向 RPC/入站流自动编解码；
      修复整数编码约定（原 wasm 正数按 u64 直写与 i64 ZigZag 约定不一致的潜在 bug）
- [x] **Node 绑定 DX + ESM**：`addRpc("add", async (a, b) => a + b)` 自动解码参数/编码响应；
      新增客户端 onRpc/onStream；包与测试全部升级 ESM（type: module）
- [x] **Python 绑定 DX**：`echostream` 包包装层（_native + postcard.py），参数自动解码展开、返回值自动编码
- [x] 服务端广播修复：`broadcast_raw` 防二次编码（原 ctx.broadcast 对已编码字节再编码）
- [x] 流收发强类型化：`StreamSender::send<T: Serialize>` / `StreamReceiver::recv<T>` 自动序列化
- [x] 清理遗留物：删除 history/ 早期设计文档与 tools/e2e/（Playwright 死代码与旧矩阵脚本）；
      跨端矩阵迁入 `scripts/cross_e2e.mjs`（6 组合全绿）

### ✅ P5 性能优化（本次）

- [x] **RPC 复用通道**（默认）：一条长连接双向流按请求 id 多路复用 —— 并发吞吐 80k → 127k req/s（+58%），
      顺序延迟 50 → 43µs（-15%）；载荷 >64KiB 自动切独立流（`request_raw_stream`）
- [x] **连接池**：`ClientBuilder::pool(n)` —— 4 连接并发吞吐 148k req/s（+85%）
- [x] **传输窗口调优**：连接/流级接收窗口 16MiB/8MiB、并发流上限 4096 —— 并发 RPC +30%、事件 +30%
- [x] 流吞吐 214 → 238 MiB/s；不可靠事件 673k → 711k evt/s
- [ ] 发布 v0.1（需 crates.io / npm / PyPI 凭据，按 RELEASE.md 执行）

## 设计决策记录

- 传输接口（Endpoint/FrameIo/FrameRead/FrameWrite/Listener）放 proto 层，避免依赖环
- QUIC 为主传输并入 core（默认 feature），ws/web 独立分包（浏览器场景）
- RPC 复用通道：默认高频小请求走通道（吞吐/延迟最优），大载荷走独立流（避免队头阻塞）；
  通道开启标记为保留方法名 `$channel`，服务端/客户端接收循环均识别
- 连接池：多 QUIC 连接分摊流控窗口并跨核扩展（quinn 单连接单任务）；重连后收敛为单连接
- 跨端载荷：所有整数走 i64 ZigZag（E5 约定）；BigInt/超 i64 走 u64 普通 varint；
  解码智能推断（字符串优先），歧义场景显式 schema
- 中间件拦截 RPC 回 FORBIDDEN 错误响应（不静默关闭流）；流帧载荷自动编解码（跨端一致）
- 认证竞态：认证事件与业务请求并发处理，中间件轮询等待认证状态生效
