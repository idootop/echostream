# EchoStream 模块职责

> 当前实现的模块边界与依赖方向（一个源，README 与代码以本文件为准）。

## 依赖方向

```
proto（零依赖）← transport ← core ← derive / discovery（独立可选）← echostream（入口）
```

| 模块 | 职责 | 依赖 |
|------|------|------|
| echostream-proto | 协议类型：`Message`（Request/Response/Event/Stream）、`Error`、`Timestamp`、`StatusCode` | serde、bytes、thiserror |
| echostream-transport | QUIC 封装：端点/连接/双向流/单向流/数据报、自签证书、帧编解码（长度前缀 + postcard） | proto、quinn、rustls、rcgen、postcard |
| echostream-core | 框架：Server/Client（Builder）、Session（双向主动通信）、Router（分发）、强类型 Handler、中间件、插件、生命周期钩子、ServerContext（状态/会话/广播） | proto、transport、tokio、serde、postcard、async-trait |
| echostream-derive | 过程宏：`#[rpc]` / `#[event]` / `#[stream]`，生成零大小 Handler 结构体 | syn、quote、proc-macro2 |
| echostream-discovery | mDNS 服务发现：`advertise` / `discover` / `discover_stream` | proto、mdns-sd、tokio |
| echostream | 统一入口：重导出 + prelude + 宏（feature：derive、discovery） | core、proto |

## 关键设计

- **通信模型**：每条消息一条 QUIC 流。RPC 走双向流（请求 + 响应同流），事件/流走单向流。流首帧由分派层读出并缓存进 `StreamReceiver`。
- **强类型 Handler**：`RpcHandler<Req, Resp>` / `EventHandler<Data>` 只面对具体类型，编解码由框架统一处理（`handle_encoded` 入口），宏自动实现。
- **错误转换**：transport 层用本地 trait（`ToEcho`）将 quinn 错误转为 `proto::Error`，避免孤儿规则。
- **会话状态**：`Session` 为具体结构（Arc 共享），提供会话级 K-V 状态与 `request`/`emit`/`create_stream` 双向通信。
- **钩子**：`on_start` / `on_stop` / `on_connect` / `on_disconnect`（同步回调，异步逻辑内部 spawn）。
- **中间件**：`Middleware::on_message` 洋葱链，返回 `None` 拦截。
- **插件**：`ServerPlugin::install(Box<Self>, ServerBuilder)` 打包处理器与钩子。
- **优雅关闭**：`Server::shutdown()` 停止接受新连接并触发 `on_stop`。
