# EchoStream 心跳保活插件

客户端周期发送心跳，服务端清理失活会话（防半开连接泄漏）。

## 用法

```rust
use std::time::Duration;
use echostream_plugin_heartbeat::{HeartbeatClientPlugin, HeartbeatServerPlugin};

// 服务端：15s 未收到心跳断开
ServerBuilder::new().plugin(HeartbeatServerPlugin::new(Duration::from_secs(15)));

// 客户端：每 5s 心跳一次（建议 < 服务端超时的 1/3）
ClientBuilder::new().plugin(HeartbeatClientPlugin::new(Duration::from_secs(5)));
```

- 心跳事件名：__echostream.heartbeat（中间件拦截，不向业务分发）
- 服务端失活扫描：on_start 后台任务，按 interval 定期扫描并断开超时会话
