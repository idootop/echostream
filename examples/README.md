# EchoStream Examples

完整示例位于 `crates/echostream/examples/`：

| 示例 | 内容 | 运行 |
|------|------|------|
| `simple_rpc` | RPC / Event / Stream 三种模式端到端（声明式宏） | `cargo run -p echostream --example simple_rpc` |
| `bidi` | 服务端主动调用客户端、事件监听、中间件、生命周期钩子 | `cargo run -p echostream --example bidi` |
| `discovery` | mDNS 服务发现与连接 | `cargo run -p echostream --example discovery` |
| `chat_server` | 常驻服务端（Node/Python 绑定测试用） | `cargo run -p echostream --example chat_server` |
| `bench` | 基准测试（RPC 延迟/吞吐、事件、流） | `cargo run -p echostream --example bench --release` |
| `codec_probe` | 协议基准字节输出（跨语言编解码验证） | `cargo run -p echostream --example codec_probe` |

WebTransport 示例位于 `crates/echostream-web/examples/`：

| 示例 | 内容 | 运行 |
|------|------|------|
| `web_server` | WebTransport 服务端 + wtransport 客户端验证 | `cargo run -p echostream-web --example web_server` |
| `web_chat_server` | WebTransport 常驻服务端（浏览器 E2E 用） | `cargo run -p echostream-web --example web_chat_server --release` |
