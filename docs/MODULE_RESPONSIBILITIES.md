# EchoStream 模块职责

> 一个源：README、代码与发布以本文件为准（v3：传输并入核心 + 复用通道）。

## 分层与依赖方向

```
协议层    echostream-proto         Message/Error/传输接口/帧编解码/动态值编解码（零运行时依赖）
                                    ↑
框架层    echostream-core          Server/Client/Session/Router/Handler/中间件/插件/复用通道/连接池
          echostream-core::quic    内置 QUIC 传输（quinn，默认开启；原 echostream-transport 并入）
          echostream-client-core   无 I/O 客户端状态机（WASM/绑定复用）
                                    ↑
传输层    echostream-ws            WebSocket（局域网 Web 零证书）
          echostream-web           WebTransport（公网浏览器）
                                    ↑
扩展层    echostream-derive        过程宏：#[rpc] / #[event] / #[stream]
          echostream-discovery     mDNS 局域网发现
          echostream-middleware-*  中间件集合（数据面）
          echostream-plugin-*      插件集合（控制面）
                                    ↑
入口      echostream               统一入口（重导出 + prelude + 宏）
                                    ↑
绑定      bindings/node|python|wasm|web
```

## 模块职责表

| 模块 | 职责 | 依赖 |
|------|------|------|
| echostream-proto | Message（Request/Response/Event/Stream/StreamEnd）、Error、传输接口（Endpoint/FrameIo/FrameRead/FrameWrite/Listener）、帧编解码（长度前缀+postcard）、动态值编解码（跨语言自动序列化约定 `dynamic`） | serde/bytes/postcard/async-trait/thiserror |
| echostream-core | Server/Client（Builder，含连接池 pool(n)）、Session（双向通信 + 事件复用通道 + RPC 复用通道）、Router（分发）、强类型 Handler、中间件、插件、ServerContext、内置 QUIC 传输（quic 模块） | proto、tokio、quinn（quic feature，默认开） |
| echostream-client-core | 无 I/O 状态机：RPC 匹配/事件路由/流管理/流结束/入站流路由 | proto、bytes、postcard、futures |
| echostream-ws | WebSocket 服务端：WsServer/WsServerBuilder，帧协议与 QUIC 一致 | core、proto、tokio-tungstenite |
| echostream-web | WebTransport 服务端：WebServer/WebServerBuilder | core、proto、wtransport |
| echostream-derive | #[rpc]/#[event]/#[stream] | syn/quote/proc-macro2 |
| echostream-discovery | mDNS 发现：advertise/discover/discover_stream + metadata | proto、mdns-sd、tokio |
| echostream-middleware-* | 数据面扩展集合：logging 等 | core |
| echostream-plugin-* | 控制面扩展集合：reconnect/auth/retry 等 | core |
| echostream | 统一入口：re-export + prelude + 宏 | 全部 |
| bindings/* | 各语言绑定（自动编解码，底层 API 另提供） | 各自工具链 |

## 关键设计

- **传输无关框架**：core 只依赖 proto 的传输接口（Endpoint/FrameIo/Listener），
  具体传输（QUIC / WS / WebTransport）各自实现；QUIC 为主传输随核心开箱即用，
  `ServerBuilder::listener` / `ClientBuilder::from_endpoint` 注入任意传输
- **帧协议统一**：所有传输同一线缆格式（长度前缀 + postcard Message）；
  WebSocket 无流关闭语义 → StreamEnd 消息显式标记
- **RPC 复用通道**：默认一条长连接双向流按请求 id 多路复用（高频小请求性能最优，
  通道开启标记为保留方法名 `$channel`）；载荷 >64KiB 自动切独立流；
  连接池（pool(n)）多连接分摊流控窗口并跨核扩展
- **事件复用通道**：长连接单向流批量帧（可靠）；数据报（不可靠，吞吐最高）
- **自动编解码**：`proto::dynamic` 定义跨语言载荷约定（i64 ZigZag 等），
  Rust derive / WASM / Node postcard.js / Python postcard.py 四端实现一致，字节级交叉验证
- **扩展机制**：中间件 = 数据面（消息拦截/修改）；插件 = 控制面（生命周期/配置打包）
- **多端复用**：client-core 状态机 + proto 编解码编译 WASM，Web SDK 与 Rust 原生共享核心
