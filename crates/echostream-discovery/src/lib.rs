//! EchoStream 局域网服务发现（基于 mDNS）
//!
//! 零配置发现局域网内的 EchoStream 服务端：
//! - ServiceInfo：服务描述（builder 风格：名称 + 端口 + 属性）
//! - Discovery::advertise：广播服务（返回 RAII guard，drop 后自动停止）
//! - Discovery::discover：一次性发现（超时返回服务列表）
//! - Discovery::discover_stream：持续发现（流式返回新上线的服务）
//!
//! 服务类型固定为 _echostream._udp.local.（QUIC 基于 UDP），
//! 通过 TXT 记录携带服务元数据（版本、能力等）。

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use echostream_proto::{Error, Result};
use mdns_sd::{
    ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo as MdnsInfo, TxtProperties,
};

/// mDNS 服务类型（QUIC 基于 UDP）
const SERVICE_TYPE: &str = "_echostream._udp.local.";

/// 可发现的服务单元（builder 风格构造）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceInfo {
    /// 服务实例名
    name: String,
    /// 服务地址（自动获取的本地 IP + 端口）
    addr: SocketAddr,
    /// 服务元数据（TXT 记录，键值对属性：版本、能力等）
    metadata: HashMap<String, String>,
}

impl ServiceInfo {
    /// 创建服务信息（自动获取本地 IP，见 crate::local_ipv4）
    pub fn new(name: &str, port: u16) -> Result<Self> {
        Ok(Self {
            name: name.to_string(),
            addr: SocketAddr::new(local_ipv4()?, port),
            metadata: HashMap::new(),
        })
    }

    /// 设置服务属性（版本、能力、权重等；TXT 记录）
    pub fn set_property(mut self, key: impl Into<String>, value: impl ToString) -> Self {
        self.metadata.insert(key.into(), value.to_string());
        self
    }

    /// 服务实例名
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 服务地址
    pub fn address(&self) -> SocketAddr {
        self.addr
    }

    /// 服务属性
    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    /// 读取服务属性
    pub fn get_property(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(String::as_str)
    }
}

/// 服务发现门面
pub struct Discovery;

impl Discovery {
    /// 广播服务（返回 RAII guard，drop 后自动停止广播）
    pub fn advertise(service: ServiceInfo) -> Result<Advertiser> {
        let daemon = ServiceDaemon::new().map_err(|e| Error::Io(e.to_string()))?;
        let info = MdnsInfo::new(
            SERVICE_TYPE,
            &service.name,
            &format!("{}.local.", service.name),
            service.addr.ip(),
            service.addr.port(),
            service.metadata,
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

// ======================== 内部辅助 ========================

/// 获取本机 IPv4 地址
///
/// 优先遍历真实网卡（跳过回环与代理工具 fake-IP 段 198.18.0.0/15，如 Surge/Clash
/// 的假 IP 模式会劫持默认路由导致 UDP 探测拿到虚拟地址）；全部失败时回退 UDP 探测。
fn local_ipv4() -> Result<IpAddr> {
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            if let IpAddr::V4(ip) = iface.ip() {
                let o = ip.octets();
                if (o[0] == 198 && (o[1] == 18 || o[1] == 19)) || o[0] == 169 && o[1] == 254 {
                    continue; // fake-IP 段 / 链路本地
                }
                return Ok(IpAddr::V4(ip));
            }
        }
    }
    // 回退：UDP 探测（不实际发包）
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

/// TXT 属性转 HashMap
fn txt_to_map(props: &TxtProperties) -> HashMap<String, String> {
    props
        .iter()
        .map(|p| (p.key().to_string(), p.val_str().to_string()))
        .collect()
}

/// 提取服务列表（每个地址一个 ServiceInfo）
fn services_of(info: &ResolvedService) -> Vec<ServiceInfo> {
    let metadata = txt_to_map(info.get_properties());
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

/// 提取实例名（fullname 形如 name._echostream._udp.local.）
fn instance_name(info: &ResolvedService) -> String {
    info.get_fullname()
        .split('.')
        .next()
        .unwrap_or_default()
        .to_string()
}
