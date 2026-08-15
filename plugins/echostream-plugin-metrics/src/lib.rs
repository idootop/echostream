//! EchoStream 指标插件
//!
//! 请求统计与性能指标收集：中间件记录 RPC / Event / Stream 计数与延迟，
//! 连接钩子记录会话数；通过内置 RPC（默认 metrics.snapshot）随时查询快照。
//!
//! 用法：ServerBuilder::new().plugin(MetricsPlugin::new());
//! 查询：client.request::<MetricsSnapshot>("metrics.snapshot", &())

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use async_trait::async_trait;
use echostream_core::{Middleware, Next, ServerBuilder, ServerPlugin, Session};
use echostream_proto::{Message, Result};
use serde::{Deserialize, Serialize};

/// 指标快照（postcard 序列化，可经 RPC 返回客户端）
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// 各 RPC 方法统计（按方法名排序）
    pub rpc: Vec<(String, RpcStats)>,
    /// 事件总数
    pub events_total: u64,
    /// 入站流总数
    pub streams_total: u64,
    /// 累计连接数
    pub connects_total: u64,
    /// 累计断开数
    pub disconnects_total: u64,
    /// 当前在线会话数
    pub active_sessions: usize,
}

/// 单个 RPC 方法的统计
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RpcStats {
    /// 调用次数
    pub calls: u64,
    /// 错误次数
    pub errors: u64,
    /// 累计延迟（微秒；平均延迟 = latency_us_sum / calls）
    pub latency_us_sum: u64,
}

/// 指标注册表（Clone 共享）
#[derive(Clone, Default)]
pub struct MetricsRegistry {
    rpc: Arc<std::sync::Mutex<HashMap<String, RpcStats>>>,
    events_total: Arc<AtomicU64>,
    streams_total: Arc<AtomicU64>,
    connects_total: Arc<AtomicU64>,
    disconnects_total: Arc<AtomicU64>,
}

impl MetricsRegistry {
    /// 采集当前快照（active_sessions 需传入服务端上下文会话数）
    pub fn snapshot(&self, active_sessions: usize) -> MetricsSnapshot {
        let mut rpc: Vec<(String, RpcStats)> = self
            .rpc
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        rpc.sort_by(|a, b| a.0.cmp(&b.0));
        MetricsSnapshot {
            rpc,
            events_total: self.events_total.load(Ordering::Relaxed),
            streams_total: self.streams_total.load(Ordering::Relaxed),
            connects_total: self.connects_total.load(Ordering::Relaxed),
            disconnects_total: self.disconnects_total.load(Ordering::Relaxed),
            active_sessions,
        }
    }
}

/// 指标插件：安装统计中间件 + 快照查询 RPC
pub struct MetricsPlugin {
    registry: Arc<MetricsRegistry>,
    /// 快照查询 RPC 名（默认 "metrics.snapshot"）
    rpc_name: String,
}

impl MetricsPlugin {
    /// 创建指标插件（快照 RPC 默认 metrics.snapshot）
    pub fn new() -> Self {
        Self {
            registry: Arc::new(MetricsRegistry::default()),
            rpc_name: "metrics.snapshot".to_string(),
        }
    }

    /// 自定义快照查询 RPC 名
    pub fn rpc_name(mut self, name: impl Into<String>) -> Self {
        self.rpc_name = name.into();
        self
    }

    /// 共享的指标注册表（供外部读取 / 上报）
    pub fn registry(&self) -> Arc<MetricsRegistry> {
        self.registry.clone()
    }
}

impl Default for MetricsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerPlugin for MetricsPlugin {
    fn name(&self) -> &str {
        "metrics"
    }

    fn install(self: Box<Self>, builder: ServerBuilder) -> ServerBuilder {
        let registry = self.registry.clone();
        let rpc_name = self.rpc_name.clone();
        builder
            .middleware(MetricsMiddleware {
                registry: registry.clone(),
            })
            .add_rpc(MetricsRpc { registry, rpc_name })
    }
}

/// 指标中间件：记录消息计数与延迟
struct MetricsMiddleware {
    registry: Arc<MetricsRegistry>,
}

#[async_trait]
impl Middleware for MetricsMiddleware {
    fn name(&self) -> &str {
        "metrics"
    }

    async fn handle(&self, _session: &Session, msg: Message, next: Next) -> Result<Option<Message>> {
        // 先按消息类型分类（避免借用冲突），再执行链
        let rpc_name = match &msg {
            Message::Request(r) => Some(r.name.clone()),
            _ => None,
        };
        let is_event = matches!(&msg, Message::Event(_));
        let is_stream = matches!(&msg, Message::Stream(_));
        let start = Instant::now();
        let result = next.run(msg).await;
        let elapsed = start.elapsed().as_micros() as u64;
        if let Some(name) = rpc_name {
            let mut guard = self.registry.rpc.lock().unwrap();
            let stat = guard.entry(name).or_default();
            stat.calls += 1;
            stat.latency_us_sum += elapsed;
            if result.as_ref().is_err() {
                stat.errors += 1;
            }
            drop(guard);
        }
        if is_event {
            self.registry.events_total.fetch_add(1, Ordering::Relaxed);
        }
        if is_stream {
            self.registry.streams_total.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    async fn on_connect(&self, _session: &Session) -> Result<()> {
        self.registry.connects_total.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn on_disconnect(&self, _session: &Session) -> Result<()> {
        self.registry
            .disconnects_total
            .fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// 快照查询 RPC
struct MetricsRpc {
    registry: Arc<MetricsRegistry>,
    rpc_name: String,
}

#[async_trait]
impl echostream_core::RpcHandler for MetricsRpc {
    type Req = ();
    type Resp = MetricsSnapshot;

    fn name(&self) -> &str {
        &self.rpc_name
    }

    async fn handle(&self, session: &Session, _req: ()) -> Result<MetricsSnapshot> {
        Ok(self.registry.snapshot(session.ctx().sessions().len()))
    }
}
