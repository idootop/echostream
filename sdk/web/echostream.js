// EchoStream 浏览器客户端 SDK
//
// 基于 WebTransport（HTTP/3 + QUIC），与 Rust 服务端（echostream-web）互通。
// 协议编解码由 Rust 编译的 WASM 提供（bindings/wasm），JS 只负责网络层，
// 保证与 Rust 服务端的线缆格式单一事实来源、永不漂移。
//
// 用法：
//   import { EchoStream } from "./echostream.js";
//   const client = new EchoStream("https://127.0.0.1:4433");
//   await client.connect();
//   const data = await client.request("add", [10, 20], { decode: "bytes" });
//   client.onEvent("hello", (data) => console.log(new TextDecoder().decode(data)));
//   await client.emit("join", "alice");
//   const stream = await client.createStream("chat");
//   await stream.send("hi"); await stream.finish();

import init, {
  encode_payload,
  encode_message,
  decode_message,
  encode_frame,
  decode_u64,
  decode_string,
  decode_bytes,
} from "./wasm/echostream_wasm.js";

let wasmReady = null;
function ensureWasm() {
  if (!wasmReady) wasmReady = init();
  return wasmReady;
}

export class EchoStream {
  constructor(url) {
    this.url = url;
    this.transport = null;
    this.nextId = 1n;
    this.eventHandlers = new Map(); // name -> [handler]
    this.streamHandlers = new Map(); // name -> [handler]
  }

  /** 连接服务端（自签名证书需先访问 https://host:port 信任） */
  async connect() {
    await ensureWasm();
    this.transport = new WebTransport(this.url);
    await this.transport.ready;
    this._receiveLoop();
  }

  async close() {
    if (this.transport) {
      try { this.transport.close(); } catch (_) { /* ignore */ }
    }
  }

  // ======================== RPC ========================

  /** 发起 RPC 请求；options.decode: "bytes" | "string" | "number" | 自定义函数 */
  async request(name, payload, options = {}) {
    const stream = await this.transport.createBidirectionalStream();
    const writer = stream.writable.getWriter();
    const id = this.nextId++;
    await writer.write(encode_frame({
      type: "request",
      id,
      name,
      data: this._toBytes(payload),
    }));
    await writer.close();

    const reader = stream.readable.getReader();
    const msg = await readFrame(reader);
    reader.releaseLock();
    if (msg === null) throw new Error("连接已关闭");
    if (msg.type !== "response" || msg.id !== id) throw new Error("响应不匹配");
    if (msg.code !== 0) throw new Error(`RPC 错误 (${msg.code}): ${msg.message ?? "unknown"}`);
    return this._fromBytes(msg.data, options.decode);
  }

  // ======================== Event ========================

  /** 注册事件监听（回调收到事件载荷） */
  onEvent(name, handler) {
    const list = this.eventHandlers.get(name) ?? [];
    list.push(handler);
    this.eventHandlers.set(name, list);
  }

  /** 发送单向事件 */
  async emit(name, payload) {
    const stream = await this.transport.createUnidirectionalStream();
    const writer = stream.getWriter();
    await writer.write(encode_frame({
      type: "event",
      id: this.nextId++,
      name,
      data: this._toBytes(payload),
    }));
    await writer.close();
  }

  // ======================== Stream ========================

  /** 注册流处理器（接收服务端推送的流） */
  onStream(name, handler) {
    const list = this.streamHandlers.get(name) ?? [];
    list.push(handler);
    this.streamHandlers.set(name, list);
  }

  /** 创建流（推送连续数据） */
  async createStream(name) {
    const stream = await this.transport.createUnidirectionalStream();
    const writer = stream.getWriter();
    const id = this.nextId++;
    let seq = 0n;
    return {
      send: async (payload) => {
        await writer.write(encode_frame({
          type: "stream",
          id,
          name,
          seq: seq++,
          senderTs: BigInt(Date.now()),
          data: this._toBytes(payload),
        }));
      },
      finish: async () => {
        await writer.close();
      },
    };
  }

  // ======================== 内部 ========================

  /** 后台接收循环：处理服务端主动发来的 RPC / 事件 / 流 */
  _receiveLoop() {
    // 事件与流的 uni 流
    (async () => {
      try {
        const reader = this.transport.incomingUnidirectionalStreams.getReader();
        while (true) {
          const { value: stream, done } = await reader.read();
          if (done) break;
          this._handleUni(stream).catch(() => {});
        }
      } catch (_) { /* 连接关闭 */ }
    })();
    // 服务端主动 RPC 的 bi 流
    (async () => {
      try {
        const reader = this.transport.incomingBidirectionalStreams.getReader();
        while (true) {
          const { value: stream, done } = await reader.read();
          if (done) break;
          this._handleBi(stream).catch(() => {});
        }
      } catch (_) { /* 连接关闭 */ }
    })();
  }

  async _handleUni(stream) {
    const reader = stream.getReader();
    let msg;
    while ((msg = await readFrame(reader)) !== null) {
      if (msg.type === "event") {
        const handlers = this.eventHandlers.get(msg.name) ?? [];
        for (const h of handlers) h(this._fromBytes(msg.data), msg);
      } else if (msg.type === "stream") {
        const handlers = this.streamHandlers.get(msg.name) ?? [];
        for (const h of handlers) h(this._streamReceiver(reader, msg), msg);
      }
    }
  }

  async _handleBi(stream) {
    const reader = stream.readable.getReader();
    const msg = await readFrame(reader);
    if (msg === null) return;
    if (msg.type === "request") {
      // 服务端主动 RPC：调用注册的处理器并回写响应
      const handlers = this.eventHandlers.get(`rpc:${msg.name}`) ?? [];
      const writer = stream.writable.getWriter();
      let resp;
      if (handlers.length > 0) {
        try {
          const data = this._fromBytes(msg.data);
          const result = await handlers[0](data);
          resp = { type: "response", id: msg.id, code: 0, message: null, data: this._toBytes(result) };
        } catch (e) {
          resp = { type: "response", id: msg.id, code: 1, message: String(e), data: new Uint8Array(0) };
        }
      } else {
        resp = { type: "response", id: msg.id, code: 3, message: "handler not found", data: new Uint8Array(0) };
      }
      await writer.write(encode_frame(resp));
      await writer.close();
    }
  }

  _streamReceiver(reader, first) {
    let pending = first;
    return {
      async recv() {
        if (pending) {
          const p = pending;
          pending = null;
          return p;
        }
        return readFrame(reader);
      },
    };
  }

  _toBytes(payload) {
    if (payload instanceof Uint8Array) return payload;
    return encode_payload(payload);
  }

  _fromBytes(data, decode) {
    if (decode === "string") return decode_string(data);
    if (decode === "bytes") return data;
    if (decode === "number") return decode_u64(data);
    if (typeof decode === "function") return decode(data);
    return data;
  }
}

// ======================== 帧读取（网络层，JS 实现） ========================

export async function readFrame(reader) {
  const lenBytes = await readExactly(reader, 4);
  if (lenBytes === null) return null;
  const len = new DataView(lenBytes.buffer).getUint32(0, true);
  const payload = await readExactly(reader, len);
  if (payload === null) throw new Error("帧数据不完整");
  return decode_message(payload);
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
