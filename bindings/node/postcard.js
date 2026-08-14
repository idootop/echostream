// EchoStream Node.js 载荷编解码（纯 JS 实现，与 echostream-proto::dynamic 语义一致）
//
// 编码约定（JS 值 -> postcard 字节）：
// - 整数（含负数）-> i64 ZigZag varint；BigInt -> u64 普通 varint
// - 浮点数 -> f64 小端；布尔 -> 单字节；字符串/字节 -> 长度前缀
// - 数组 -> 元组/结构体字段序；对象 -> 结构体字段序；null/undefined -> 空载荷
//
// 解码默认智能推断；歧义场景传 schema（"number"|"bigint"|"u64"|"bool"|
// "string"|"bytes"|"f64"|"f32"|"list"|数组=元组|对象=结构体）。
// 与 bindings/wasm 的 encode_payload/decode_payload 交叉验证保持一致。

// ======================== 编码 ========================

export function encodePayload(value, schema) {
  const w = new Writer();
  if (schema === "list") {
    if (!Array.isArray(value)) throw new Error("列表编码需要数组载荷");
    w.varint(value.length);
    for (const item of value) w.value(item === undefined ? null : item);
  } else if (typeof schema === "object" && schema !== null && !Array.isArray(schema)) {
    if (value === null || typeof value !== "object" || Array.isArray(value))
      throw new Error("结构体编码需要对象载荷");
    for (const key of Object.keys(schema)) {
      const fieldSchema = schema[key];
      const fieldValue = value[key];
      if (fieldSchema === "list") {
        if (!Array.isArray(fieldValue)) throw new Error("列表编码需要数组载荷");
        w.varint(fieldValue.length);
        for (const item of fieldValue) w.value(item === undefined ? null : item);
      } else {
        w.value(fieldValue === undefined ? null : fieldValue);
      }
    }
  } else {
    w.value(value === undefined ? null : value);
  }
  return w.bytes;
}

class Writer {
  constructor() { this.bytes = new Uint8Array(0); }

  push(...bytes) {
    const out = new Uint8Array(this.bytes.length + bytes.length);
    out.set(this.bytes, 0);
    out.set(bytes, this.bytes.length);
    this.bytes = out;
  }

  extend(data) {
    const out = new Uint8Array(this.bytes.length + data.length);
    out.set(this.bytes, 0);
    out.set(data, this.bytes.length);
    this.bytes = out;
  }

  varint(n) {
    let v = BigInt(n);
    const out = [];
    while (v >= 0x80n) {
      out.push(Number((v & 0x7fn) | 0x80n));
      v >>= 7n;
    }
    out.push(Number(v));
    this.push(...out);
  }

  value(v) {
    if (v === null || v === undefined) return;
    if (typeof v === "boolean") { this.push(v ? 1 : 0); return; }
    if (typeof v === "number") {
      if (!Number.isFinite(v)) throw new Error("载荷 number 必须是有限值");
      if (Number.isInteger(v) && Math.abs(v) <= Number.MAX_SAFE_INTEGER) {
        // 整数 -> i64 ZigZag 约定
        const n = BigInt(v);
        this.varint(BigInt.asUintN(64, (n << 1n) ^ (n >> 63n)));
      } else {
        // 浮点 -> f64 小端
        const buf = new ArrayBuffer(8);
        new DataView(buf).setFloat64(0, v, true);
        this.extend(new Uint8Array(buf));
      }
      return;
    }
    if (typeof v === "bigint") {
      if (v >= 0n) { this.varint(v); } else { this.varint(BigInt.asUintN(64, (v << 1n) ^ (v >> 63n))); }
      return;
    }
    if (typeof v === "string") {
      const bytes = new TextEncoder().encode(v);
      this.varint(bytes.length);
      this.extend(bytes);
      return;
    }
    if (v instanceof Uint8Array) {
      this.varint(v.length);
      this.extend(v);
      return;
    }
    if (Array.isArray(v)) {
      for (const item of v) this.value(item === undefined ? null : item);
      return;
    }
    if (typeof v === "object") {
      for (const key of Object.keys(v)) this.value(v[key] === undefined ? null : v[key]);
      return;
    }
    throw new Error("不支持的载荷类型: " + typeof v);
  }
}

