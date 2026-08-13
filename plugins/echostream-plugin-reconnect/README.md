# echostream-plugin-reconnect

EchoStream 自动重连插件：客户端断线后指数退避自动重连。

```rust
use echostream_plugin_reconnect::ReconnectPlugin;

ClientBuilder::new()
    .plugin(ReconnectPlugin::new("127.0.0.1:5000"))
    .connect("127.0.0.1:5000").await?;
```

配置：`max_retries(n)` / `base_delay(d)`（指数退避 base * 2^n）。
