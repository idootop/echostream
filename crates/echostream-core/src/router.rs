//! 处理器注册表：RPC / Event / Stream 路由与分发

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use echostream_proto::{Error, EventMsg, Message, RequestMsg, ResponseMsg, StatusCode, StreamMsg};
use echostream_transport::{BiStream, UniRecv};

use crate::handler::{DynEventHandler, DynRpcHandler, StreamHandler};
use crate::session::Session;
use crate::stream::StreamReceiver;

/// 处理器注册表（线程安全，支持运行时注册/移除）
#[derive(Default)]
pub struct Router {
    rpc: RwLock<HashMap<String, Arc<dyn DynRpcHandler>>>,
    event: RwLock<HashMap<String, Vec<Arc<dyn DynEventHandler>>>>,
    stream: RwLock<HashMap<String, Arc<dyn StreamHandler>>>,
}

impl Router {
    /// 注册 RPC 处理器
    pub fn add_rpc<H: DynRpcHandler>(&self, handler: H) {
        self.rpc
            .write()
            .unwrap()
            .insert(handler.name().to_string(), Arc::new(handler));
    }

    /// 注册事件处理器（同名事件支持多个监听器，按注册顺序执行）
    pub fn add_event<H: DynEventHandler>(&self, handler: H) {
        self.event
            .write()
            .unwrap()
            .entry(handler.name().to_string())
            .or_default()
            .push(Arc::new(handler));
    }

    /// 注册流处理器
    pub fn add_stream<H: StreamHandler>(&self, handler: H) {
        self.stream
            .write()
            .unwrap()
            .insert(handler.name().to_string(), Arc::new(handler));
    }

    /// 移除 RPC 处理器
    pub fn remove_rpc(&self, name: &str) {
        self.rpc.write().unwrap().remove(name);
    }

    /// 分派 RPC 请求（在同一双向流上写回响应）
    pub async fn dispatch_rpc(&self, session: &Session, stream: &mut BiStream, msg: RequestMsg) {
        let handler = self.rpc.read().unwrap().get(&msg.name).cloned();
        let result = match handler {
            Some(handler) => handler.handle_encoded(session, msg.data.clone()).await,
            None => Err(Error::HandlerNotFound(msg.name.clone())),
        };
        let response = match result {
            Ok(data) => ResponseMsg {
                id: msg.id,
                code: StatusCode::SUCCESS,
                message: None,
                data,
            },
            Err(e) => ResponseMsg {
                id: msg.id,
                code: match &e {
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

    /// 分派事件（单向流）
    pub async fn dispatch_event(&self, session: &Session, msg: EventMsg) {
        let handlers = self.event.read().unwrap().get(&msg.name).cloned();
        if let Some(handlers) = handlers {
            for handler in handlers {
                if let Err(e) = handler.handle_encoded(session, msg.data.clone()).await {
                    tracing::debug!("事件处理器出错 ({}) : {e}", msg.name);
                }
            }
        }
    }

    /// 分派流（持续读取直到结束）
    pub async fn dispatch_stream(&self, session: &Session, recv: UniRecv, msg: StreamMsg) {
        let name = msg.name.clone();
        let handler = self.stream.read().unwrap().get(&name).cloned();
        match handler {
            Some(handler) => {
                let receiver = StreamReceiver::new(recv, msg);
                if let Err(e) = handler.handle(session, receiver).await {
                    tracing::debug!("流处理器出错 ({name}) : {e}");
                }
            }
            None => tracing::debug!("未找到流处理器: {name}"),
        }
    }
}
