# echostream-middleware-logging

EchoStream 日志中间件：结构化记录入站消息（类型/名称/会话），基于 tracing。

```rust
ServerBuilder::new()
    .bind("0.0.0.0:5000")
    .middleware(LoggingMiddleware::new())
    .build().await?;
```
