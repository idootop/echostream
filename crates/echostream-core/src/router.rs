//! 处理器注册表：RPC / Event / Stream 路由与分发
//!
//! 所有入站消息先过中间件链（洋葱模型，见 `middleware` 模块），
//! 链的终点是路由分派；连接生命周期钩子（on_connect / on_disconnect）
//! 在此统一触发各中间件。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use echostream_proto::endpoint::FrameIo;
use echostream_proto::{Error, EventMsg, Message, RequestMsg, ResponseMsg, StatusCode, StreamMsg};
use futures::future::BoxFuture;

use crate::handler::{DynEventHandler, DynRpcHandler, StreamHandler};
use crate::middleware::{Middleware, Next};
use crate::session::Session;
use crate::stream::StreamReceiver;

/// 中间件链终点：接收（可能被中间件修改过的）消息，执行实际处理，
/// 返回最终结果（RPC 为响应帧；事件 / 流为 None 表示已消费）
type Terminal =
    Arc<dyn Fn(Message) -> BoxFuture<'static, Result<Option<Message>, Error>> + Send + Sync>;

/// 注册 token（add_* 返回，供 remove_* 精确移除对应注册项）
pub type Token = u64;

/// 处理器注册表（线程安全，支持运行时注册 / 按 token 移除 / 注册表查询）
#[derive(Clone, Default)]
pub struct Router {
    inner: Arc<RouterInner>,
}

/// RPC 注册项（token + 处理器）
type RpcEntry = (Token, Arc<dyn DynRpcHandler>);
/// 事件监听注册项
type EventEntry = (Token, Arc<dyn DynEventHandler>);
/// 流注册项
type StreamEntry = (Token, Arc<dyn StreamHandler>);
/// 中间件注册项
type MiddlewareEntry = (Token, Arc<dyn Middleware>);

#[derive(Default)]
struct RouterInner {
    rpc: RwLock<HashMap<String, RpcEntry>>,
    event: RwLock<HashMap<String, Vec<EventEntry>>>,
    stream: RwLock<HashMap<String, StreamEntry>>,
    middlewares: RwLock<Vec<MiddlewareEntry>>,
    next_token: AtomicU64,
}

impl Router {
    /// 注册 RPC 处理器，返回注册 token（remove_rpc 精确移除）
    pub fn add_rpc<H: DynRpcHandler>(&self, handler: H) -> Token {
        let token = self.inner.next_token.fetch_add(1, Ordering::Relaxed);
        self.inner
            .rpc
            .write()
            .unwrap()
            .insert(handler.name().to_string(), (token, Arc::new(handler)));
        token
    }

    /// 注册事件处理器（同名事件支持多个监听器，按注册顺序执行），返回注册 token
    pub fn add_event<H: DynEventHandler>(&self, handler: H) -> Token {
        let token = self.inner.next_token.fetch_add(1, Ordering::Relaxed);
        self.inner
            .event
            .write()
            .unwrap()
            .entry(handler.name().to_string())
            .or_default()
            .push((token, Arc::new(handler)));
        token
    }

    /// 注册流处理器，返回注册 token
    pub fn add_stream<H: StreamHandler>(&self, handler: H) -> Token {
        let token = self.inner.next_token.fetch_add(1, Ordering::Relaxed);
        self.inner
            .stream
            .write()
            .unwrap()
            .insert(handler.name().to_string(), (token, Arc::new(handler)));
        token
    }

    /// 添加中间件（按添加顺序执行；洋葱链），返回注册 token
    pub fn add_middleware<M: Middleware>(&self, middleware: M) -> Token {
        let token = self.inner.next_token.fetch_add(1, Ordering::Relaxed);
        self.inner
            .middlewares
            .write()
            .unwrap()
            .push((token, Arc::new(middleware)));
        token
    }

    // ==================== 批量注册 ====================

    /// 批量注册 RPC 处理器，返回各注册 token
    pub fn add_rpcs<H: DynRpcHandler>(&self, handlers: impl IntoIterator<Item = H>) -> Vec<Token> {
        handlers.into_iter().map(|h| self.add_rpc(h)).collect()
    }

    /// 批量注册事件监听器，返回各注册 token
    pub fn add_events<H: DynEventHandler>(
        &self,
        handlers: impl IntoIterator<Item = H>,
    ) -> Vec<Token> {
        handlers.into_iter().map(|h| self.add_event(h)).collect()
    }

    /// 批量注册流处理器，返回各注册 token
    pub fn add_streams<H: StreamHandler>(
        &self,
        handlers: impl IntoIterator<Item = H>,
    ) -> Vec<Token> {
        handlers.into_iter().map(|h| self.add_stream(h)).collect()
    }

    /// 移除 RPC 处理器（按注册 token），返回是否移除成功
    pub fn remove_rpc(&self, token: Token) -> bool {
        let removed = self
            .inner
            .rpc
            .read()
            .unwrap()
            .values()
            .any(|(t, _)| *t == token);
        self.inner
            .rpc
            .write()
            .unwrap()
            .retain(|_, (t, _)| *t != token);
        removed
    }

