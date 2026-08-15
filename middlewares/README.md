# EchoStream 中间件

这里存放 EchoStream 的核心中间件集合（数据面扩展：消息拦截与转换，洋葱链执行）。

## 核心中间件

| 中间件 | 功能 |
|--------|------|
| echostream-middleware-logging | 结构化消息日志（含处理耗时） |
| echostream-middleware-timeout | 中间件链与处理器执行超时控制（RPC 回 TIMEOUT） |
| echostream-middleware-error | 下游错误统一归一化（业务错误码透传） |
| echostream-middleware-transform | 请求/响应载荷转换（压缩、加密、格式包装等） |

## 开发指南

中间件实现 `Middleware` trait：返回 `Ok(None)` 拦截消息，返回 `Ok(Some(msg))` 可修改内容。

```rust
use async_trait::async_trait;
use echostream_core::{Middleware, Session};
use echostream_proto::{Message, Result};

pub struct Logging;

#[async_trait]
impl Middleware for Logging {
    fn name(&self) -> &str {
        "logging"
    }

    async fn on_message(&self, session: &Session, msg: Message) -> Result<Option<Message>> {
        tracing::info!(session = %session.id(), ?msg, "收到消息");
        Ok(Some(msg)) // 放行；返回 None 拦截
    }
}
```

创建新中间件时，建议遵循以下结构：

```text
middleware-name/
├── Cargo.toml
├── src/
│   └── lib.rs
└── README.md
```

中间件应该：

- 职责单一，功能聚焦
- 性能优先，避免阻塞
- 提供清晰的错误信息
