//! EchoStream 心跳保活插件
//!
//! 客户端周期发送心跳事件，服务端记录最近心跳并清理失活会话（防半开连接泄漏）。
//!
//! 服务端：
//!   ServerBuilder::new().plugin(HeartbeatServerPlugin::new(Duration::from_secs(15)));
//! 客户端：
//!   ClientBuilder::new().plugin(HeartbeatClientPlugin::new(Duration::from_secs(5)));

use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use echostream_core::{
    ClientBuilder, ClientPlugin, Middleware, Next, ServerBuilder, ServerContext, ServerPlugin, Session,
};
use echostream_proto::{Error, Message, Result};

/// 心跳事件名（客户端发送、服务端识别）
pub const HEARTBEAT_EVENT: &str = "__echostream.heartbeat";

/// 会话最近心跳时间键
const LAST_SEEN_KEY: &str = "__echostream_last_seen";

// ======================== 服务端 ========================

/// 服务端心跳插件：记录最近心跳，超时未心跳的会话被强制断开
pub struct HeartbeatServerPlugin {
    /// 心跳超时（超过该时长未收到心跳视为失活）
    timeout: Duration,
    /// 失活扫描间隔
    interval: Duration,
}

impl HeartbeatServerPlugin {
    /// 创建服务端心跳插件（timeout 为失活判定阈值，默认 3 * timeout 为扫描间隔）
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            interval: timeout / 3,
        }
    }

    /// 自定义失活扫描间隔
    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
}

/// 服务端心跳中间件：记录心跳时间（失活判定由扫描任务负责）
struct HeartbeatMiddleware;

#[async_trait]
impl Middleware for HeartbeatMiddleware {
    fn name(&self) -> &str {
        "heartbeat"
    }

    async fn handle(&self, session: &Session, msg: Message, next: Next) -> Result<Option<Message>> {
        if matches!(&msg, Message::Event(e) if e.name == HEARTBEAT_EVENT) {
            session.set(LAST_SEEN_KEY, Instant::now());
            return Ok(None); // 心跳事件不向业务分发
        }
        next.run(msg).await
    }

    async fn on_connect(&self, session: &Session) -> Result<()> {
        session.set(LAST_SEEN_KEY, Instant::now());
        Ok(())
    }
}

impl ServerPlugin for HeartbeatServerPlugin {
    fn name(&self) -> &str {
        "heartbeat"
    }

    fn install(self: Box<Self>, builder: ServerBuilder) -> ServerBuilder {
        let timeout = self.timeout;
        let interval = self.interval;
        builder
            .middleware(HeartbeatMiddleware)
            .on_start(move |ctx: &ServerContext| {
                let ctx = ctx.clone();
                // 失活扫描任务：定期断开超时未心跳的会话
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(interval).await;
                        let now = Instant::now();
                        for session in ctx.sessions() {
                            let stale = match session.get::<Instant>(LAST_SEEN_KEY) {
                                Some(last) => now.duration_since(*last) > timeout,
                                None => true, // 无心跳记录（非心跳协议客户端）
                            };
                            if stale {
                                tracing::warn!(session = session.id(), "心跳超时，断开失活会话");
                                session.close();
                                ctx.unregister_session(session.id());
                            }
                        }
                    }
                });
            })
    }
}

// ======================== 客户端 ========================

/// 客户端心跳插件：周期发送心跳事件
pub struct HeartbeatClientPlugin {
    /// 心跳发送间隔
    interval: Duration,
}

impl HeartbeatClientPlugin {
    /// 创建客户端心跳插件（建议小于服务端超时的 1/3）
    pub fn new(interval: Duration) -> Self {
        Self { interval }
    }
}

impl ClientPlugin for HeartbeatClientPlugin {
    fn name(&self) -> &str {
        "heartbeat"
    }

    fn install(self: Box<Self>, builder: ClientBuilder) -> ClientBuilder {
        let interval = self.interval;
        builder.on_connect(move |client: &echostream_core::Client| {
            let client = client.clone();
            tokio::spawn(async move {
                loop {
                    // 连接断开后停止心跳（发送失败即退出）
                    if client.session().emit_raw(HEARTBEAT_EVENT, Bytes::new()).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(interval).await;
                    if client.is_closed() {
                        return;
                    }
                }
            });
        })
    }
}

/// 客户端辅助：手动发送一次心跳（非插件场景）
pub async fn send_heartbeat(session: &Session) -> Result<()> {
    session
        .emit_raw(HEARTBEAT_EVENT, Bytes::new())
        .await
        .map_err(|e| Error::Io(e.to_string()))
}
