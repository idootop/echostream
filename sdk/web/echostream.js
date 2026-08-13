// EchoStream 浏览器客户端 SDK
//
// 架构：协议编解码 + 客户端状态机（RPC 匹配/事件路由/流管理）全部由
// Rust 编译的 WASM 提供（bindings/wasm），JS 只负责 WebTransport 网络层：
// 读帧 → core.handle_inbound，写帧 ← core 的 build 方法。
// 与 Rust 原生客户端共享同一份核心逻辑，单一事实来源。
//
// 用法：
//   import { EchoStream } from "./echostream.js";
//   const client = new EchoStream("https://127.0.0.1:4433");
//   await client.connect();
//   const data = await client.request("add", [10, 20], { decode: "bytes" });
//   client.on_event("hello", (data) => console.log(new TextDecoder().decode(data)));
//   await client.emit("join", "alice");

import init, {
  ClientCoreHandle,
  encode_payload,
  decode_u64,
  decode_string,
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
    this.core = null;
    this.streams = new Map(); // JS 侧流句柄（网络层状态：writer）
  }

  /** 连接服务端（自签名证书需先访问 https://host:port 信任） */
  async connect() {
    await ensureWasm();
    this.core = new ClientCoreHandle();
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

    // 状态机分配 id、注册响应回调；同步返回请求帧
    let frame;
    const response = new Promise((resolve) => {
      const data = this._toBytes(payload);
      frame = this.core.request(name, data, (respData) => {
        // 响应回调（状态机匹配 id 后触发，resolve 响应值）
        resolve(this._fromBytes(respData, options.decode));
      });
    });
    await writer.write(frame);
    await writer.close();

    // 读响应帧并喂给状态机（触发上方响应回调）
    const reader = stream.readable.getReader();
    const msg = await readFrame(reader);
    if (msg === null) throw new Error("连接已关闭");
    this.core.handle_inbound(msg);
    reader.releaseLock();
    return response;
  }

  // ======================== Event ========================

  /** 注册事件监听（回调收到事件载荷） */
  on_event(name, handler) {
    this.core.on_event(name, (eventName, data) => {
      handler(this._fromBytes(data), eventName);
    });
  }

  /** 发送单向事件 */
  async emit(name, payload) {
    const stream = await this.transport.createUnidirectionalStream();
    const writer = stream.getWriter();
    const frame = this.core.build_event(name, this._toBytes(payload));
    await writer.write(frame);
    await writer.close();
  }

  // ======================== Stream ========================

  /** 创建流（推送连续数据，序号由状态机管理） */
  async createStream(name) {
    const stream = await this.transport.createUnidirectionalStream();
    const writer = stream.getWriter();
    const id = this.core.open_stream(name);
    const sender = {
      send: async (payload) => {
        const frame = this.core.build_stream_frame(id, name, this._toBytes(payload), BigInt(Date.now()));
        await writer.write(frame);
      },
      finish: async () => {
        await writer.close();
      },
    };
    this.streams.set(id, sender);
    return sender;
  }

  // ======================== 内部 ========================

  /** 后台接收循环：读帧 → 状态机分发（事件/流/服务端主动 RPC） */
  _receiveLoop() {
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
    let payload;
    while ((payload = await readFrame(reader)) !== null) {
      this.core.handle_inbound(payload); // 事件/流分发（状态机内部路由）
    }
  }

  async _handleBi(stream) {
    // 服务端主动 RPC：读一帧 → 状态机处理 → 响应帧写回
    const reader = stream.readable.getReader();
    const payload = await readFrame(reader);
    if (payload === null) return;
    const outbound = this.core.handle_inbound(payload);
    if (outbound) {
      const writer = stream.writable.getWriter();
      await writer.write(outbound);
      await writer.close();
    }
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

/** 读取一帧，返回消息载荷字节（喂给 WASM 状态机解码） */
export async function readFrame(reader) {
  const lenBytes = await readExactly(reader, 4);
  if (lenBytes === null) return null;
  const len = new DataView(lenBytes.buffer).getUint32(0, true);
  const payload = await readExactly(reader, len);
  if (payload === null) throw new Error("帧数据不完整");
  return payload;
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
