# echostream-plugin-retry

EchoStream RPC 重试插件：请求失败自动重试（指数退避）。

```rust
use echostream_plugin_retry::{RetryPolicy, request_with_retry};

let policy = RetryPolicy::default(); // 最多 3 次，基础间隔 200ms
let sum: i64 = request_with_retry(&client, "add", &(1, 2), &policy).await?;
```

默认仅重试可恢复错误（超时 / 连接断开 / 网络错误），业务错误直接返回。
