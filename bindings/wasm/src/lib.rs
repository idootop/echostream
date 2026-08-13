//! EchoStream WASM binding
//!
//! 将 Rust 侧的协议编解码编译为 WASM，供浏览器/Node 复用：
//! - `encode_payload`：JS 值 → postcard 载荷字节
//! - `encode_message` / `decode_message`：Message 编解码
//! - `encode_frame`：帧（长度前缀 + 消息）
//!
//! 单一事实来源：与 Rust 服务端的线缆格式天然一致，无需跨语言重实现。

use bytes::Bytes;
use echostream_proto::{
    EventMsg, Message, RequestMsg, ResponseMsg, StatusCode, StreamEndMsg, StreamMsg, Timestamp,
};
use js_sys::{Array, BigInt, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

// ======================== 载荷编码 ========================

/// 编码载荷：JS 值 → postcard 字节
///
/// 支持：number（非负整数 → u64 varint，负数 → i64 zigzag）、bigint、
/// string（长度前缀 + UTF-8）、Uint8Array（长度前缀 + 字节）、
/// Array（按 Rust 元组/结构体字段顺序编码，无长度前缀）、
/// Object（字段按插入序编码，等价结构体字段序）。
#[wasm_bindgen]
pub fn encode_payload(value: JsValue) -> Result<Vec<u8>, JsValue> {
    let mut w = Writer::default();
    w.value(&value)?;
    Ok(w.bytes)
}

// ======================== 消息编解码 ========================

/// 编码消息：JS 对象 → postcard 字节
///
/// 输入：`{ type, id, name, data, ... }`
/// - request/event：`{ type, id, name, data: Uint8Array }`
/// - response：`{ type, id, code, message?, data }`
/// - stream：`{ type, id, name, seq, senderTs, data }`
#[wasm_bindgen]
pub fn encode_message(msg: JsValue) -> Result<Vec<u8>, JsValue> {
    let msg = js_to_message(&msg)?;
    postcard::to_allocvec(&msg).map_err(|e| js_err(&format!("编码失败: {e}")))
}

/// 解码消息：postcard 字节 → JS 对象
#[wasm_bindgen]
pub fn decode_message(bytes: &[u8]) -> Result<JsValue, JsValue> {
    let msg: Message =
        postcard::from_bytes(bytes).map_err(|e| js_err(&format!("解码失败: {e}")))?;
    message_to_js(&msg)
}

/// 编码帧：4 字节小端长度前缀 + 消息载荷
#[wasm_bindgen]
pub fn encode_frame(msg: JsValue) -> Result<Vec<u8>, JsValue> {
    let payload = encode_message(msg)?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

// ======================== 载荷解码原语 ========================

/// 解码 u64（varint）
#[wasm_bindgen]
pub fn decode_u64(bytes: &[u8]) -> Result<f64, JsValue> {
    let mut r = Reader::new(bytes);
    let v = r.varint()?;
    Ok(v as f64)
}

/// 解码 string
#[wasm_bindgen]
pub fn decode_string(bytes: &[u8]) -> Result<String, JsValue> {
    let mut r = Reader::new(bytes);
    r.string()
}

/// 解码 bytes（长度前缀 + 字节）
#[wasm_bindgen]
pub fn decode_bytes(bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut r = Reader::new(bytes);
    r.bytes()
}

// ======================== 编码器 ========================

#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn varint(&mut self, mut n: u64) {
        while n >= 0x80 {
            self.bytes.push((n as u8 & 0x7f) | 0x80);
            n >>= 7;
        }
        self.bytes.push(n as u8);
    }

    fn put_bytes(&mut self, data: &[u8]) {
        self.varint(data.len() as u64);
        self.bytes.extend_from_slice(data);
    }

    fn string(&mut self, s: &str) {
        self.put_bytes(s.as_bytes());
    }

    fn value(&mut self, v: &JsValue) -> Result<(), JsValue> {
        if let Some(n) = v.as_f64() {
            // number：整数且安全范围内
            if !n.is_finite() || n.fract() != 0.0 {
                return Err(js_err("载荷 number 必须是整数"));
            }
            if n >= 0.0 {
                self.varint(n as u64);
            } else {
                // zigzag（Rust 有符号整数）
                self.varint((-n as i64 * 2 - 1) as u64);
            }
            return Ok(());
        }
        if let Some(b) = v.as_bool() {
            self.bytes.push(b as u8);
            return Ok(());
        }
        if let Some(s) = v.as_string() {
            self.string(&s);
            return Ok(());
        }
        if v.is_bigint() {
            let big = BigInt::from(v.clone());
            let s = big
                .to_string(10)
                .map_err(|_| js_err("bigint 转换失败"))?
                .as_string()
                .unwrap_or_default();
            let n: i128 = s.parse().map_err(|_| js_err("bigint 解析失败"))?;
            if n >= 0 {
                self.varint(n as u64);
            } else {
                self.varint(((-n) * 2 - 1) as u64);
            }
            return Ok(());
        }
        if let Some(arr) = v.dyn_ref::<Uint8Array>() {
            let data = arr.to_vec();
            self.put_bytes(&data);
            return Ok(());
        }
        if let Some(arr) = v.dyn_ref::<Array>() {
            for item in arr.iter() {
                self.value(&item)?;
            }
            return Ok(());
        }
        if v.is_object() {
            let obj = Object::from(v.clone());
            let keys =
                Reflect::own_keys(&obj).map_err(|e| js_err(&format!("对象键获取失败: {e:?}")))?;
            for i in 0..keys.length() {
                let key = keys.get(i);
                let key_str = key.as_string().ok_or_else(|| js_err("对象键非字符串"))?;
                let val = Reflect::get(&obj, &key)
                    .map_err(|e| js_err(&format!("字段读取失败: {e:?}")))?;
                self.string(&key_str);
                self.value(&val)?;
            }
            return Ok(());
        }
        Err(js_err("不支持的载荷类型"))
    }
}

