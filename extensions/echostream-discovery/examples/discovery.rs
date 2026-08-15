//! EchoStream 服务发现示例
//!
//! 服务端广播服务，客户端通过 mDNS 零配置发现并连接。
//! 同一进程内验证 advertise + discover 全链路。
//!
//! 运行：cargo run -p echostream --example discovery

use std::time::Duration;

use echostream::prelude::*;
use echostream_discovery::{Discovery, ServiceInfo};

/// RPC：加法
#[rpc("add")]
async fn add((a, b): (i64, i64)) -> Result<i64> {
    Ok(a + b)
}

#[tokio::main]
async fn main() -> Result<()> {
    let service_name = "echo-server-demo";

    // 服务端：绑定随机端口并广播服务（携带版本/能力元数据）
    let server = ServerBuilder::new()
        .bind("0.0.0.0:0")
        .add_rpc(Add)
        .build()
        .await?;
    let addr = server
        .endpoint_addr()
        .ok_or_else(|| Error::InvalidParameter("无法获取监听地址".into()))?;
    let service = ServiceInfo::new(service_name, addr.port())?
        .set_property("version", "0.1.0")
        .set_property("role", "demo-server");
    let _advertiser = Discovery::advertise(service)?;
    println!("[server] 已广播服务 {service_name} @ {addr}");

    let server_handle = tokio::spawn(async move {
        server.run().await.unwrap();
    });

    // 客户端：mDNS 发现服务并连接
    println!("[client] 正在发现服务 {service_name} ...");
    let found = Discovery::discover(service_name, Duration::from_secs(5)).await?;
    assert!(!found.is_empty(), "未发现服务");
    for svc in &found {
        println!(
            "[client] 发现服务 {} @ {}，属性: {:?}",
            svc.name(),
            svc.address(),
            svc.metadata()
        );
    }
    assert_eq!(svc_meta(&found, "version"), Some("0.1.0"));

    let client = ClientBuilder::new().connect(found[0].address()).await?;
    let sum: i64 = client.request("add", &(40, 2)).await?;
    println!("[client] add(40, 2) = {sum}");
    assert_eq!(sum, 42);

    client.close();
    server_handle.abort();
    println!("全部完成");
    Ok(())
}

/// 读取第一个服务的属性
fn svc_meta<'a>(services: &'a [ServiceInfo], key: &str) -> Option<&'a str> {
    services.first().and_then(|s| s.get_property(key))
}
