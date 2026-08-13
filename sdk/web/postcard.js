// EchoStream 协议编解码（JS 端，与 Rust postcard 线缆格式兼容）
//
// Message 枚举（variant 为 varint）：
//   0 = Request  { id: u64, name: String, data: Bytes }
//   1 = Response { id: u64, code: u16, message: Option<String>, data: Bytes }
//   2 = Event    { id: u64, name: String, data: Bytes }
//   3 = Stream   { id: u64, name: String, seq: u64, sender_ts: u64, data: Bytes }

// ======================== 基础编码 ========================

export function encode(value) {
  // 载荷便捷编码：支持 number(u32/i64 范围内)、bigint、string、Uint8Array、array
  const enc = new Writer();
  enc.value(value);
  return enc.finish();
}

export class Writer {
  constructor() {
    this.bytes = [];
  }

  finish() {
    return new Uint8Array(this.bytes);
  }

  varint(n) {
    while (n >= 0x80) {
      this.bytes.push((n & 0x7f) | 0x80);
      n = Math.floor(n / 128);
    }
    this.bytes.push(n);
  }

  bigint(n) {
    while (n >= 128n) {
      this.bytes.push(Number((n & 127n) | 128n));
      n >>= 7n;
    }
    this.bytes.push(Number(n));
  }

  putBytes(data) {
    this.varint(data.length);
    for (const b of data) this.bytes.push(b);
  }

  string(s) {
    this.putBytes(new TextEncoder().encode(s));
  }

  optionString(s) {
    if (s === null || s === undefined) {
      this.bytes.push(0);
    } else {
      this.bytes.push(1);
      this.string(s);
    }
  }

  value(v) {
    // 数组按 Rust 元组/结构体语义：定长、顺序编码、无长度前缀
    if (typeof v === "number") {
      if (!Number.isInteger(v)) throw new Error("postcard: 请使用整数");
      // 负数按 zigzag（Rust 有符号整数），正数按无符号 varint（Rust u32/u64）
      if (v >= 0) this.varint(v);
      else this.varint(v * -2 - 1);
    } else if (typeof v === "bigint") {
      if (v >= 0n) this.bigint(v);
      else this.bigint(v * -2n - 1n);
    } else if (typeof v === "string") {
      this.string(v);
    } else if (v instanceof Uint8Array) {
      this.bytes(v);
    } else if (Array.isArray(v)) {
      for (const item of v) this.value(item);
    } else {
      throw new Error(`postcard: 不支持的类型 ${typeof v}`);
    }
  }
}

// ======================== 消息编码 ========================

export function encodeMessage(msg) {
  const w = new Writer();
  switch (msg.type) {
    case "request":
      w.varint(0);
      w.bigint(msg.id);
      w.string(msg.name);
      w.putBytes(msg.data);
      break;
    case "response":
      w.varint(1);
      w.bigint(msg.id);
      w.varint(msg.code);
      w.optionString(msg.message);
      w.putBytes(msg.data);
      break;
    case "event":
      w.varint(2);
      w.bigint(msg.id);
      w.string(msg.name);
      w.putBytes(msg.data);
      break;
    case "stream":
      w.varint(3);
      w.bigint(msg.id);
      w.string(msg.name);
      w.bigint(msg.seq);
      w.bigint(msg.senderTs);
      w.putBytes(msg.data);
      break;
    default:
      throw new Error(`未知消息类型: ${msg.type}`);
  }
  return w.finish();
}

// ======================== 消息解码 ========================

export class Reader {
  constructor(bytes) {
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    this.pos = 0;
  }

  varint() {
    let result = 0;
    let shift = 0;
    while (true) {
      const b = this.view.getUint8(this.pos++);
      result += (b & 0x7f) * Math.pow(2, shift);
      if ((b & 0x80) === 0) break;
      shift += 7;
      if (shift > 63) throw new Error("varint 溢出");
    }
    return result;
  }

  bigint() {
    let result = 0n;
    let shift = 0n;
    while (true) {
      const b = this.view.getUint8(this.pos++);
      result |= BigInt(b & 0x7f) << shift;
      if ((b & 0x80) === 0) break;
      shift += 7n;
    }
    return result;
  }

  bytes() {
    const len = this.varint();
    const out = new Uint8Array(len);
    for (let i = 0; i < len; i++) out[i] = this.view.getUint8(this.pos++);
    return out;
  }

  string() {
    return new TextDecoder().decode(this.bytes());
  }

  optionString() {
    const tag = this.view.getUint8(this.pos++);
    if (tag === 0) return null;
    if (tag === 1) return this.string();
    throw new Error("Option 标记无效");
  }
}

export function decodeMessage(bytes) {
  const r = new Reader(bytes);
  const variant = r.varint();
  switch (variant) {
    case 0:
      return { type: "request", id: r.bigint(), name: r.string(), data: r.bytes() };
    case 1:
      return {
        type: "response",
        id: r.bigint(),
        code: r.varint(),
        message: r.optionString(),
        data: r.bytes(),
      };
    case 2:
      return { type: "event", id: r.bigint(), name: r.string(), data: r.bytes() };
    case 3:
      return {
        type: "stream",
        id: r.bigint(),
        name: r.string(),
        seq: r.bigint(),
        senderTs: r.bigint(),
        data: r.bytes(),
      };
    default:
      throw new Error(`未知消息 variant: ${variant}`);
  }
}

// ======================== 帧编解码（长度前缀 + 消息） ========================

export function encodeFrame(msg) {
  const payload = encodeMessage(msg);
  const frame = new Uint8Array(4 + payload.length);
  new DataView(frame.buffer).setUint32(0, payload.length, true);
  frame.set(payload, 4);
  return frame;
}

export async function readFrame(reader) {
  // reader: WebTransport ReadableStreamDefaultReader
  const lenBytes = await readExactly(reader, 4);
  if (lenBytes === null) return null; // 流正常结束
  const len = new DataView(lenBytes.buffer).getUint32(0, true);
  const payload = await readExactly(reader, len);
  if (payload === null) throw new Error("帧数据不完整");
  return decodeMessage(payload);
}

async function readExactly(reader, len) {
  const buf = new Uint8Array(len);
  let got = 0;
  while (got < len) {
    const { value, done } = await reader.read();
    if (done) return got === 0 ? null : (() => { throw new Error("帧数据不完整"); })();
    buf.set(value.subarray(0, len - got), got);
    got += value.length;
  }
  return buf;
}
