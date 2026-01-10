# echostream-discovery

基于 mDNS 的轻量级局域网服务发现模块。

## 子模块划分

- `service.rs`: ServiceInfo 模型与属性转换
- `discovery.rs`: Discovery 门面类，封装广播与发现逻辑
- `error.rs`: 模块专用错误类型

## 核心模型

### ServiceInfo

定义一个可发现的服务单元。

- `name`: 服务标识
- `address`: 自动获取的本地 IP
- `metadata`: 键值对属性（版本、权重、协议等）

## API 示例

```rust
use echostream_discovery::{Discovery, ServiceInfo};

// 创建服务信息
let service = ServiceInfo::new("AudioService")
    .set_property("port", 8080)
    .set_property("id", "node-1");

// 广播服务
let advertiser = Discovery::advertise(service).await?;

// 发现服务
let services = Discovery::discover("AudioService", timeout).await?;

for service in services {
    println!("发现服务: {} at {}:{}",
        service.name,
        service.address,
        service.get_property("port").unwrap_or_default(),
    );
}
```


## 注意事项

- mDNS 仅适用于局域网环境
- 服务发现有网络延迟，建议设置合理的超时时间
- 防火墙可能阻止 mDNS 流量（UDP 5353 端口）
