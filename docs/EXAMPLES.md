# EchoStream 示例导航

所有示例均可直接运行（Rust 示例无需额外服务；Node/Python 示例按说明配对启动）。

## Rust 示例

### 核心框架（crates/echostream/examples/）

| 示例 | 内容 | 运行 |
|------|------|------|
| simple_rpc | RPC / Event / Stream 三种模式端到端（声明式宏） | cargo run -p echostream --example simple_rpc |
| bidi | 服务端主动调用客户端、事件监听、中间件、生命周期钩子 | cargo run -p echostream --example bidi |
| discovery | mDNS 服务发现（ServiceInfo + Discovery）与连接 | cargo run -p echostream --example discovery |
| plugin_stack | 插件栈：认证 / 重连 / RPC 重试 / 日志 全链路 | cargo run -p echostream --example plugin_stack |
| chat_server | 常驻服务端（Node/Python 绑定测试用） | cargo run -p echostream --example chat_server |
| e2e_peer | 跨端矩阵通用对端（server / client 双角色） | cargo run -p echostream --release --example e2e_peer -- --server/--client |
| bench | 基准测试（RPC 延迟/吞吐、复用通道、连接池、事件、流） | cargo run -p echostream --example bench --release |
| codec_probe | 协议基准字节输出（跨语言编解码验证） | cargo run -p echostream --example codec_probe |

### 传输层（crates/echostream-transport/examples/）

| 示例 | 内容 | 运行 |
|------|------|------|
| ws_chat_server | WebSocket 服务端（浏览器局域网零证书） | cargo run -p echostream-transport --example ws_chat_server --features ws |
| web_server | WebTransport 服务端 + wtransport 客户端验证 | cargo run -p echostream-transport --example web_server --features web |
| web_chat_server | WebTransport 常驻服务端（浏览器 E2E 用） | cargo run -p echostream-transport --example web_chat_server --release --features web |

## Node.js 示例（bindings/node/examples/）

| 文件 | 内容 | 运行 |
|------|------|------|
| server.mjs | 服务端：RPC / 事件 / 流（自动编解码 DX） | node bindings/node/examples/server.mjs |
| client.mjs | 客户端：请求 / 事件 / 流（需先启动服务端） | node bindings/node/examples/client.mjs |

## Python 示例（bindings/python/examples/）

| 文件 | 内容 | 运行 |
|------|------|------|
| server.py | 服务端：RPC / 事件 / 流（自动编解码 DX） | python3 bindings/python/examples/server.py |
| client.py | 客户端：请求 / 事件 / 流（需先启动服务端） | python3 bindings/python/examples/client.py |

## Web 示例（bindings/web/）

| 文件 | 内容 | 运行 |
|------|------|------|
| index.html | 浏览器 demo 页面 | 静态服务 bindings/web 后访问 |
| e2e.html | 浏览器 E2E 页面（自动执行 RPC/事件/流） | 配合 ws_chat_server / web_chat_server |

## 跨端矩阵

```bash
node scripts/cross_e2e.mjs    # Rust ↔ Node ↔ Python 6 组合交叉验证
```