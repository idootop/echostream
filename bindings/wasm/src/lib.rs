//! EchoStream WASM binding
//!
//! 将 Rust 侧协议核心编译为 WASM，供浏览器 / Node 复用（单一事实来源）：
//! - encode_payload / decode_payload：JS 值 <-> postcard 载荷字节（自动编解码）
//! - encode_message / decode_message / encode_frame：Message 帧编解码
//! - ClientCoreHandle：无 I/O 客户端状态机（RPC 匹配 / 事件路由 / 流管理）
//!
//! 载荷编码约定（与 echostream-proto::dynamic 一致）：
//! - JS 整数（含负数）-> i64 ZigZag varint；BigInt -> u64 普通 varint
//! - 浮点数 -> f64 小端；布尔 -> 单字节；字符串/字节 -> 长度前缀
//! - 数组 -> 元组/结构体字段序；对象 -> 结构体字段序

use bytes::Bytes;
use echostream_proto::{
    Dynamic, EventMsg, Message, RequestMsg, ResponseMsg, Schema, StatusCode, StreamEndMsg,
    StreamMetaEntry, StreamMsg, StreamOpenMsg, Timestamp,
};
use js_sys::{Array, BigInt, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

// ======================== 载荷自动编解码 ========================

/// 编码载荷：JS 值 -> postcard 字节（约定见模块文档）
#[wasm_bindgen]
pub fn encode_payload(value: JsValue) -> Result<Vec<u8>, JsValue> {
    let v = js_to_dynamic(&value)?;
    echostream_proto::dynamic::encode(&v).map_err(js_err_from)
}

/// 解码载荷：postcard 字节 -> JS 值（智能推断）
///
/// schema 可选（字符串 / 数组 / 对象），用于歧义场景精确解码：
/// - "auto" | "number" | "bigint" | "u64" | "bool" | "string" | "bytes" | "f64" | "f32" | "list"
/// - 数组 = 元组逐字段；对象 = 结构体具名字段
#[wasm_bindgen]
pub fn decode_payload(bytes: &[u8], schema: JsValue) -> Result<JsValue, JsValue> {
    let s = js_to_schema(&schema)?;
    let v = echostream_proto::dynamic::decode_with(bytes, &s).map_err(js_err_from)?;
    Ok(dynamic_to_js(&v))
}

// ======================== 载荷解码原语（兼容） ========================

/// 解码 u64（普通 varint）
#[wasm_bindgen]
pub fn decode_u64(bytes: &[u8]) -> Result<f64, JsValue> {
    match echostream_proto::dynamic::decode_with(bytes, &Schema::U64).map_err(js_err_from)? {
        Dynamic::UInt(n) => Ok(n as f64),
        _ => Err(js_err("载荷不是 u64")),
    }
}

/// 编码 i64（ZigZag varint，与 postcard 有符号整数一致）
#[wasm_bindgen]
pub fn encode_i64(n: i64) -> Vec<u8> {
    echostream_proto::dynamic::encode(&Dynamic::Int(n)).expect("编码失败")
}

/// 解码 i64（ZigZag varint）
#[wasm_bindgen]
pub fn decode_i64(bytes: &[u8]) -> Result<f64, JsValue> {
    match echostream_proto::dynamic::decode_with(bytes, &Schema::Number).map_err(js_err_from)? {
        Dynamic::Int(n) => Ok(n as f64),
        _ => Err(js_err("载荷不是 i64")),
    }
}

/// 解码 string
#[wasm_bindgen]
pub fn decode_string(bytes: &[u8]) -> Result<String, JsValue> {
    match echostream_proto::dynamic::decode_with(bytes, &Schema::Str).map_err(js_err_from)? {
        Dynamic::Str(s) => Ok(s),
        _ => Err(js_err("载荷不是字符串")),
    }
}

/// 解码 bytes（长度前缀 + 字节）
#[wasm_bindgen]
pub fn decode_bytes(bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    match echostream_proto::dynamic::decode_with(bytes, &Schema::Bytes).map_err(js_err_from)? {
        Dynamic::Bytes(b) => Ok(b),
        _ => Err(js_err("载荷不是字节数组")),
    }
}

// ======================== 消息编解码 ========================

/// 编码消息：JS 对象 -> postcard 字节
///
/// 输入：{ type, id, name, data, ... }
/// - request/event：{ type, id, name, data: Uint8Array }
/// - response：{ type, id, code, message?, data }
/// - stream：{ type, id, name, seq, senderTs, data }
#[wasm_bindgen]
pub fn encode_message(msg: JsValue) -> Result<Vec<u8>, JsValue> {
    let msg = js_to_message(&msg)?;
    postcard::to_allocvec(&msg).map_err(|e| js_err(&format!("编码失败: {e}")))
}

/// 解码消息：postcard 字节 -> JS 对象
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

// ======================== JS 值 <-> Dynamic ========================

/// JS 值 -> Dynamic（约定见模块文档）
fn js_to_dynamic(v: &JsValue) -> Result<Dynamic, JsValue> {
    if v.is_undefined() || v.is_null() {
        return Ok(Dynamic::Null);
    }
    if let Some(b) = v.as_bool() {
        return Ok(Dynamic::Bool(b));
    }
    if let Some(n) = v.as_f64() {
        if !n.is_finite() {
            return Err(js_err("载荷 number 必须是有限值"));
        }
        if n.fract() == 0.0 && n.abs() <= 9.007199254740992e15 {
            return Ok(Dynamic::Int(n as i64)); // JS 整数 -> i64 ZigZag 约定
        }
        return Ok(Dynamic::Float(n)); // 浮点 -> f64
    }
    if v.is_bigint() {
        let big = BigInt::from(v.clone());
        let s = big
            .to_string(10)
            .map_err(|_| js_err("bigint 转换失败"))?
            .as_string()
            .unwrap_or_default();
        // 非负 BigInt -> u64 普通 varint；负数 -> i64 ZigZag
        if let Ok(n) = s.parse::<u64>() {
            return Ok(Dynamic::UInt(n));
        }
        if let Ok(n) = s.parse::<i64>() {
            return Ok(Dynamic::Int(n));
        }
        return Err(js_err("bigint 超出 u64/i64 范围"));
    }
    if let Some(s) = v.as_string() {
        return Ok(Dynamic::Str(s));
    }
    if let Some(arr) = v.dyn_ref::<Uint8Array>() {
        return Ok(Dynamic::Bytes(arr.to_vec()));
    }
    if let Some(arr) = v.dyn_ref::<Array>() {
        let mut items = Vec::with_capacity(arr.length() as usize);
        for item in arr.iter() {
            items.push(js_to_dynamic(&item)?);
        }
        return Ok(Dynamic::Seq(items));
    }
    if v.is_object() {
        let obj = Object::from(v.clone());
        let keys =
            Reflect::own_keys(&obj).map_err(|e| js_err(&format!("对象键获取失败: {e:?}")))?;
        let mut fields = Vec::with_capacity(keys.length() as usize);
        for i in 0..keys.length() {
            let key = keys.get(i);
            let key_str = key.as_string().ok_or_else(|| js_err("对象键非字符串"))?;
            let val =
                Reflect::get(&obj, &key).map_err(|e| js_err(&format!("字段读取失败: {e:?}")))?;
            fields.push((key_str, js_to_dynamic(&val)?));
        }
        return Ok(Dynamic::Map(fields));
    }
    Err(js_err("不支持的载荷类型"))
}

/// Dynamic -> JS 值
fn dynamic_to_js(v: &Dynamic) -> JsValue {
    match v {
        Dynamic::Null => JsValue::UNDEFINED,
        Dynamic::Bool(b) => JsValue::from_bool(*b),
        Dynamic::Int(n) => {
            if n.unsigned_abs() <= (1u64 << 53) {
                JsValue::from_f64(*n as f64)
            } else {
                BigInt::from(*n).into()
            }
        }
        Dynamic::UInt(n) => {
            if *n <= (1u64 << 53) {
                JsValue::from_f64(*n as f64)
            } else {
                BigInt::from(*n).into()
            }
        }
        Dynamic::Float(f) => JsValue::from_f64(*f),
        Dynamic::Str(s) => JsValue::from_str(s),
        Dynamic::Bytes(b) => Uint8Array::from(b.as_slice()).into(),
        Dynamic::Seq(items) => {
            let arr = Array::new();
            for item in items {
                arr.push(&dynamic_to_js(item));
            }
            arr.into()
        }
        Dynamic::Map(fields) => {
            let obj = Object::new();
            for (name, value) in fields {
                Reflect::set(&obj, &name.clone().into(), &dynamic_to_js(value)).unwrap();
            }
            obj.into()
        }
    }
}

/// JS schema 值 -> Schema（字符串 / 数组 / 对象）
fn js_to_schema(v: &JsValue) -> Result<Schema, JsValue> {
    if v.is_undefined() || v.is_null() {
        return Ok(Schema::Auto);
    }
    if let Some(s) = v.as_string() {
        return Ok(match s.as_str() {
            "auto" | "json" => Schema::Auto,
            "number" | "int" | "i64" => Schema::Number,
            "bigint" => Schema::BigInt,
            "u64" => Schema::U64,
            "bool" | "boolean" => Schema::Bool,
            "string" | "str" => Schema::Str,
            "bytes" | "buffer" => Schema::Bytes,
            "f64" | "float" => Schema::F64,
            "f32" => Schema::F32,
            "list" | "array" => Schema::List(Box::new(Schema::Auto)),
            other => return Err(js_err(&format!("未知 schema: {other}"))),
        });
    }
    if let Some(arr) = v.dyn_ref::<Array>() {
        let mut schemas = Vec::with_capacity(arr.length() as usize);
        for item in arr.iter() {
            schemas.push(js_to_schema(&item)?);
        }
        return Ok(Schema::Seq(schemas));
    }
    if v.is_object() {
        let obj = Object::from(v.clone());
        let keys =
            Reflect::own_keys(&obj).map_err(|e| js_err(&format!("对象键获取失败: {e:?}")))?;
        let mut fields = Vec::with_capacity(keys.length() as usize);
        for i in 0..keys.length() {
            let key = keys.get(i);
            let key_str = key.as_string().ok_or_else(|| js_err("对象键非字符串"))?;
            let val =
                Reflect::get(&obj, &key).map_err(|e| js_err(&format!("字段读取失败: {e:?}")))?;
            fields.push((key_str, js_to_schema(&val)?));
        }
        return Ok(Schema::Map(fields));
    }
    Err(js_err("schema 必须是字符串 / 数组 / 对象"))
}

// ======================== JS 值 <-> Message ========================

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
        "streamOpen" => Ok(Message::StreamOpen(StreamOpenMsg {
            id,
            name: get_str(&obj, "name")?,
            metadata: get_metadata(&obj)?,
        })),
        "stream" => Ok(Message::Stream(StreamMsg {
            id,
            seq: get_u64(&obj, "seq")?,
            sender_ts: Timestamp(get_u64(&obj, "senderTs")?),
            data: get_bytes(&obj, "data")?,
        })),
        "streamEnd" => Ok(Message::StreamEnd(StreamEndMsg {
            id,
            code: get_u64(&obj, "code")? as u16,
            message: get_opt_str(&obj, "message")?,
            metadata: get_metadata(&obj)?,
        })),
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
        Message::StreamOpen(m) => {
            Reflect::set(&obj, &"type".into(), &"streamOpen".into()).unwrap();
            Reflect::set(&obj, &"id".into(), &JsValue::from_f64(m.id as f64)).unwrap();
            Reflect::set(&obj, &"name".into(), &m.name.clone().into()).unwrap();
            Reflect::set(&obj, &"metadata".into(), &metadata_to_js(&m.metadata)).unwrap();
        }
        Message::Stream(m) => {
            Reflect::set(&obj, &"type".into(), &"stream".into()).unwrap();
            Reflect::set(&obj, &"id".into(), &JsValue::from_f64(m.id as f64)).unwrap();
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
            Reflect::set(&obj, &"code".into(), &JsValue::from_f64(m.code as f64)).unwrap();
            Reflect::set(
                &obj,
                &"message".into(),
                &m.message
                    .clone()
                    .map(JsValue::from)
                    .unwrap_or(JsValue::null()),
            )
            .unwrap();
            Reflect::set(&obj, &"metadata".into(), &metadata_to_js(&m.metadata)).unwrap();
        }
    }
    Ok(obj.into())
}