// ======================== 解码 ========================

export function decodePayload(bytes, schema) {
  const r = new Reader(bytes);
  const v = schema === undefined || schema === "auto" || schema === "json"
    ? r.auto()
    : r.schema(schema);
  if (!r.eof()) throw new Error("载荷解码未消费完整: 剩余 " + r.remaining() + " 字节");
  return v;
}

class Reader {
  constructor(bytes) {
    this.bytes = bytes instanceof Uint8Array ? bytes : Uint8Array.from(bytes);
    this.pos = 0;
  }

  eof() { return this.pos >= this.bytes.length; }
  remaining() { return this.bytes.length - this.pos; }

  varint() {
    let result = 0n;
    let shift = 0n;
    for (;;) {
      if (this.pos >= this.bytes.length) throw new Error("varint 越界");
      const b = this.bytes[this.pos++];
      result |= BigInt(b & 0x7f) << shift;
      if ((b & 0x80) === 0) break;
      shift += 7n;
      if (shift > 63n) throw new Error("varint 溢出");
    }
    return result;
  }

  take(len) {
    if (this.pos + len > this.bytes.length) throw new Error("字节数据越界");
    const out = this.bytes.subarray(this.pos, this.pos + len);
    this.pos += len;
    return out;
  }

  fromZigzag(v) { return BigInt.asIntN(64, (v >> 1n) ^ -(v & 1n)); }

  number(v) {
    const n = this.fromZigzag(v);
    return n >= BigInt(Number.MIN_SAFE_INTEGER) && n <= BigInt(Number.MAX_SAFE_INTEGER)
      ? Number(n) : n;
  }

  schema(s) {
    if (typeof s === "string") {
      switch (s) {
        case "number": case "int": case "i64": return this.number(this.varint());
        case "bigint": return this.fromZigzag(this.varint());
        case "u64": {
          const n = this.varint();
          return n >= BigInt(Number.MIN_SAFE_INTEGER) && n <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(n) : n;
        }
        case "bool": {
          if (this.pos >= this.bytes.length) throw new Error("bool 越界");
          return this.bytes[this.pos++] !== 0;
        }
        case "string": {
          const len = Number(this.varint());
          return new TextDecoder().decode(this.take(len));
        }
        case "bytes": {
          const len = Number(this.varint());
          return new Uint8Array(this.take(len));
        }
        case "f64": {
          const buf = new Uint8Array(this.take(8)).buffer;
          return new DataView(buf).getFloat64(0, true);
        }
        case "f32": {
          const buf = new Uint8Array(this.take(4)).buffer;
          return new DataView(buf).getFloat32(0, true);
        }
        case "list": {
          const len = Number(this.varint());
          const items = [];
          for (let i = 0; i < len; i++) items.push(this.auto());
          return items;
        }
        default: throw new Error("未知 schema: " + s);
      }
    }
    if (Array.isArray(s)) {
      const items = [];
      for (const item of s) items.push(this.schema(item));
      return items;
    }
    if (typeof s === "object") {
      const obj = {};
      for (const key of Object.keys(s)) obj[key] = this.schema(s[key]);
      return obj;
    }
    throw new Error("schema 必须是字符串 / 数组 / 对象");
  }

  // 智能推断：字符串优先（与 Rust 端 auto 规则一致）
  auto() {
    if (this.eof()) return undefined;
    const fields = [];
    for (;;) {
      if (this.eof()) break;
      const start = this.pos;
      const v = this.varint();
      if (v === 0n) { fields.push(0); continue; }
      if (v <= BigInt(this.remaining())) {
        const data = this.take(Number(v));
        try {
          const s = new TextDecoder("utf-8", { fatal: true }).decode(data);
          fields.push(s);
        } catch (_) {
          fields.push(new Uint8Array(data));
        }
        continue;
      }
      this.pos = start;
      fields.push(this.number(this.varint()));
    }
    return fields.length === 1 ? fields[0] : fields;
  }
}
