# EchoStream 模块职责（v2 分层）

> 一个源：README、代码与发布以本文件为准。

## 分层与依赖方向

```
协议层    echostream-proto         Message/Error/传输接口/帧编解码（零运行时依赖）
                                    ↑
框架层    echostream-core           Router/Handler/Session/Server/Client/中间件/插件/状态
          echostream-client-core    无 I/O 客户端状态机（WASM/绑定复用）
                                    ↑
传输层    echostream-transport      QUIC（quinn，实现 proto 接口）
          echostream-ws             WebSocket（局域网 Web 零证书）
          echostream-web            WebTransport（公网浏览器）
                                    ↑
扩展层    echostream-derive         过程宏
          echostream-discovery      mDNS 局域网发现（通用化）
          echostream-middleware-*   中间件集合（数据面）
          echostream-plugin-*       插件集合（控制面）
                                    ↑
入口      echostream                统一入口（重导出 + 便捷 QUIC bind/connect）
                                    ↑
绑定      bindings/node|python|wasm|web
```

## 模块职责表

| 模块 | 职责 | 依赖 |
|------|------|------|
| echostream-proto | Message（Request/Response/Event/Stream/StreamEnd）、Error、**传输接口**（Endpoint/FrameIo/FrameRead/FrameWrite/Listener）、帧编解码（长度前缀+postcard） | serde/bytes/postcard/async-trait/thiserror |
| echostream-core | Server/Client（Builder，传输无关：listener/from_endpoint）、Session（双向通信）、Router（分发）、强类型 Handler、中间件、插件、ServerContext、生命周期钩子 | proto、tokio（可选 quic feature 依赖 transport） |
| echostream-client-core | 无 I/O 状态机：RPC 匹配/事件路由/流管理/流结束 | proto、bytes、futures |
| echostream-transport | QUIC 实现：QuicEndpoint（Listener）/QuicConn（Endpoint）/流/证书/0-RTT | proto、quinn、rustls、rcgen、postcard |
| echostream-ws | WebSocket 服务端：WsServer/WsServerBuilder，帧协议与 QUIC 一致，流结束用 StreamEnd | core、proto、tokio-tungstenite |
| echostream-web | WebTransport 服务端：WebServer/WebServerBuilder | core、proto、wtransport |
| echostream-derive | #[rpc]/#[event]/#[stream] | syn/quote/proc-macro2 |
| echostream-discovery | mDNS 发现：advertise/discover/discover_stream + metadata | proto、mdns-sd、tokio |
| echostream-middleware-* | 数据面扩展集合：timeout/logging/auth 校验等 | core |
| echostream-plugin-* | 控制面扩展集合：reconnect/auth/logging 等 | core |
| echostream | 统一入口：re-export + prelude + 宏 + QUIC bind/connect 便捷 | 全部 |
| bindings/* | 各语言绑定（node/python/wasm/web） | 各自工具链 |

## 关键设计

- **传输无关框架**：core 只依赖 proto 的接口（Endpoint/FrameIo/Listener），
  具体传输（QUIC/WS/WebTransport）各自独立 crate 实现；ServerBuilder::listener /
  ClientBuilder::from_endpoint 注入任意传输，QUIC 便捷 API 走 core 的 quic feature。
- **帧协议统一**：所有传输同一线缆格式（长度前缀 + postcard Message）；
  WebSocket 无流关闭语义 → StreamEnd 消息显式标记。
- **通信模型**：RPC 每请求一条逻辑流（业界标准，QUIC 流廉价无队头阻塞）；
  事件走复用通道（长连接 uni 流批量帧）或 datagram（不可靠）。
- **扩展机制**：中间件 = 数据面（消息拦截/修改）；插件 = 控制面（生命周期/配置打包）。
- **多端复用**：client-core 状态机 + proto 编解码编译 WASM，Web SDK 与 Rust 原生共享核心。
