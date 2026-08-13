//! 客户端：连接、发起 RPC / 事件 / 流，并处理服务端主动调用

use std::net::ToSocketAddrs;
use std::sync::Arc;

use echostream_proto::Message;
use echostream_transport::{connect, QuicConn};

use crate::context::ServerContext;
use crate::handler::{DynEventHandler, DynRpcHandler, StreamHandler};
use crate::router::Router;
use crate::session::Session;
use crate::stream::StreamSender;

/// 客户端
#[derive(Clone)]
pub struct Client {
    session: Session,
    router: Arc<Router>,
}

impl Client {
    /// 底层会话（可发起双向调用）
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// 发起 RPC 请求
    pub async fn request<Req: serde::Serialize + Send, Resp: serde::de::DeserializeOwned + Send>(
        &self,
        name: &str,
        req: &Req,
    ) -> echostream_proto::Result<Resp> {
        self.session.request(name, req).await
    }

    /// 发送单向事件
    pub async fn emit<T: serde::Serialize + Send>(&self, name: &str, data: &T) -> echostream_proto::Result<()> {
        self.session.emit(name, data).await
    }

    /// 创建流（推送连续数据）
    pub async fn create_stream(&self, name: &str) -> echostream_proto::Result<StreamSender> {
        self.session.create_stream(name).await
    }

    /// 关闭连接
    pub fn close(&self) {
        self.session.close();
    }
}

/// 客户端构建器
pub struct ClientBuilder {
    router: Arc<Router>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientBuilder {
    /// 创建构建器
    pub fn new() -> Self {
        Self {
            router: Arc::new(Router::default()),
        }
    }

    /// 注册 RPC 处理器（处理服务端主动调用）
    pub fn add_rpc<H: DynRpcHandler>(self, handler: H) -> Self {
        self.router.add_rpc(handler);
        self
    }

    /// 注册事件处理器（接收服务端推送的事件）
    pub fn add_event<H: DynEventHandler>(self, handler: H) -> Self {
        self.router.add_event(handler);
        self
    }

    /// 注册流处理器（接收服务端推送的流）
    pub fn add_stream<H: StreamHandler>(self, handler: H) -> Self {
        self.router.add_stream(handler);
        self
    }

    /// 连接到服务端并启动后台接收循环
    pub async fn connect(self, addr: impl ToSocketAddrs) -> echostream_proto::Result<Client> {
        let addr = addr
            .to_socket_addrs()
            .map_err(|e| echostream_proto::Error::Io(e.to_string()))?
            .next()
            .ok_or_else(|| echostream_proto::Error::InvalidParameter("无法解析服务端地址".into()))?;
        let conn: QuicConn = connect(addr).await?;
        let ctx = Arc::new(ServerContext::new());
        let session = Session::new(ctx.next_session_id(), conn, ctx.clone());
        let client = Client {
            session,
            router: self.router,
        };
        tokio::spawn(receive_loop(client.clone()));
        Ok(client)
    }
}

/// 客户端接收循环：处理服务端主动发来的 RPC / 事件 / 流
async fn receive_loop(client: Client) {
    let conn = client.session.conn().clone();
    loop {
        tokio::select! {
            bi = conn.accept_bi() => {
                match bi {
                    Ok(mut stream) => match stream.read_message().await {
                        Ok(Some(Message::Request(req))) => {
                            client.router.dispatch_rpc(client.session(), &mut stream, req).await;
                            let _ = stream.finish();
                        }
                        Ok(Some(Message::Stream(frame))) => {
                            let (_, recv) = stream.split();
                            client.router.dispatch_stream(client.session(), recv, frame).await;
                        }
                        Ok(Some(_)) => { /* 忽略不支持的帧类型 */ }
                        Ok(None) | Err(_) => break,
                    },
                    Err(_) => break,
                }
            }
            uni = conn.accept_uni() => {
                match uni {
                    Ok(mut recv) => match recv.read_message().await {
                        Ok(Some(Message::Event(event))) => {
                            client.router.dispatch_event(client.session(), event).await;
                        }
                        Ok(Some(Message::Stream(frame))) => {
                            client.router.dispatch_stream(client.session(), recv, frame).await;
                        }
                        Ok(Some(_)) => { /* 忽略不支持的帧类型 */ }
                        Ok(None) | Err(_) => break,
                    },
                    Err(_) => break,
                }
            }
        }
    }
    tracing::debug!("连接已断开");
}
