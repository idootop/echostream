# echostream-discovery

基于 mDNS 的局域网零配置服务发现模块。

## 核心模型：ServiceInfo

可发现的服务单元（builder 风格构造，自动获取本地 IP）。

```rust
use echostream_discovery::{Discovery, ServiceInfo};

// 创建服务信息（名称 + 端口，属性为 TXT 记录）
let service = ServiceInfo::new("AudioService", 8080)?
    .set_property("version", "0.1.0")
    .set_property("id", "node-1");
```

## API

```rust
use echostream_discovery::{Discovery, ServiceInfo};
use std::time::Duration;

// 广播服务（返回 RAII guard，drop 后自动停止）
let _advertiser = Discovery::advertise(service)?;

// 一次性发现（超时返回服务列表）
let services = Discovery::discover("AudioService", Duration::from_secs(3)).await?;

for svc in &services {
    println!(
        "发现服务: {} at {}:{}，版本 {}",
        svc.name(),
        svc.address().ip(),
        svc.address().port(),
        svc.get_property("version").unwrap_or_default(),
    );
}

// 持续发现（流式返回新上线的服务）
let mut stream = Discovery::discover_stream("AudioService");
while let Some(svc) = stream.next().await {
    // ...
}
```

## 注意事项

- mDNS 仅适用于局域网环境
- 服务发现有网络延迟，建议设置合理的超时时间
- 防火墙可能阻止 mDNS 流量（UDP 5353 端口）