// ======================== 解码器 ========================

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn varint(&mut self) -> Result<u64, JsValue> {
        let mut result: u64 = 0;
        let mut shift = 0;
        loop {
            let b = *self
                .bytes
                .get(self.pos)
                .ok_or_else(|| js_err("varint 越界"))?;
            self.pos += 1;
            result |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 63 {
                return Err(js_err("varint 溢出"));
            }
        }
        Ok(result)
    }

    fn bytes(&mut self) -> Result<Vec<u8>, JsValue> {
        let len = self.varint()? as usize;
        let end = self.pos + len;
        if end > self.bytes.len() {
            return Err(js_err("字节数据越界"));
        }
        let out = self.bytes[self.pos..end].to_vec();
        self.pos = end;
        Ok(out)
    }

    fn string(&mut self) -> Result<String, JsValue> {
        let data = self.bytes()?;
        String::from_utf8(data).map_err(|e| js_err(&format!("UTF-8 解码失败: {e}")))
    }
}

// ======================== JS 值 ↔ Message ========================

fn js_to_message(v: &JsValue) -> Result<Message, JsValue> {
    let obj = Object::from(v.clone());
    let ty = get_str(&obj, "type")?;
    let id = get_u64(&obj, "id")?;
    match ty.as_str() {
        "request" => Ok(Message::Request(RequestMsg {
            id,
            name: get_str(&obj, "name")?,
            data: get_bytes(&obj, "data")?,
        })),
        "response" => Ok(Message::Response(ResponseMsg {
            id,
            code: StatusCode(get_u64(&obj, "code")? as u16),
            message: get_opt_str(&obj, "message")?,
            data: get_bytes(&obj, "data")?,
        })),
        "event" => Ok(Message::Event(EventMsg {
            id,
            name: get_str(&obj, "name")?,
            data: get_bytes(&obj, "data")?,
        })),
        "stream" => Ok(Message::Stream(StreamMsg {
            id,
            name: get_str(&obj, "name")?,
            seq: get_u64(&obj, "seq")?,
            sender_ts: Timestamp(get_u64(&obj, "senderTs")?),
            data: get_bytes(&obj, "data")?,
        })),
        "streamEnd" => Ok(Message::StreamEnd(StreamEndMsg { id })),
        other => Err(js_err(&format!("未知消息类型: {other}"))),
    }
}

