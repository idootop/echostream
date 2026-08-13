//! 无 I/O 客户端状态机（ClientCore）
//!
//! 客户端核心逻辑：RPC id 分配与响应匹配、事件监听注册与分发、
//! 服务端主动 RPC 处理、流序号管理。
//!
//! 不依赖任何网络 I/O 与异步运行时（无 tokio / quinn），
//! 可编译到 WASM 供 Web / 其他语言复用 —— 与 Rust 原生客户端
//! 共享同一份状态机逻辑，避免多端重复实现。

use std::collections::HashMap;

use bytes::Bytes;
use echostream_proto::{
    EventMsg, Message, RequestMsg, ResponseMsg, StatusCode, StreamEndMsg, StreamMsg, Timestamp,
};

/// 事件监听器：接收事件载荷字节（状态机为单线程使用，无 Send 约束）
pub type EventListener = Box<dyn Fn(Bytes)>;

/// RPC 处理器（处理对端主动调用）：返回响应载荷字节；
/// 返回 `None` 表示由调用方异步处理（稍后通过 `build_response` 补响应）
pub type RpcListener = Box<dyn Fn(Bytes) -> Option<Bytes>>;

/// 无 I/O 客户端状态机
#[derive(Default)]
pub struct ClientCore {
    next_id: u64,
    pending: HashMap<u64, EventListener>, // 待响应的 RPC（id → 响应回调）
    events: HashMap<String, Vec<EventListener>>,
    rpcs: HashMap<String, RpcListener>,
    stream_seq: HashMap<u64, u64>, // 流 id → 下一帧序号
}

impl ClientCore {
    /// 创建状态机
    pub fn new() -> Self {
        Self::default()
    }

    /// 发起 RPC：分配 id 并注册响应回调，返回（请求 id、请求帧）
    ///
    /// 响应到达时通过 `handle_inbound` 触发 `on_response` 回调。
    pub fn build_request(
        &mut self,
        name: &str,
        payload: Bytes,
        on_response: impl Fn(Bytes) + 'static,
    ) -> (u64, Message) {
        let id = self.next_id();
        self.pending.insert(id, Box::new(on_response));
        (
            id,
            Message::Request(RequestMsg {
                id,
                name: name.to_string(),
                data: payload,
            }),
        )
    }

    /// 构造数据报事件载荷（裸 postcard Message，无长度前缀；用于不可靠通道）
    pub fn build_datagram_event(&mut self, name: &str, payload: Bytes) -> Vec<u8> {
        let msg = Message::Event(EventMsg {
            id: self.next_id(),
            name: name.to_string(),
            data: payload,
        });
        postcard::to_allocvec(&msg).expect("编码失败")
    }

    /// 构造事件帧（自动分配 id）
    pub fn build_event(&mut self, name: &str, payload: Bytes) -> Message {
        Message::Event(EventMsg {
            id: self.next_id(),
            name: name.to_string(),
            data: payload,
        })
    }

    /// 打开流：分配流 id（后续帧用 `build_stream_frame` 发送）
    pub fn open_stream(&mut self, _name: &str) -> u64 {
        let id = self.next_id();
        self.stream_seq.insert(id, 0);
        id
    }

    /// 构造流数据帧（自动递增序号；时间戳由调用方提供，WASM 环境无系统时钟）
    pub fn build_stream_frame(
        &mut self,
        id: u64,
        name: &str,
        data: Bytes,
        sender_ts: u64,
    ) -> Message {
        let seq = self.stream_seq.entry(id).or_insert(0);
        let frame = Message::Stream(StreamMsg {
            id,
            name: name.to_string(),
            seq: *seq,
            sender_ts: Timestamp(sender_ts),
            data,
        });
        *seq += 1;
        frame
    }

    /// 构造流结束标记（WebSocket 传输的流关闭）
    pub fn build_stream_end(&self, id: u64) -> Message {
        Message::StreamEnd(StreamEndMsg { id })
    }

    /// 构造响应帧（供对端主动调用的异步回复）
    pub fn build_response(&self, id: u64, data: Bytes) -> Message {
        Message::Response(ResponseMsg {
            id,
            code: StatusCode::SUCCESS,
            message: None,
            data,
        })
    }

    /// 构造错误响应帧
    pub fn build_error_response(&self, id: u64, message: &str) -> Message {
        Message::Response(ResponseMsg {
            id,
            code: StatusCode::ERROR,
            message: Some(message.to_string()),
            data: Bytes::new(),
        })
    }

    /// 注册事件监听
    pub fn on_event(&mut self, name: &str, listener: impl Fn(Bytes) + 'static) {
        self.events
            .entry(name.to_string())
            .or_default()
            .push(Box::new(listener));
    }

    /// 注册 RPC 处理器（处理对端主动调用）
    pub fn on_rpc(&mut self, name: &str, handler: impl Fn(Bytes) -> Option<Bytes> + 'static) {
        self.rpcs.insert(name.to_string(), Box::new(handler));
    }

    /// 处理入站消息（网络层收到一帧后调用）
    ///
    /// 返回需要发送给对端的响应帧（对端主动调用且同步处理完成时）。
    pub fn handle_inbound(&mut self, msg: Message) -> Option<Message> {
        match msg {
            Message::Response(resp) => {
                if let Some(cb) = self.pending.remove(&resp.id) {
                    if resp.code.is_success() {
                        cb(resp.data);
                    } else {
                        // 错误响应：回调收到空数据，由调用方通过错误信息处理
                        // （简化：成功时回调数据；失败时回调空并忽略 message）
                        cb(Bytes::new());
                    }
                }
                None
            }
            Message::Event(event) => {
                if let Some(listeners) = self.events.get(&event.name) {
                    for l in listeners {
                        l(event.data.clone());
                    }
                }
                None
            }
            Message::Request(req) => {
                if let Some(handler) = self.rpcs.get(&req.name) {
                    match handler(req.data.clone()) {
                        Some(data) => Some(self.build_response(req.id, data)),
                        None => None, // 异步处理，稍后调用方补响应
                    }
                } else {
                    Some(self.build_error_response(req.id, "handler not found"))
                }
            }
            Message::Stream(_) | Message::StreamEnd(_) => None,
        }
    }

    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}
