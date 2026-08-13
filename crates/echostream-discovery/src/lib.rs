//! EchoStream 局域网服务发现（基于 mDNS）
//!
//! 零配置发现局域网内的 EchoStream 服务端：
//! - `advertise`：广播服务（返回 RAII guard，drop 后自动停止广播）
//! - `discover`：一次性发现（超时返回已发现的服务列表）
//! - `discover_stream`：持续发现（流式返回新上线的服务）
//!
//! 服务类型固定为 `_echostream._udp.local.`（QUIC 基于 UDP）。

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use echostream_proto::{Error, Result};
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};

/// mDNS 服务类型（QUIC 基于 UDP）
const SERVICE_TYPE: &str = "_echostream._udp.local.";

/// 服务广播器（RAII：drop 后自动停止广播）
pub struct Advertiser {
    daemon: ServiceDaemon,
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

/// 广播服务（监听端口需与 `ServerBuilder::bind` 一致）
pub fn advertise(name: &str, port: u16) -> Result<Advertiser> {
    let daemon = ServiceDaemon::new().map_err(|e| Error::Io(e.to_string()))?;
    let info = ServiceInfo::new(
        SERVICE_TYPE,
        name,
        &format!("{name}.local."),
        local_ipv4()?,
        port,
        HashMap::<String, String>::new(),
    )
    .map_err(|e| Error::InvalidParameter(e.to_string()))?;
    daemon
        .register(info)
        .map_err(|e| Error::Io(e.to_string()))?;
    Ok(Advertiser { daemon })
}

/// 一次性发现：超时内收集所有匹配的服务地址
pub async fn discover(name: &str, timeout: Duration) -> Result<Vec<SocketAddr>> {
    let daemon = ServiceDaemon::new().map_err(|e| Error::Io(e.to_string()))?;
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .map_err(|e| Error::Io(e.to_string()))?;

    let deadline = std::time::Instant::now() + timeout;
    let mut addrs = Vec::new();
    while std::time::Instant::now() < deadline {
        let remain = deadline.saturating_duration_since(std::time::Instant::now());
        if let Ok(ServiceEvent::ServiceResolved(info)) = receiver.recv_timeout(remain)
            && instance_matches(&info, name)
        {
            collect_addrs(&info, &mut addrs);
        }
    }
    let _ = daemon.shutdown();
    Ok(addrs)
}

/// 持续发现：流式返回新上线的服务地址
pub fn discover_stream(name: &str) -> impl tokio_stream::Stream<Item = SocketAddr> {
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let name = name.to_string();
    std::thread::spawn(move || {
        let daemon = match ServiceDaemon::new() {
            Ok(d) => d,
            Err(_) => return,
        };
        let receiver = match daemon.browse(SERVICE_TYPE) {
            Ok(r) => r,
            Err(_) => return,
        };
        while let Ok(event) = receiver.recv() {
            if let ServiceEvent::ServiceResolved(info) = event
                && instance_matches(&info, &name)
            {
                for addr in addrs_of(&info) {
                    if tx.blocking_send(addr).is_err() {
                        let _ = daemon.shutdown();
                        return;
                    }
                }
            }
        }
        let _ = daemon.shutdown();
    });
    tokio_stream::wrappers::ReceiverStream::new(rx)
}

// ======================== 内部辅助 ========================

/// 获取本机 IPv4 地址（UDP 探测，不实际发包）
fn local_ipv4() -> Result<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|e| Error::Io(e.to_string()))?;
    socket
        .connect("8.8.8.8:80")
        .map_err(|e| Error::Io(e.to_string()))?;
    socket
        .local_addr()
        .map(|a| a.ip())
        .map_err(|e| Error::Io(e.to_string()))
}

/// 判断解析出的服务实例名是否匹配
fn instance_matches(info: &ResolvedService, name: &str) -> bool {
    info.get_fullname().starts_with(&format!("{name}."))
}

/// 提取服务地址列表
fn addrs_of(info: &ResolvedService) -> Vec<SocketAddr> {
    info.get_addresses_v4()
        .into_iter()
        .map(|ip| SocketAddr::new(IpAddr::V4(ip), info.get_port()))
        .collect()
}

/// 追加服务地址（去重）
fn collect_addrs(info: &ResolvedService, addrs: &mut Vec<SocketAddr>) {
    for addr in addrs_of(info) {
        if !addrs.contains(&addr) {
            addrs.push(addr);
        }
    }
}