/// JS 元数据对象 <-> Vec<StreamMetaEntry>
///
/// JS 表示：Record<string, string | Uint8Array>（值自动按 UTF-8 字符串/字节处理）
fn get_metadata(obj: &js_sys::Object) -> Result<Vec<StreamMetaEntry>, JsValue> {
    let meta = Reflect::get(obj, &"metadata".into()).unwrap_or(JsValue::UNDEFINED);
    if meta.is_undefined() || meta.is_null() {
        return Ok(Vec::new());
    }
    let obj = meta
        .dyn_ref::<js_sys::Object>()
        .ok_or_else(|| js_err("metadata 必须是对象"))?;
    let keys =
        Reflect::own_keys(obj).map_err(|e| js_err(&format!("metadata 键获取失败: {e:?}")))?;
    let mut out = Vec::with_capacity(keys.length() as usize);
    for i in 0..keys.length() {
        let key = keys.get(i);
        let key_str = key
            .as_string()
            .ok_or_else(|| js_err("metadata 键非字符串"))?;
        let val =
            Reflect::get(obj, &key).map_err(|e| js_err(&format!("metadata 值读取失败: {e:?}")))?;
        let value = if let Some(s) = val.as_string() {
            Bytes::from(s.into_bytes())
        } else if let Some(arr) = val.dyn_ref::<Uint8Array>() {
            Bytes::from(arr.to_vec())
        } else if val.as_f64().is_some() {
            Bytes::from(val.as_f64().unwrap_or(0.0).to_string().into_bytes())
        } else if val.is_bigint() {
            let s = js_sys::BigInt::from(val)
                .to_string(10)
                .map(|s| s.as_string().unwrap_or_default())
                .unwrap_or_default();
            Bytes::from(s.into_bytes())
        } else {
            return Err(js_err("metadata 值必须是字符串 / 数字 / Uint8Array"));
        };
        out.push(StreamMetaEntry {
            key: key_str,
            value,
        });
    }
    Ok(out)
}

