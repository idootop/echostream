# EchoStream 插件

这里存放 EchoStream 的核心插件集合（控制面扩展：生命周期 / 配置打包）。

## 核心插件

| 插件 | 功能 | 运行示例 |
|------|------|----------|
| echostream-plugin-auth | 连接 Token 认证（认证事件 + 中间件拦截） | cargo run -p echostream --example plugin_stack |
| echostream-plugin-reconnect | 客户端断线自动重连（指数退避，主动关闭不重连） | 同上 |
| echostream-plugin-retry | RPC 请求失败自动重试（仅可恢复错误） | 同上 |
| echostream-plugin-metrics | 请求统计与性能指标（快照 RPC 查询） | cargo run -p echostream --example middleware_stack |
| echostream-plugin-heartbeat | 心跳保活（客户端周期心跳 + 服务端失活清理） | 同上 |

## 开发指南

插件通过 Builder 打包处理器与生命周期钩子，对外只暴露一个可复用的集合：

```rust
use echostream_core::{ServerBuilder, ServerPlugin};

pub struct MyPlugin;

impl ServerPlugin for MyPlugin {
    fn name(&self) -> &str {
        "my-plugin"
    }

    fn install(self: Box<Self>, builder: ServerBuilder) -> ServerBuilder {
        builder
            .add_rpc(MyRpc)
            .on_connect(|session| {
                // 连接钩子
            })
    }
}
```

创建新插件时，建议遵循以下结构：

```text
plugin-name/
├── Cargo.toml
├── src/
│   └── lib.rs
└── README.md
```

插件应该：

- 接口简洁，开箱即用
- 零依赖或最小化依赖
- 提供清晰的使用示例
- 性能优先，避免不必要的开销
