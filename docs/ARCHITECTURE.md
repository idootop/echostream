# EchoStream 架构与模块职责

> 一个源：README、代码与发布以本文件为准（v4：core 传输无关 + transport 三 feature + client-core 并入）。

## 分层与依赖方向

```text
协议层    echostream-proto         Message/Error/传输接口/帧编解码/动态值编解码（零运行时依赖）
                                    ↑
框架层    echostream-core          Server/Client/Session/Router/Handler/中间件/插件/复用通道/连接池
          echostream-core::ClientCore  无 I/O 客户端状态机（WASM 可编译；io feature 关闭时仅剩此 + 编解码）
                                    ↑
传输层    echostream-transport     quic（默认）/ ws / web 三 feature + QUIC 便捷 bind/connect
                                    ↑
扩展层    echostream-derive         过程宏：#[rpc] / #[event] / #[stream]
          echostream-discovery     mDNS 局域网发现（Discovery 门面 + ServiceInfo builder）
          echostream-middleware-*  中间件集合（数据面）
          echostream-plugin-*      插件集合（控制面）
                                    ↑
入口      echostream               统一入口（重导出 + prelude + 宏 + QUIC 便捷 API）
                                    ↑
绑定      bindings/node|python|wasm|web
```

依赖方向：proto ← core ← transport ← echostream，无环。

## 模块职责表

| 模块 | 职责 | 依赖 |
|------|------|------|
| echostream-proto | Message（Request/Response/Event/Stream/StreamEnd）、Error、传输接口（Endpoint/FrameIo/FrameRead/FrameWrite/Listener）、帧编解码（长度前缀+postcard）、动态值编解码（跨语言自动序列化约定 dynamic） | serde/bytes/postcard/async-trait/thiserror |
| echostream-core | Server/Client（Builder，含连接池 pool(n)，传输无关）、Session（双向通信 + 事件/RPC 复用通道）、Router、强类型 Handler、中间件、插件、ServerContext、无 I/O 状态机 ClientCore | proto、tokio（io feature，默认开） |
| echostream-transport | QUIC（quic 默认：端点/流/数据报/证书 + ServerBuilderExt::bind / ClientBuilderExt::connect）、WebSocket（ws：WsServer）、WebTransport（web：WebServer） | core、proto、quinn/tokio-tungstenite/wtransport（按 feature） |
| echostream-derive | #[rpc]/#[event]/#[stream] | syn/quote/proc-macro2 |
| echostream-file | 场景扩展：文件流传输（分块发送 + sha256 校验和 + 大小校验） | core |
| echostream-av | 场景扩展：音视频推流/接收（参数协商 + pts/关键帧封装） | core |
| echostream-discovery | 场景扩展：mDNS 发现：Discovery::advertise/discover/discover_stream + ServiceInfo（builder） | proto、mdns-sd、tokio |
| echostream-middleware-* | 数据面扩展集合：logging/timeout/error/transform | core |
| echostream-plugin-* | 控制面扩展集合：auth/reconnect/retry/metrics/heartbeat | core（reconnect 另依赖 transport quic） |
| echostream-file | 场景扩展：文件流传输（分块 + sha256 校验 + 大小校验） | core |
| echostream-av | 场景扩展：音视频推流/接收（参数协商 + pts/关键帧封装） | core |
| echostream | 统一入口：re-export + prelude + 宏 + QUIC 便捷 API | 全部 |
| bindings/* | 各语言绑定（自动编解码，底层 API 另提供） | 各自工具链 |

## 关键设计

- **core 传输无关**：只依赖 proto 的传输接口；ServerBuilder::listener / from_endpoint 注入任意传输，
  listener_factory 支持延迟创建；QUIC 便捷 bind/connect 由 transport 扩展 trait 提供（入口 prelude 重导出）
- **transport 三 feature**：quic（默认）/ ws / web 同一包内按需启用，帧协议统一（长度前缀 + postcard Message）
- **client-core 并入 core**：ClientCore 无 I/O 状态机（RPC 匹配/事件路由/流管理/入站流），
  为 WASM 浏览器场景剥离；core 的 io feature 门控 tokio 相关模块，关闭后可编译 WASM
- **RPC 复用通道**：默认长连接双向流按 id 多路复用（高频小请求性能最优，通道开启标记为保留方法名
  $channel）；载荷 >64KiB 自动切独立流；连接池（pool(n)）多连接分摊流控窗口并跨核扩展
- **事件复用通道**：长连接单向流批量帧（可靠）；数据报（不可靠，吞吐最高）
- **流三帧协议**：StreamOpen（名称 + 可扩展元数据协商，如音视频参数 / 文件信息 / clock-rate）→
  StreamMsg（按 id 路由的数据帧，核心仅传输语义）→ StreamEnd（结束码 + 原因 + trailers）；
  采样时钟 / 关键帧等上层语义由上层插件在载荷内实现，核心协议稳定极简（gRPC headers/messages/trailers 同构）
- **自动编解码**：proto::dynamic 定义跨语言载荷约定（i64 ZigZag 等），
  Rust derive / WASM / Node postcard.js / Python postcard.py 四端实现一致，字节级交叉验证
- **扩展机制三类**：插件 = 控制面（实现 Plugin trait，install 装配 Builder）；
  中间件 = 数据面（实现 Middleware trait，洋葱链）；
  扩展 = 场景工具（业务直接调用的库，如文件传输 / 音视频流，无宿主契约）
- **多端复用**：ClientCore 状态机 + proto 编解码编译 WASM，Web SDK 与 Rust 原生共享核心
