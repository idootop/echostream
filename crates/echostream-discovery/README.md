# echostream-discovery

基于 mDNS 的轻量级局域网服务发现模块。

## 设计原则

- **精简**: 仅依赖 `mdns-sd`，无冗余中间层
- **直观**: `Discovery` 单一入口，支持流式 (Stream) 发现
- **健壮**: 自动处理实例名冲突，内置属性 (TXT record) 编解码

## 核心模型

### ServiceInfo

定义一个可发现的服务单元。

- `name`: 服务标识
- `address`: 自动获取的本地 IP
- `metadata`: 键值对属性（版本、权重、协议等）

## 子模块划分

- `service.rs`: ServiceInfo 模型与属性转换
- `discovery.rs`: Discovery 门面类，封装广播与发现逻辑
- `error.rs`: 模块专用错误类型

## API 示例

```rust
use echostream_discovery::{Discovery, ServiceInfo};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = ServiceInfo::new("echo-bolt")
        .with_property("protocol", "quic")
        .with_property("id", "node-1");

    // 只要 advertiser 不被 drop，广播就会持续
    let _advertiser = Discovery::advertise(service).await?;

    // 维持主程序运行
    tokio::signal::ctrl_c().await?;
    Ok(())
}

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