fn metadata_to_js(meta: &[StreamMetaEntry]) -> JsValue {
    let obj = js_sys::Object::new();
    for m in meta {
        let val = match String::from_utf8(m.value.to_vec()) {
            Ok(s) => JsValue::from(s),
            Err(_) => Uint8Array::from(&m.value[..]).into(),
        };
        Reflect::set(&obj, &m.key.clone().into(), &val).unwrap();
    }
    obj.into()
}

// ======================== 辅助 ========================

fn js_err(msg: &str) -> JsValue {
    JsValue::from_str(msg)
}

fn js_err_from(e: echostream_proto::Error) -> JsValue {
    JsValue::from_str(&e.to_string())
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
/// JS 网络层只需：读帧 -> handle_inbound，写帧 <- 各 build 方法产物。
#[wasm_bindgen]
pub struct ClientCoreHandle {
    core: echostream_core::ClientCore,
}

impl Default for ClientCoreHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// 帧编码（Message -> 长度前缀帧）
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
            core: echostream_core::ClientCore::new(),
        }
    }

    /// 发起 RPC：返回请求帧（长度前缀 + Message）
    /// 响应到达时调用 resolve(data: Uint8Array, error: string | null)
    pub fn request(
        &mut self,
        name: &str,
        payload: &[u8],
        resolve: js_sys::Function,
    ) -> Result<Vec<u8>, JsValue> {
        let (_, msg) = self.core.build_request(
            name,
            Bytes::copy_from_slice(payload),
            move |data: Bytes, err: Option<String>| {
                let arr = Uint8Array::from(&data[..]);
                let err_js = match err {
                    Some(e) => JsValue::from_str(&e),
                    None => JsValue::NULL,
                };
                let _ = resolve.call2(&JsValue::NULL, &arr.into(), &err_js);
            },
        );
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

    /// 构造流开始帧（流协商：名称 + 元数据；流首帧必须为此帧）
    /// metadata：Record<string, string | number | Uint8Array>
    pub fn build_stream_open(
        &mut self,
        id: u64,
        name: &str,
        metadata: &JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let meta = get_metadata(
            metadata
                .dyn_ref::<js_sys::Object>()
                .ok_or_else(|| js_err("metadata 必须是对象"))?,
        )?;
        let msg = self.core.build_stream_open(id, name, meta);
        encode_frame_bytes(&msg)
    }

    /// 构造流数据帧（自动递增序号；senderTs 为毫秒墙钟）
    pub fn build_stream_frame(
        &mut self,
        id: u64,
        payload: &[u8],
        sender_ts: u64,
    ) -> Result<Vec<u8>, JsValue> {
        let msg = self
            .core
            .build_stream_frame(id, Bytes::copy_from_slice(payload), sender_ts);
        encode_frame_bytes(&msg)
    }

    /// 构造数据报事件载荷（不可靠通道；WebTransport.sendDatagram / QUIC datagram）
    pub fn build_datagram_event(&mut self, name: &str, payload: &[u8]) -> Vec<u8> {
        self.core
            .build_datagram_event(name, Bytes::copy_from_slice(payload))
    }

    /// 构造流结束帧（WebSocket 传输的流关闭；code=0 正常，非 0 异常/取消）
    pub fn build_stream_end(
        &mut self,
        id: u64,
        code: u32,
        message: Option<String>,
    ) -> Result<Vec<u8>, JsValue> {
        let msg = self.core.build_stream_end(id, code as u16, message);
        encode_frame_bytes(&msg)
    }

    /// 查询流的元数据（StreamOpen 记录；返回 Record<string, string | Uint8Array>）
    pub fn stream_metadata(&self, id: u64) -> Option<JsValue> {
        self.core.stream_metadata(id).map(metadata_to_js)
    }

    /// 查询流的结束信息（StreamEnd 记录；返回 { code, message, metadata } 或 null）
    pub fn stream_end(&self, id: u64) -> Option<JsValue> {
        self.core.stream_end(id).map(|e| {
            let obj = js_sys::Object::new();
            Reflect::set(&obj, &"code".into(), &JsValue::from_f64(e.code as f64)).unwrap();
            Reflect::set(
                &obj,
                &"message".into(),
                &e.message
                    .clone()
                    .map(JsValue::from)
                    .unwrap_or(JsValue::null()),
            )
            .unwrap();
            Reflect::set(&obj, &"metadata".into(), &metadata_to_js(&e.metadata)).unwrap();
            obj.into()
        })
    }

    /// 清理已结束流的内部状态（避免元数据累积）
    pub fn remove_stream_state(&mut self, id: u64) {
        self.core.remove_stream_state(id);
    }

    /// 构造响应帧（服务端主动调用的异步回复）
    pub fn build_response(&mut self, id: u64, payload: &[u8]) -> Result<Vec<u8>, JsValue> {
        let msg = self
            .core
            .build_response(id, Bytes::copy_from_slice(payload));
        encode_frame_bytes(&msg)
    }

    /// 构造错误响应帧（处理对端主动调用失败时回复）
    pub fn build_error_response(&mut self, id: u64, message: &str) -> Result<Vec<u8>, JsValue> {
        let msg = self.core.build_error_response(id, message);
        encode_frame_bytes(&msg)
    }

    /// 注册入站流处理器（处理对端推送的流；回调：frame 对象或 null），
    /// 帧对象含 { id, seq, senderTs, data: Uint8Array }，返回监听 id（off_stream 取消注册）
    pub fn on_stream(&mut self, name: &str, callback: js_sys::Function) -> u32 {
        let id = self
            .core
            .on_stream(name, move |frame: Option<StreamMsg>| match frame {
                Some(f) => {
                    let obj = js_sys::Object::new();
                    Reflect::set(&obj, &"id".into(), &JsValue::from_f64(f.id as f64)).unwrap();
                    Reflect::set(&obj, &"seq".into(), &JsValue::from_f64(f.seq as f64)).unwrap();
                    Reflect::set(
                        &obj,
                        &"senderTs".into(),
                        &JsValue::from_f64(f.sender_ts.0 as f64),
                    )
                    .unwrap();
                    Reflect::set(&obj, &"data".into(), &Uint8Array::from(&f.data[..]).into())
                        .unwrap();
                    let _ = callback.call1(&JsValue::NULL, &obj.into());
                }
                None => {
                    let _ = callback.call1(&JsValue::NULL, &JsValue::NULL);
                }
            });
        id as u32
    }

    /// 取消注册流处理器（按 on_stream 返回的 id）
    pub fn off_stream(&mut self, id: u32) -> bool {
        self.core.off_stream(id as u64)
    }

    /// 注册事件监听（回调：name 与 data 两个参数），返回监听 id（off_event 取消注册）
    pub fn on_event(&mut self, name: &str, callback: js_sys::Function) -> u32 {
        let name_js = JsValue::from_str(name);
        let id = self
            .core
            .on_event(name, move |_event_name: &str, data: Bytes| {
                let arr = Uint8Array::from(&data[..]);
                let _ = callback.call2(&JsValue::NULL, &name_js, &arr.into());
            });
        id as u32
    }

    /// 取消注册事件监听（按 on_event 返回的 id）
    pub fn off_event(&mut self, id: u32) -> bool {
        self.core.off_event(id as u64)
    }

    /// 注册 RPC 处理器（处理对端主动调用），返回监听 id（off_rpc 取消注册）
    /// 回调签名：(name: string, data: Uint8Array, id: number) => Uint8Array | null
    /// 返回 null 表示异步处理（稍后通过 build_response(id, payload) 补响应）
    pub fn on_rpc(&mut self, name: &str, callback: js_sys::Function) -> u32 {
        let name_js = JsValue::from_str(name);
        let id = self.core.on_rpc(name, move |id: u64, data: Bytes| {
            let arr = Uint8Array::from(&data[..]);
            match callback.call3(
                &JsValue::NULL,
                &name_js,
                &arr.into(),
                &JsValue::from_f64(id as f64),
            ) {
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
        id as u32
    }

    /// 取消注册 RPC 处理器（按 on_rpc 返回的 id）
    pub fn off_rpc(&mut self, id: u32) -> bool {
        self.core.off_rpc(id as u64)
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
