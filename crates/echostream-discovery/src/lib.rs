//! EchoStream 局域网服务发现（基于 mDNS）
//!
//! 零配置发现局域网内的 EchoStream 服务端：
//! - `advertise` / `advertise_with`：广播服务（返回 RAII guard，drop 后自动停止广播）
//! - `discover`：一次性发现（超时返回已发现的服务列表）
//! - `discover_stream`：持续发现（流式返回新上线的服务）
//!
//! 服务类型固定为 `_echostream._udp.local.`（QUIC 基于 UDP），
//! 通过 TXT 记录携带服务元数据（版本、能力等）。

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use echostream_proto::{Error, Result};
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo as MdnsInfo, TxtProperties};

/// mDNS 服务类型（QUIC 基于 UDP）
const SERVICE_TYPE: &str = "_echostream._udp.local.";

/// 发现的服务的元数据与地址
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceInfo {
    /// 服务实例名
    pub name: String,
    /// 服务地址
    pub addr: SocketAddr,
    /// 服务元数据（TXT 记录）
    pub metadata: HashMap<String, String>,
}

/// 服务广播器（RAII：drop 后自动停止广播）
pub struct Advertiser {
    daemon: ServiceDaemon,
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

/// 广播服务（无元数据；监听端口需与 `ServerBuilder::bind` 一致）
pub fn advertise(name: &str, port: u16) -> Result<Advertiser> {
    advertise_with(name, port, HashMap::new())
}

/// 广播服务并携带元数据（TXT 记录，供发现方读取）
pub fn advertise_with(
    name: &str,
    port: u16,
    metadata: HashMap<String, String>,
) -> Result<Advertiser> {
    let daemon = ServiceDaemon::new().map_err(|e| Error::Io(e.to_string()))?;
    let info = MdnsInfo::new(
        SERVICE_TYPE,
        name,
        &format!("{name}.local."),
        local_ipv4()?,
        port,
        metadata,
    )
    .map_err(|e| Error::InvalidParameter(e.to_string()))?;
    daemon
        .register(info)
        .map_err(|e| Error::Io(e.to_string()))?;
    Ok(Advertiser { daemon })
}

/// 一次性发现：超时内收集所有匹配的服务
pub async fn discover(name: &str, timeout: Duration) -> Result<Vec<ServiceInfo>> {
    let daemon = ServiceDaemon::new().map_err(|e| Error::Io(e.to_string()))?;
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .map_err(|e| Error::Io(e.to_string()))?;

    let deadline = std::time::Instant::now() + timeout;
    let mut services = Vec::new();
    while std::time::Instant::now() < deadline {
        let remain = deadline.saturating_duration_since(std::time::Instant::now());
        if let Ok(ServiceEvent::ServiceResolved(info)) = receiver.recv_timeout(remain)
            && instance_matches(&info, name)
        {
            collect_services(&info, &mut services);
        }
    }
    let _ = daemon.shutdown();
    Ok(services)
}

/// 持续发现：流式返回新上线的服务
pub fn discover_stream(name: &str) -> impl tokio_stream::Stream<Item = ServiceInfo> {
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
                for service in services_of(&info) {
                    if tx.blocking_send(service).is_err() {
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

/// 提取 TXT 元数据
fn metadata_of(info: &ResolvedService) -> HashMap<String, String> {
    txt_to_map(info.get_properties())
}

/// TXT 属性转 HashMap
fn txt_to_map(props: &TxtProperties) -> HashMap<String, String> {
    props
        .iter()
        .map(|p| (p.key().to_string(), p.val_str().to_string()))
        .collect()
}

/// 提取服务列表（每个地址一个 ServiceInfo）
fn services_of(info: &ResolvedService) -> Vec<ServiceInfo> {
    let metadata = metadata_of(info);
    let name = instance_name(info);
    info.get_addresses_v4()
        .into_iter()
        .map(|ip| ServiceInfo {
            name: name.clone(),
            addr: SocketAddr::new(IpAddr::V4(ip), info.get_port()),
            metadata: metadata.clone(),
        })
        .collect()
}

/// 追加服务（按地址去重）
fn collect_services(info: &ResolvedService, services: &mut Vec<ServiceInfo>) {
    for service in services_of(info) {
        if !services.contains(&service) {
            services.push(service);
        }
    }
}

/// 提取实例名（fullname 形如 `name._echostream._udp.local.`）
fn instance_name(info: &ResolvedService) -> String {
    info.get_fullname()
        .split('.')
        .next()
        .unwrap_or_default()
        .to_string()
}
