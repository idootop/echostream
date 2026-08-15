# EchoStream 超时中间件

数据面扩展：为整条中间件链（含业务处理器）设置超时上限。

## 用法

```rust
use std::time::Duration;
use echostream_middleware_timeout::TimeoutMiddleware;

ServerBuilder::new()
    .middleware(TimeoutMiddleware::new(Duration::from_secs(5)))
    .build().await?;
```

- RPC 超时：回 TIMEOUT 错误响应（错误码透传）
- 事件 / 流超时：终止处理并记录日志
