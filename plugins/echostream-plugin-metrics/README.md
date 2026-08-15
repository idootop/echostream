# EchoStream 指标插件

请求统计与性能指标收集：RPC 计数/延迟、Event/Stream 计数、连接数。

## 用法

```rust
use echostream_plugin_metrics::MetricsPlugin;

let builder = ServerBuilder::new().plugin(MetricsPlugin::new());
let server = builder.build().await?;
// 客户端查询快照：
// let snapshot: MetricsSnapshot = client.request("metrics.snapshot", &()).await?;
```

- 快照 RPC 默认 metrics.snapshot（MetricsPlugin::rpc_name 可改）
- MetricsPlugin::registry() 可共享注册表，供外部采集 / 上报
