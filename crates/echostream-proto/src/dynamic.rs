//! 动态值编解码：跨语言载荷的自动序列化约定（schema-less 兼容 postcard）
//!
//! 各端绑定（WASM / Node / Python）通过本模块实现自动编解码：业务侧
//! 直接传对象/数组/字符串等原生值，无需手写字节；线缆格式与 Rust 的
//! postcard 序列化完全一致，跨端互操作天然成立。
//!
//! 编码约定（JS/Python 值 -> postcard 字节）：
//! - 整数（含负数）-> i64 ZigZag varint（跨端统一约定，与 Rust i64 对齐）
//! - 非负 BigInt -> u64 普通 varint（Rust u64 对齐）
//! - 浮点数 -> f64 小端 8 字节
//! - 布尔 -> 单字节 0/1
//! - 字符串 -> varint 长度前缀 + UTF-8
//! - 字节数组 -> varint 长度前缀 + 原始字节
//! - 数组 -> 元组/结构体字段（顺序编码，无长度前缀）
//! - 对象 -> 结构体字段（按插入序编码，无键名、无长度前缀）
//! - null/undefined -> 空载荷（Rust ()）
//!
//! 解码默认智能推断（Schema::Auto），歧义场景（如 Vec 与元组、空字符串
//! 与数字 0）可显式传入 Schema 精确解码。

use crate::{Error, Result};

/// 动态值：各端原生值（JS/Python 对象）与字节之间的中间表示
#[derive(Debug, Clone, PartialEq)]
pub enum Dynamic {
    /// 空值（Rust () / None）
    Null,
    /// 布尔
    Bool(bool),
    /// 有符号整数（编码为 ZigZag varint）
    Int(i64),
    /// 无符号整数（编码为普通 varint）
    UInt(u64),
    /// 浮点数（编码为 f64 小端）
    Float(f64),
    /// 字符串（varint 长度前缀 + UTF-8）
    Str(String),
    /// 字节数组（varint 长度前缀 + 原始字节）
    Bytes(Vec<u8>),
    /// 序列（元组/结构体字段序，无长度前缀）
    Seq(Vec<Dynamic>),
    /// 具名字段结构体（编码同 Seq，键名不落盘）
    Map(Vec<(String, Dynamic)>),
}

/// 编解码模式：Auto 智能推断，或显式指定精确类型
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Schema {
    /// 智能推断（默认）
    #[default]
    Auto,
    /// i64 ZigZag varint -> JS 安全整数（超出 2^53 请用 BigInt 模式）
    Number,
    /// i64 ZigZag varint -> BigInt
    BigInt,
    /// u64 普通 varint
    U64,
    /// 布尔（单字节）
    Bool,
    /// 长度前缀 + UTF-8
    Str,
    /// 长度前缀 + 原始字节
    Bytes,
    /// f64 小端 8 字节
    F64,
    /// f32 小端 4 字节
    F32,
    /// 长度前缀的序列（Rust Vec<T>）
    List(Box<Schema>),
    /// 元组（按序字段，无长度前缀）
    Seq(Vec<Schema>),
    /// 结构体（按序字段，无长度前缀；键名仅用于映射字段名）
    Map(Vec<(String, Schema)>),
}

/// 编码动态值为 postcard 兼容字节
pub fn encode(value: &Dynamic) -> Result<Vec<u8>> {
    let mut w = Writer::default();
    w.value(value)?;
    Ok(w.bytes)
}

/// 解码字节为动态值（智能推断）
pub fn decode(bytes: &[u8]) -> Result<Dynamic> {
    decode_with(bytes, &Schema::Auto)
}

