"""EchoStream Python 载荷编解码（纯 Python，与 echostream-proto::dynamic 语义一致）

编码约定（Python 值 -> postcard 字节）：
- int -> i64 ZigZag varint（超出 i64 范围的非负 int -> u64 普通 varint）
- float -> f64 小端；bool -> 单字节；str/bytes -> 长度前缀
- list/tuple -> 元组/结构体字段序；dict -> 结构体字段序；None -> 空载荷

解码默认智能推断；歧义场景传 schema（"number"|"bigint"|"u64"|"bool"|
"string"|"bytes"|"f64"|"f32"|"list"|list=元组|dict=结构体）。
与 WASM/Node 实现交叉验证保持一致。
"""

from __future__ import annotations

import struct

_MAX_I64 = (1 << 63) - 1
_MIN_I64 = -(1 << 63)
_MAX_U64 = (1 << 64) - 1
_MAX_JS_SAFE = (1 << 53) - 1


def _varint(n: int) -> bytes:
    out = bytearray()
    while n >= 0x80:
        out.append((n & 0x7F) | 0x80)
        n >>= 7
    out.append(n)
    return bytes(out)


def _zigzag(n: int) -> int:
    return ((n << 1) ^ (n >> 63)) & _MAX_U64


def _from_zigzag(v: int) -> int:
    v &= _MAX_U64
    return (v >> 1) ^ -(v & 1)


def _js_number(n: int):
    return n if abs(n) <= _MAX_JS_SAFE else n


def _encode_value(w: bytearray, v, schema=None) -> None:
    if v is None:
        return
    if isinstance(v, bool):
        w.append(1 if v else 0)
        return
    if isinstance(v, int):
        if _MIN_I64 <= v <= _MAX_I64:
            w += _varint(_zigzag(v))
        elif 0 <= v <= _MAX_U64:
            w += _varint(v)
        else:
            raise ValueError(f"整数超出 u64/i64 范围: {v}")
        return
    if isinstance(v, float):
        w += struct.pack("<d", v)
        return
    if isinstance(v, str):
        data = v.encode("utf-8")
        w += _varint(len(data))
        w += data
        return
    if isinstance(v, (bytes, bytearray)):
        w += _varint(len(v))
        w += bytes(v)
        return
    if isinstance(v, (list, tuple)):
        if schema == "list":
            w += _varint(len(v))
        for item in v:
            _encode_value(w, item)
        return
    if isinstance(v, dict):
        if isinstance(schema, dict):
            for key, field_schema in schema.items():
                if key not in v:
                    raise ValueError(f"缺少字段: {key}")
                _encode_value(w, v[key], field_schema)
        else:
            for value in v.values():
                _encode_value(w, value)
        return
    raise TypeError(f"不支持的载荷类型: {type(v).__name__}")


def encode_payload(value, schema=None) -> bytes:
    """Python 值 -> postcard 字节（schema 可选："list" 或字段类型 dict）"""
    w = bytearray()
    _encode_value(w, value, schema)
    return bytes(w)


class _Reader:
    def __init__(self, data: bytes):
        self.data = data
        self.pos = 0

    def eof(self) -> bool:
        return self.pos >= len(self.data)

    def remaining(self) -> int:
        return len(self.data) - self.pos

    def varint(self) -> int:
        result = 0
        shift = 0
        while True:
            if self.pos >= len(self.data):
                raise ValueError("varint 越界")
            b = self.data[self.pos]
            self.pos += 1
            result |= (b & 0x7F) << shift
            if not (b & 0x80):
                return result
            shift += 7
            if shift > 63:
                raise ValueError("varint 溢出")

    def take(self, n: int) -> bytes:
        if self.pos + n > len(self.data):
            raise ValueError("字节数据越界")
        out = self.data[self.pos : self.pos + n]
        self.pos += n
        return out

    def value(self, schema=None):
        if schema is None or schema == "auto" or schema == "json":
            return self.auto()
        if isinstance(schema, str):
            if schema in ("number", "int", "i64"):
                return _from_zigzag(self.varint())
            if schema == "bigint":
                return _from_zigzag(self.varint())
            if schema == "u64":
                return _js_number(self.varint())
            if schema == "bool":
                if self.pos >= len(self.data):
                    raise ValueError("bool 越界")
                b = self.data[self.pos]
                self.pos += 1
                return b != 0
            if schema == "string":
                return self.take(self.varint()).decode("utf-8")
            if schema == "bytes":
                return self.take(self.varint())
            if schema == "f64":
                return struct.unpack("<d", self.take(8))[0]
            if schema == "f32":
                return struct.unpack("<f", self.take(4))[0]
            if schema == "list":
                n = self.varint()
                return [self.auto() for _ in range(n)]
            raise ValueError(f"未知 schema: {schema}")
        if isinstance(schema, (list, tuple)):
            return [self.value(s) for s in schema]
        if isinstance(schema, dict):
            return {k: self.value(s) for k, s in schema.items()}
        raise ValueError("schema 必须是字符串 / 列表 / 字典")

    def auto(self):
        """智能推断：字符串优先（与 Rust 端 auto 规则一致）"""
        if self.eof():
            return None
        fields = []
        while not self.eof():
            start = self.pos
            v = self.varint()
            if v == 0:
                fields.append(0)
                continue
            if v <= self.remaining():
                data = self.take(v)
                try:
                    fields.append(data.decode("utf-8"))
                except UnicodeDecodeError:
                    fields.append(data)
                continue
            self.pos = start
            fields.append(_from_zigzag(self.varint()))
        return fields[0] if len(fields) == 1 else fields


def decode_payload(data: bytes, schema=None):
    """postcard 字节 -> Python 值（智能推断或显式 schema）"""
    r = _Reader(data)
    v = r.value(schema)
    if not r.eof():
        raise ValueError(f"载荷解码未消费完整: 剩余 {r.remaining()} 字节")
    return v