    /// 移除事件监听器（按注册 token）
    pub fn remove_event(&self, token: Token) -> bool {
        let mut found = false;
        let mut guard = self.inner.event.write().unwrap();
        for listeners in guard.values_mut() {
            listeners.retain(|(t, _)| {
                if *t == token {
                    found = true;
                    false
                } else {
                    true
                }
            });
        }
        guard.retain(|_, listeners| !listeners.is_empty());
        found
    }

    /// 移除流处理器（按注册 token），返回是否移除成功
    pub fn remove_stream(&self, token: Token) -> bool {
        let removed = self
            .inner
            .stream
            .read()
            .unwrap()
            .values()
            .any(|(t, _)| *t == token);
        self.inner
            .stream
            .write()
            .unwrap()
            .retain(|_, (t, _)| *t != token);
        removed
    }

    /// 移除中间件（按注册 token）
    pub fn remove_middleware(&self, token: Token) -> bool {
        let mut found = false;
        self.inner.middlewares.write().unwrap().retain(|(t, _)| {
            if *t == token {
                found = true;
                false
            } else {
                true
            }
        });
        found
    }

    // ==================== 注册表查询 ====================

    /// 已注册的 RPC 方法名列表
    pub fn rpc_names(&self) -> Vec<String> {
        self.inner.rpc.read().unwrap().keys().cloned().collect()
    }

    /// 已注册的事件名列表
    pub fn event_names(&self) -> Vec<String> {
        self.inner.event.read().unwrap().keys().cloned().collect()
    }

    /// 已注册的流名列表
    pub fn stream_names(&self) -> Vec<String> {
        self.inner.stream.read().unwrap().keys().cloned().collect()
    }

    /// 已注册的中间件名称列表（按执行顺序）
    pub fn middleware_names(&self) -> Vec<String> {
        self.inner
            .middlewares
            .read()
            .unwrap()
            .iter()
            .map(|(_, m)| m.name().to_string())
            .collect()
    }

    /// 获取 RPC 处理器（供非流式传输分派）
    pub fn get_rpc(&self, name: &str) -> Option<Arc<dyn DynRpcHandler>> {
        self.inner
            .rpc
            .read()
            .unwrap()
            .get(name)
            .map(|(_, h)| h.clone())
    }

    /// 是否存在流处理器
    pub fn has_stream(&self, name: &str) -> bool {
        self.inner.stream.read().unwrap().contains_key(name)
    }

    // ==================== 中间件链 ====================

    /// 运行中间件链（洋葱模型），终点为 `terminal`
    ///
    /// 返回语义：`Ok(Some(msg))` 链结果（终端响应或中间件短路值）；
    /// `Ok(None)` 被拦截；`Err` 链中错误（传播给上层处理）。
    async fn run_chain(
        &self,
        session: &Session,
        msg: Message,
        terminal: Terminal,
    ) -> Result<Option<Message>, Error> {
        let chain = Arc::new(
            self.inner
                .middlewares
                .read()
                .unwrap()
                .iter()
                .map(|(_, m)| m.clone())
                .collect::<Vec<_>>(),
        );
        let next = Next {
            chain,
            session: session.clone(),
            idx: 0,
            terminal,
        };
        next.run(msg).await
    }

    /// 触发所有中间件的连接建立钩子
    pub async fn run_connect_hooks(&self, session: &Session) {
        let mws = self.inner.middlewares.read().unwrap().clone();
        for (_, mw) in mws {
            if let Err(e) = mw.on_connect(session).await {
                tracing::debug!("中间件 {} on_connect 出错: {e}", mw.name());
            }
        }
    }

    /// 触发所有中间件的连接断开钩子
    pub async fn run_disconnect_hooks(&self, session: &Session) {
        let mws = self.inner.middlewares.read().unwrap().clone();
        for (_, mw) in mws {
            if let Err(e) = mw.on_disconnect(session).await {
                tracing::debug!("中间件 {} on_disconnect 出错: {e}", mw.name());
            }
        }
    }

    // ==================== 分派 ====================