/// 解码字节为动态值（按指定模式）
pub fn decode_with(bytes: &[u8], schema: &Schema) -> Result<Dynamic> {
    let mut r = Reader::new(bytes);
    let v = r.value(schema)?;
    if !r.eof() {
        return Err(Error::Serialization(format!(
            "载荷解码未消费完整: 剩余 {} 字节",
            r.remaining()
        )));
    }
    Ok(v)
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

    fn value(&mut self, v: &Dynamic) -> Result<()> {
        match v {
            Dynamic::Null => {}
            Dynamic::Bool(b) => self.bytes.push(*b as u8),
            Dynamic::Int(n) => self.varint(((n << 1) ^ (n >> 63)) as u64),
            Dynamic::UInt(n) => self.varint(*n),
            Dynamic::Float(f) => self.bytes.extend_from_slice(&f.to_le_bytes()),
            Dynamic::Str(s) => {
                self.varint(s.len() as u64);
                self.bytes.extend_from_slice(s.as_bytes());
            }
            Dynamic::Bytes(b) => {
                self.varint(b.len() as u64);
                self.bytes.extend_from_slice(b);
            }
            Dynamic::Seq(items) => {
                for item in items {
                    self.value(item)?;
                }
            }
            Dynamic::Map(fields) => {
                for (_, value) in fields {
                    self.value(value)?;
                }
            }
        }
        Ok(())
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

    fn eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    fn varint(&mut self) -> Result<u64> {
        let mut result: u64 = 0;
        let mut shift = 0;
        loop {
            let b = *self
                .bytes
                .get(self.pos)
                .ok_or_else(|| Error::Serialization("varint 越界".into()))?;
            self.pos += 1;
            result |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 63 {
                return Err(Error::Serialization("varint 溢出".into()));
            }
        }
        Ok(result)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.pos + len;
        if end > self.bytes.len() {
            return Err(Error::Serialization("字节数据越界".into()));
        }
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn value(&mut self, schema: &Schema) -> Result<Dynamic> {
        match schema {
            Schema::Auto => self.auto(),
            Schema::Number => {
                let v = self.varint()?;
                Ok(Dynamic::Int(from_zigzag(v)))
            }
            Schema::BigInt => {
                let v = self.varint()?;
                Ok(Dynamic::Int(from_zigzag(v)))
            }
            Schema::U64 => Ok(Dynamic::UInt(self.varint()?)),
            Schema::Bool => {
                let b = self
                    .bytes
                    .get(self.pos)
                    .ok_or_else(|| Error::Serialization("bool 越界".into()))?;
                self.pos += 1;
                Ok(Dynamic::Bool(*b != 0))
            }
            Schema::Str => {
                let len = self.varint()? as usize;
                let data = self.take(len)?;
                let s = std::str::from_utf8(data)
                    .map_err(|e| Error::Serialization(format!("UTF-8 解码失败: {e}")))?;
                Ok(Dynamic::Str(s.to_string()))
            }
            Schema::Bytes => {
                let len = self.varint()? as usize;
                Ok(Dynamic::Bytes(self.take(len)?.to_vec()))
            }
            Schema::F64 => {
                let data = self.take(8)?;
                Ok(Dynamic::Float(f64::from_le_bytes(data.try_into().unwrap())))
            }
            Schema::F32 => {
                let data = self.take(4)?;
                Ok(Dynamic::Float(
                    f32::from_le_bytes(data.try_into().unwrap()) as f64
                ))
            }
            Schema::List(inner) => {
                let len = self.varint()? as usize;
                let mut items = Vec::with_capacity(len.min(1024));
                for _ in 0..len {
                    items.push(self.value(inner)?);
                }
                Ok(Dynamic::Seq(items))
            }
            Schema::Seq(schemas) => {
                let mut items = Vec::with_capacity(schemas.len());
                for s in schemas {
                    items.push(self.value(s)?);
                }
                Ok(Dynamic::Seq(items))
            }
            Schema::Map(fields) => {
                let mut items = Vec::with_capacity(fields.len());
                for (name, s) in fields {
                    items.push((name.clone(), self.value(s)?));
                }
                Ok(Dynamic::Map(items))
            }
        }
    }

    /// 智能推断解码：逐字段贪心解析
    ///
    /// 规则（按优先级）：
    /// 1. 空载荷 -> Null
    /// 2. varint 为 0 -> 数字 0（空字符串需显式 Str 模式）
    /// 3. varint N <= 剩余且后 N 字节为合法 UTF-8 -> 字符串（长度 N）
    /// 4. varint N <= 剩余且后 N 字节非 UTF-8 -> 字节数组
    /// 5. 否则 -> i64 ZigZag 数字
    ///
    /// 单个字段直接返回；多字段返回序列（元组/结构体）
    fn auto(&mut self) -> Result<Dynamic> {
        if self.eof() {
            return Ok(Dynamic::Null);
        }
        let mut fields = Vec::new();
        loop {
            if self.eof() {
                break;
            }
            let start = self.pos;
            let v = self.varint()?;
            if v == 0 {
                fields.push(Dynamic::Int(0));
                continue;
            }
            if v as usize <= self.remaining() {
                let data = self.take(v as usize)?;
                match std::str::from_utf8(data) {
                    Ok(s) => fields.push(Dynamic::Str(s.to_string())),
                    Err(_) => fields.push(Dynamic::Bytes(data.to_vec())),
                }
                continue;
            }
            // 不是长度前缀：回退为数字
            self.pos = start;
            let n = self.varint()?;
            fields.push(Dynamic::Int(from_zigzag(n)));
        }
        if fields.len() == 1 {
            Ok(fields.pop().unwrap())
        } else {
            Ok(Dynamic::Seq(fields))
        }
    }
}

/// ZigZag 解码：varint -> 有符号整数
fn from_zigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_scalars() {
        // i64 ZigZag：10 -> 0x14
        let b = encode(&Dynamic::Int(10)).unwrap();
        assert_eq!(b, vec![0x14]);
        assert_eq!(decode(&b).unwrap(), Dynamic::Int(10));
        // 负数
        let b = encode(&Dynamic::Int(-1)).unwrap();
        assert_eq!(b, vec![0x01]);
        assert_eq!(decode(&b).unwrap(), Dynamic::Int(-1));
        // u64
        let b = encode(&Dynamic::UInt(10)).unwrap();
        assert_eq!(b, vec![0x0a]);
        // 布尔
        assert_eq!(encode(&Dynamic::Bool(true)).unwrap(), vec![0x01]);
        assert_eq!(encode(&Dynamic::Bool(false)).unwrap(), vec![0x00]);
        // 字符串
        let b = encode(&Dynamic::Str("hello".into())).unwrap();
        assert_eq!(b, vec![0x05, b'h', b'e', b'l', b'l', b'o']);
        assert_eq!(decode(&b).unwrap(), Dynamic::Str("hello".into()));
        // 字节
        let b = encode(&Dynamic::Bytes(vec![0xde, 0xad, 0xbe, 0xef])).unwrap();
        assert_eq!(
            decode(&b).unwrap(),
            Dynamic::Bytes(vec![0xde, 0xad, 0xbe, 0xef])
        );
        // 浮点
        let b = encode(&Dynamic::Float(1.5)).unwrap();
        assert_eq!(b.len(), 8);
        assert_eq!(decode_with(&b, &Schema::F64).unwrap(), Dynamic::Float(1.5));
        // 空载荷
        assert_eq!(decode(&[]).unwrap(), Dynamic::Null);
        assert_eq!(encode(&Dynamic::Null).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn roundtrip_tuple_and_map() {
        // 元组 (i64, i64)
        let v = Dynamic::Seq(vec![Dynamic::Int(10), Dynamic::Int(20)]);
        let b = encode(&v).unwrap();
        assert_eq!(b, vec![0x14, 0x28]);
        assert_eq!(decode(&b).unwrap(), v);
        // 结构体 {a, b}
        let v = Dynamic::Map(vec![
            ("a".into(), Dynamic::Int(10)),
            ("b".into(), Dynamic::Str("hi".into())),
        ]);
        let b = encode(&v).unwrap();
        assert_eq!(b, vec![0x14, 0x02, b'h', b'i']);
        assert_eq!(
            decode_with(
                &b,
                &Schema::Map(vec![
                    ("a".into(), Schema::Number),
                    ("b".into(), Schema::Str),
                ])
            )
            .unwrap(),
            v
        );
    }

    #[test]
    fn schema_list() {
        // Vec<i64>：长度前缀 + 元素
        let v = Dynamic::Seq(vec![Dynamic::Int(1), Dynamic::Int(2), Dynamic::Int(3)]);
        let schema = Schema::List(Box::new(Schema::Number));
        let mut w = Writer::default();
        w.varint(3);
        w.value(&v).unwrap();
        assert_eq!(w.bytes, vec![0x03, 0x02, 0x04, 0x06]);
        assert_eq!(decode_with(&w.bytes, &schema).unwrap(), v);
    }

    #[test]
    fn smart_decode_ambiguity() {
        // 单个 varint -> 数字（30 的 ZigZag 编码为 60 = 0x3c）
        assert_eq!(decode(&[0x3c]).unwrap(), Dynamic::Int(30));
        // 字符串优先
        assert_eq!(
            decode(&[0x05, b'h', b'e', b'l', b'l', b'o']).unwrap(),
            Dynamic::Str("hello".into())
        );
        // 多字段（10、20 的 ZigZag 编码为 0x14、0x28）
        assert_eq!(
            decode(&[0x14, 0x28]).unwrap(),
            Dynamic::Seq(vec![Dynamic::Int(10), Dynamic::Int(20)])
        );
        // 非 UTF-8 字节
        assert_eq!(
            decode(&[0x04, 0xde, 0xad, 0xbe, 0xef]).unwrap(),
            Dynamic::Bytes(vec![0xde, 0xad, 0xbe, 0xef])
        );
        // 0 -> 数字 0
        assert_eq!(decode(&[0x00]).unwrap(), Dynamic::Int(0));
    }
}