fn message_to_js(msg: &Message) -> Result<JsValue, JsValue> {
    let obj = Object::new();
    match msg {
        Message::Request(m) => {
            Reflect::set(&obj, &"type".into(), &"request".into()).unwrap();
            Reflect::set(&obj, &"id".into(), &JsValue::from_f64(m.id as f64)).unwrap();
            Reflect::set(&obj, &"name".into(), &m.name.clone().into()).unwrap();
            Reflect::set(&obj, &"data".into(), &Uint8Array::from(&m.data[..]).into()).unwrap();
        }
        Message::Response(m) => {
            Reflect::set(&obj, &"type".into(), &"response".into()).unwrap();
            Reflect::set(&obj, &"id".into(), &JsValue::from_f64(m.id as f64)).unwrap();
            Reflect::set(&obj, &"code".into(), &JsValue::from_f64(m.code.0 as f64)).unwrap();
            Reflect::set(
                &obj,
                &"message".into(),
                &m.message
                    .clone()
                    .map(JsValue::from)
                    .unwrap_or(JsValue::null()),
            )
            .unwrap();
            Reflect::set(&obj, &"data".into(), &Uint8Array::from(&m.data[..]).into()).unwrap();
        }
        Message::Event(m) => {
            Reflect::set(&obj, &"type".into(), &"event".into()).unwrap();
            Reflect::set(&obj, &"id".into(), &JsValue::from_f64(m.id as f64)).unwrap();
            Reflect::set(&obj, &"name".into(), &m.name.clone().into()).unwrap();
            Reflect::set(&obj, &"data".into(), &Uint8Array::from(&m.data[..]).into()).unwrap();
        }
        Message::Stream(m) => {
            Reflect::set(&obj, &"type".into(), &"stream".into()).unwrap();
            Reflect::set(&obj, &"id".into(), &JsValue::from_f64(m.id as f64)).unwrap();
            Reflect::set(&obj, &"name".into(), &m.name.clone().into()).unwrap();
            Reflect::set(&obj, &"seq".into(), &JsValue::from_f64(m.seq as f64)).unwrap();
            Reflect::set(
                &obj,
                &"senderTs".into(),
                &JsValue::from_f64(m.sender_ts.0 as f64),
            )
            .unwrap();
            Reflect::set(&obj, &"data".into(), &Uint8Array::from(&m.data[..]).into()).unwrap();
        }
        Message::StreamEnd(m) => {
            Reflect::set(&obj, &"type".into(), &"streamEnd".into()).unwrap();
            Reflect::set(&obj, &"id".into(), &JsValue::from_f64(m.id as f64)).unwrap();
        }
    }
    Ok(obj.into())
}

// ======================== 辅助 ========================

fn js_err(msg: &str) -> JsValue {
    JsValue::from_str(msg)
}

fn get_str(obj: &Object, key: &str) -> Result<String, JsValue> {
    let v = Reflect::get(obj, &key.into())
        .map_err(|e| js_err(&format!("字段 {key} 读取失败: {e:?}")))?;
    v.as_string()
        .ok_or_else(|| js_err(&format!("字段 {key} 必须是字符串")))
}

fn get_u64(obj: &Object, key: &str) -> Result<u64, JsValue> {
    let v = Reflect::get(obj, &key.into())
        .map_err(|e| js_err(&format!("字段 {key} 读取失败: {e:?}")))?;
    if let Some(n) = v.as_f64()
        && n.is_finite()
        && n >= 0.0
    {
        return Ok(n as u64);
    }
    if v.is_bigint() {
        let big = BigInt::from(v);
        let s = big
            .to_string(10)
            .map_err(|_| js_err("bigint 转换失败"))?
            .as_string()
            .unwrap_or_default();
        return s
            .parse()
            .map_err(|_| js_err(&format!("字段 {key} 超出 u64 范围")));
    }
    Err(js_err(&format!("字段 {key} 必须是整数")))
}

fn get_opt_str(obj: &Object, key: &str) -> Result<Option<String>, JsValue> {
    let v = Reflect::get(obj, &key.into())
        .map_err(|e| js_err(&format!("字段 {key} 读取失败: {e:?}")))?;
    if v.is_null() || v.is_undefined() {
        return Ok(None);
    }
    v.as_string()
        .map(Some)
        .ok_or_else(|| js_err(&format!("字段 {key} 必须是字符串或 null")))
}

fn get_bytes(obj: &Object, key: &str) -> Result<Bytes, JsValue> {
    let v = Reflect::get(obj, &key.into())
        .map_err(|e| js_err(&format!("字段 {key} 读取失败: {e:?}")))?;
    let arr = v
        .dyn_ref::<Uint8Array>()
        .ok_or_else(|| js_err(&format!("字段 {key} 必须是 Uint8Array")))?;
    Ok(Bytes::from(arr.to_vec()))
}

// ======================== 无 I/O 客户端状态机 ========================

/// 客户端核心状态机（WASM 句柄）
///
/// RPC id 分配/响应匹配、事件路由、服务端主动调用处理全部在 Rust 侧，
/// JS 网络层只需：读帧 → `handle_inbound`，写帧 ← 各 build 方法产物。
#[wasm_bindgen]
pub struct ClientCoreHandle {
    core: echostream_client_core::ClientCore,
}

