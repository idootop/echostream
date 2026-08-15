# EchoStream 错误处理中间件

数据面扩展：捕获下游错误，统一转换为结构化错误响应。

## 用法

```rust
use echostream_middleware_error::ErrorMiddleware;

ServerBuilder::new()
    .middleware(ErrorMiddleware::new())          // 默认：错误码透传 + 错误消息
    .middleware(ErrorMiddleware::new().hide_internal()) // 生产环境：隐藏内部细节
    .build().await?;
```

- RPC 错误：回 `Response{ code, message }`（错误码可自定义）
- 事件 / 流错误：记录日志并继续传播