    /// 分派 RPC 请求（在同一双向流上写回响应）
    pub async fn dispatch_rpc(&self, session: &Session, stream: &mut dyn FrameIo, msg: RequestMsg) {
        let this = self.clone();
        let session2 = session.clone();
        let terminal: Terminal = Arc::new(move |req_msg: Message| {
            let this = this.clone();
            let session = session2.clone();
            Box::pin(async move {
                let Message::Request(m) = req_msg else {
                    return Ok(Some(req_msg));
                };
                // 按（可能被中间件修改的）方法名查找处理器
                let handler = this
                    .inner
                    .rpc
                    .read()
                    .unwrap()
                    .get(&m.name)
                    .map(|(_, h)| h.clone());
                let result = match handler {
                    Some(handler) => handler.handle_encoded(&session, m.data.clone()).await,
                    None => Err(Error::HandlerNotFound(m.name.clone())),
                };
                match result {
                    Ok(data) => Ok(Some(Message::Response(ResponseMsg {
                        id: m.id,
                        code: StatusCode::SUCCESS,
                        message: None,
                        data,
                    }))),
                    Err(e) => Err(e),
                }
            })
        });
        let result = self
            .run_chain(session, Message::Request(msg.clone()), terminal)
            .await;
        // 写回响应：链结果（Response / 短路值）或错误映射
        let response = match result {
            Ok(Some(Message::Response(r))) => r,
            Ok(Some(_)) | Ok(None) => ResponseMsg {
                id: msg.id,
                code: StatusCode::FORBIDDEN,
                message: Some("请求被中间件拦截".into()),
                data: Bytes::new(),
            },
            Err(e) => ResponseMsg {
                id: msg.id,
                code: match &e {
                    // 业务错误码透传
                    Error::Rpc(code, _) => StatusCode::new(*code),
                    Error::HandlerNotFound(_) => StatusCode::NOT_FOUND,
                    Error::Timeout(_) => StatusCode::TIMEOUT,
                    _ => StatusCode::ERROR,
                },
                message: Some(e.to_string()),
                data: Bytes::new(),
            },
        };
        if let Err(e) = stream.write_message(&Message::Response(response)).await {
            tracing::debug!("写响应失败: {e}");
        }
    }

    /// 分派数据报（不可靠通道：事件）
    pub async fn dispatch_inbound_datagram(&self, session: &Session, msg: Message) {
        if let Message::Event(event) = msg {
            self.dispatch_event(session, event).await
        }
    }

    /// 分派事件（单向流）
    pub async fn dispatch_event(&self, session: &Session, msg: EventMsg) {
        let this = self.clone();
        let session2 = session.clone();
        let terminal: Terminal = Arc::new(move |evt_msg: Message| {
            let this = this.clone();
            let session = session2.clone();
            Box::pin(async move {
                let Message::Event(e) = evt_msg else {
                    return Ok(Some(evt_msg));
                };
                // 按（可能被中间件修改的）事件名查找监听器
                let handlers = this
                    .inner
                    .event
                    .read()
                    .unwrap()
                    .get(&e.name)
                    .map(|listeners| listeners.iter().map(|(_, h)| h.clone()).collect::<Vec<_>>());
                if let Some(handlers) = handlers {
                    for handler in handlers {
                        if let Err(err) = handler.handle_encoded(&session, e.data.clone()).await {
                            tracing::debug!("事件处理器出错 ({}) : {err}", e.name);
                        }
                    }
                }
                Ok(Some(Message::Event(e))) // 已消费（回传消息供中间件观测）
            })
        });
        match self
            .run_chain(session, Message::Event(msg.clone()), terminal)
            .await
        {
            Ok(Some(Message::Event(_))) => {} // 已分发
            Ok(None) => tracing::debug!("事件被中间件拦截: {}", msg.name),
            Ok(Some(m)) => tracing::debug!("事件中间件链未消费消息: {m:?}"),
            Err(e) => tracing::debug!("事件中间件链出错: {e}"),
        }
    }

    /// 分派流（中间件链后持续读取直到结束）
    pub async fn dispatch_stream(&self, session: &Session, recv: Box<dyn FrameIo>, msg: StreamMsg) {
        let this = self.clone();
        let session2 = session.clone();
        // 接收器由终端独占消费（中间件至多调用一次 next；Arc 共享避免借用逃逸）
        let recv: Arc<std::sync::Mutex<Option<Box<dyn FrameIo>>>> =
            Arc::new(std::sync::Mutex::new(Some(recv)));
        let terminal: Terminal = Arc::new(move |stream_msg: Message| {
            let this = this.clone();
            let session = session2.clone();
            let recv = recv.clone();
            Box::pin(async move {
                let Message::Stream(frame) = stream_msg else {
                    return Ok(Some(stream_msg));
                };
                let name = frame.name.clone();
                let Some(recv) = recv.lock().unwrap().take() else {
                    return Err(Error::Protocol("流处理器被重复调用".into()));
                };
                let handler = this
                    .inner
                    .stream
                    .read()
                    .unwrap()
                    .get(&name)
                    .map(|(_, h)| h.clone());
                match handler {
                    Some(handler) => {
                        let receiver = StreamReceiver::new(recv, frame.clone());
                        handler.handle(&session, receiver).await?;
                        Ok(Some(Message::Stream(frame))) // 已消费（回传首帧供中间件观测）
                    }
                    None => {
                        tracing::debug!("未找到流处理器: {name}");
                        Ok(Some(Message::Stream(frame)))
                    }
                }
            })
        });
        let stream_name = msg.name.clone();
        match self
            .run_chain(session, Message::Stream(msg), terminal)
            .await
        {
            Ok(Some(Message::Stream(_))) => {} // 已处理（或被未调用 next 的中间件短路）
            Ok(None) => tracing::debug!("流被中间件拦截: {stream_name}"),
            Ok(Some(m)) => tracing::debug!("流中间件链未消费消息: {m:?}"),
            Err(e) => tracing::debug!("流中间件链出错: {e}"),
        }
    }
}
