//! 无 I/O 客户端状态机（ClientCore）
//!
//! 客户端核心逻辑：RPC id 分配与响应匹配、事件监听注册与分发、
//! 服务端主动 RPC 处理、流序号管理、入站流路由。
//!
//! 不依赖任何网络 I/O 与异步运行时（无 tokio / quinn），
//! 可编译到 WASM 供 Web / 其他语言复用 —— 与 Rust 原生客户端
//! 共享同一份状态机逻辑，避免多端重复实现。

use std::collections::HashMap;

use bytes::Bytes;
use echostream_proto::{
    EventMsg, Message, RequestMsg, ResponseMsg, StatusCode, StreamEndMsg, StreamMsg, Timestamp,
};

/// 监听注册 id（on_* 返回，供 off_* 精确取消注册）
pub type ListenerId = u64;

/// 事件监听器：接收事件载荷字节与事件名（状态机为单线程使用，无 Send 约束）
pub type EventListener = Box<dyn Fn(&str, Bytes)>;

/// RPC 响应回调：参数为（响应载荷字节、错误信息）
pub type ResponseListener = Box<dyn Fn(Bytes, Option<String>)>;

/// RPC 处理器（处理对端主动调用）：参数为（请求 id、载荷字节），返回响应载荷字节；
/// 返回 None 表示由调用方异步处理（稍后通过 build_response 补响应）
pub type RpcListener = Box<dyn Fn(u64, Bytes) -> Option<Bytes>>;

/// 入站流处理器：参数为（流帧载荷；None 表示流结束）
pub type StreamListener = Box<dyn Fn(Option<StreamMsg>)>;

/// 无 I/O 客户端状态机
#[derive(Default)]
pub struct ClientCore {
    next_id: u64,
    next_listener_id: u64,
    pending: HashMap<u64, ResponseListener>, // 待响应的 RPC（id -> 响应回调）
    events: HashMap<String, Vec<(ListenerId, EventListener)>>,
    rpcs: HashMap<String, (ListenerId, RpcListener)>,
    streams: HashMap<String, (ListenerId, StreamListener)>,
    stream_names: HashMap<u64, String>, // 流 id -> 名称（StreamEnd 按 id 路由）
    stream_seq: HashMap<u64, u64>,      // 流 id -> 下一帧序号
}

impl ClientCore {
    /// 创建状态机
    pub fn new() -> Self {
        Self::default()
    }

    /// 发起 RPC：分配 id 并注册响应回调，返回（请求 id、请求帧）
    ///
    /// 响应到达时通过 handle_inbound 触发 on_response 回调；
    /// 错误响应回调（空载荷、错误信息）。
    pub fn build_request(
        &mut self,
        name: &str,
        payload: Bytes,
        on_response: impl Fn(Bytes, Option<String>) + 'static,
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

    /// 打开流：分配流 id（后续帧用 build_stream_frame 发送）
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

    /// 注册事件监听，返回注册 id（off_event 取消注册）
    pub fn on_event(&mut self, name: &str, listener: impl Fn(&str, Bytes) + 'static) -> ListenerId {
        let id = self.next_listener();
        self.events
            .entry(name.to_string())
            .or_default()
            .push((id, Box::new(listener)));
        id
    }

    /// 取消注册事件监听（按注册 id）
    pub fn off_event(&mut self, id: ListenerId) -> bool {
        let mut removed = false;
        for listeners in self.events.values_mut() {
            listeners.retain(|(i, _)| {
                if *i == id {
                    removed = true;
                    false
                } else {
                    true
                }
            });
        }
        self.events.retain(|_, listeners| !listeners.is_empty());
        removed
    }

    /// 注册 RPC 处理器（处理对端主动调用），返回注册 id（off_rpc 取消注册）
    pub fn on_rpc(
        &mut self,
        name: &str,
        handler: impl Fn(u64, Bytes) -> Option<Bytes> + 'static,
    ) -> ListenerId {
        let id = self.next_listener();
        self.rpcs.insert(name.to_string(), (id, Box::new(handler)));
        id
    }

    /// 取消注册 RPC 处理器（按注册 id）
    pub fn off_rpc(&mut self, id: ListenerId) -> bool {
        let mut removed = false;
        self.rpcs.retain(|_, (i, _)| {
            if *i == id {
                removed = true;
                false
            } else {
                true
            }
        });
        removed
    }

    /// 注册入站流处理器（按流名路由；None 表示流结束），返回注册 id（off_stream 取消注册）
    pub fn on_stream(
        &mut self,
        name: &str,
        handler: impl Fn(Option<StreamMsg>) + 'static,
    ) -> ListenerId {
        let id = self.next_listener();
        self.streams
            .insert(name.to_string(), (id, Box::new(handler)));
        id
    }

    /// 取消注册流处理器（按注册 id）
    pub fn off_stream(&mut self, id: ListenerId) -> bool {
        let mut removed = false;
        self.streams.retain(|_, (i, _)| {
            if *i == id {
                removed = true;
                false
            } else {
                true
            }
        });
        removed
    }

    /// 处理入站消息（网络层收到一帧后调用）
    ///
    /// 返回需要发送给对端的响应帧（对端主动调用且同步处理完成时）。
    pub fn handle_inbound(&mut self, msg: Message) -> Option<Message> {
        match msg {
            Message::Response(resp) => {
                if let Some(cb) = self.pending.remove(&resp.id) {
                    if resp.code.is_success() {
                        cb(resp.data, None);
                    } else {
                        cb(Bytes::new(), resp.message);
                    }
                }
                None
            }
            Message::Event(event) => {
                if let Some(listeners) = self.events.get(&event.name) {
                    for (_, l) in listeners {
                        l(&event.name, event.data.clone());
                    }
                }
                None
            }
            Message::Request(req) => {
                if let Some((_, handler)) = self.rpcs.get(&req.name) {
                    match handler(req.id, req.data.clone()) {
                        Some(data) => Some(self.build_response(req.id, data)),
                        None => None, // 异步处理，稍后调用方补响应
                    }
                } else {
                    Some(self.build_error_response(req.id, "handler not found"))
                }
            }
            Message::Stream(frame) => {
                self.stream_names.insert(frame.id, frame.name.clone());
                if let Some((_, handler)) = self.streams.get(&frame.name) {
                    handler(Some(frame));
                }
                None
            }
            Message::StreamEnd(end) => {
                if let Some(name) = self.stream_names.get(&end.id).cloned()
                    && let Some((_, handler)) = self.streams.get(&name)
                {
                    handler(None);
                }
                None
            }
        }
    }

    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn next_listener(&mut self) -> u64 {
        self.next_listener_id += 1;
        self.next_listener_id
    }
}