impl Default for ClientCoreHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// 帧编码（Message → 长度前缀帧）
fn encode_frame_bytes(msg: &Message) -> Result<Vec<u8>, JsValue> {
    let payload = postcard::to_allocvec(msg).map_err(|e| js_err(&format!("编码失败: {e}")))?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

#[wasm_bindgen]
impl ClientCoreHandle {
    /// 创建状态机
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            core: echostream_client_core::ClientCore::new(),
        }
    }

    /// 发起 RPC：返回请求帧（长度前缀 + Message），响应到达时调用 `resolve(data: Uint8Array)`
    pub fn request(
        &mut self,
        name: &str,
        payload: &[u8],
        resolve: js_sys::Function,
    ) -> Result<Vec<u8>, JsValue> {
        let (_, msg) =
            self.core
                .build_request(name, Bytes::copy_from_slice(payload), move |data: Bytes| {
                    let arr = Uint8Array::from(&data[..]);
                    let _ = resolve.call1(&JsValue::NULL, &arr.into());
                });
        encode_frame_bytes(&msg)
    }

    /// 构造事件帧
    pub fn build_event(&mut self, name: &str, payload: &[u8]) -> Result<Vec<u8>, JsValue> {
        let msg = self.core.build_event(name, Bytes::copy_from_slice(payload));
        encode_frame_bytes(&msg)
    }

    /// 打开流：分配流 id
    pub fn open_stream(&mut self, name: &str) -> u64 {
        self.core.open_stream(name)
    }

    /// 构造流数据帧（自动递增序号；senderTs 为毫秒时间戳）
    pub fn build_stream_frame(
        &mut self,
        id: u64,
        name: &str,
        payload: &[u8],
        sender_ts: u64,
    ) -> Result<Vec<u8>, JsValue> {
        let msg =
            self.core
                .build_stream_frame(id, name, Bytes::copy_from_slice(payload), sender_ts);
        encode_frame_bytes(&msg)
    }

    /// 构造数据报事件载荷（不可靠通道；WebTransport.sendDatagram / QUIC datagram）
    pub fn build_datagram_event(&mut self, name: &str, payload: &[u8]) -> Vec<u8> {
        self.core
            .build_datagram_event(name, Bytes::copy_from_slice(payload))
    }

    /// 构造流结束标记（WebSocket 传输的流关闭）
    pub fn build_stream_end(&mut self, id: u64) -> Result<Vec<u8>, JsValue> {
        let msg = self.core.build_stream_end(id);
        encode_frame_bytes(&msg)
    }

    /// 构造响应帧（服务端主动调用的异步回复）
    pub fn build_response(&mut self, id: u64, payload: &[u8]) -> Result<Vec<u8>, JsValue> {
        let msg = self
            .core
            .build_response(id, Bytes::copy_from_slice(payload));
        encode_frame_bytes(&msg)
    }

    /// 注册事件监听（回调：`(name: string, data: Uint8Array) => void`）
    pub fn on_event(&mut self, name: &str, callback: js_sys::Function) {
        let name_js = JsValue::from_str(name);
        self.core.on_event(name, move |data: Bytes| {
            let arr = Uint8Array::from(&data[..]);
            let _ = callback.call2(&JsValue::NULL, &name_js, &arr.into());
        });
    }

    /// 注册 RPC 处理器（处理对端主动调用；回调返回响应字节或 null 表示异步处理）
    pub fn on_rpc(&mut self, name: &str, callback: js_sys::Function) {
        let name_js = JsValue::from_str(name);
        self.core.on_rpc(name, move |data: Bytes| {
            let arr = Uint8Array::from(&data[..]);
            match callback.call2(&JsValue::NULL, &name_js, &arr.into()) {
                Ok(ret) if !ret.is_null() && !ret.is_undefined() => {
                    let bytes = ret
                        .dyn_ref::<Uint8Array>()
                        .map(|a| a.to_vec())
                        .unwrap_or_default();
                    Some(Bytes::from(bytes))
                }
                _ => None, // 异步处理：调用方稍后 build_response
            }
        });
    }

    /// 处理入站帧：返回需要写回对端的响应帧（对端主动调用且同步完成时）
    pub fn handle_inbound(&mut self, frame: &[u8]) -> Result<Option<Vec<u8>>, JsValue> {
        if frame.len() < 4 {
            return Err(js_err("帧长度不足"));
        }
        let len = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        let msg: Message = postcard::from_bytes(&frame[4..4 + len])
            .map_err(|e| js_err(&format!("帧解码失败: {e}")))?;
        match self.core.handle_inbound(msg) {
            Some(resp) => Ok(Some(encode_frame_bytes(&resp)?)),
            None => Ok(None),
        }
    }
}
