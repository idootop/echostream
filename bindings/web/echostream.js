// EchoStream 浏览器 SDK
//
// 传输层双模式（核心状态机与协议编解码由 Rust WASM 提供，完全复用）：
//  - ws:// —— 局域网零证书通信（WebSocket，第一优先级场景）
//  - https:// —— WebTransport（公网可信证书场景）
//
// 用法：
//   const client = new EchoStream("ws://192.168.1.100:8081");
//   await client.connect();
//   const sum = await client.request("add", [10, 20], { decode: "number" });
//   client.onEvent("hello", (data) => console.log(new TextDecoder().decode(data)));
//   const stream = await client.createStream("chat");
//   await stream.send("hi"); await stream.finish();

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
  constructor(url, options = {}) {
    this.url = url;
    this.options = options;
    this.core = null;
    this.ws = null;        // WebSocket 传输
    this.transport = null; // WebTransport 传输
    this.isWs = url.startsWith("ws:");
  }

  async connect() {
    await ensureWasm();
    this.core = new ClientCoreHandle();
    if (this.isWs) {
      await this._connectWs();
    } else {
      await this._connectWt();
    }
  }

  async close() {
    if (this.ws) { try { this.ws.close(); } catch (_) {} }
    if (this.transport) { try { this.transport.close(); } catch (_) {} }
  }

  // ======================== WebSocket 传输 ========================

  async _connectWs() {
    this.ws = new WebSocket(this.url);
    this.ws.binaryType = "arraybuffer";
    await new Promise((resolve, reject) => {
      this.ws.onopen = resolve;
      this.ws.onerror = () => reject(new Error("WebSocket 连接失败"));
    });
    this.ws.onmessage = (e) => {
      const outbound = this.core.handle_inbound(new Uint8Array(e.data));
      if (outbound) this.ws.send(outbound); // 服务端主动调用的响应
    };
  }

  // ======================== WebTransport 传输 ========================

  async _connectWt() {
    this.transport = new WebTransport(this.url, this.options);
    await this.transport.ready;
    this._receiveLoop();
  }

  _receiveLoop() {
    (async () => {
      try {
        const reader = this.transport.incomingUnidirectionalStreams.getReader();
        while (true) {
          const { value: stream, done } = await reader.read();
          if (done) break;
          this._handleUni(stream).catch(() => {});
        }
      } catch (_) {}
    })();
    (async () => {
      try {
        const reader = this.transport.incomingBidirectionalStreams.getReader();
        while (true) {
          const { value: stream, done } = await reader.read();
          if (done) break;
          this._handleBi(stream).catch(() => {});
        }
      } catch (_) {}
    })();
  }

  async _handleUni(stream) {
    const reader = stream.getReader();
    let payload;
    while ((payload = await readFrame(reader)) !== null) {
      this.core.handle_inbound(payload);
    }
  }

  async _handleBi(stream) {
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

  // ======================== RPC ========================

  async request(name, payload, options = {}) {
    let frame;
    const response = new Promise((resolve) => {
      const data = this._toBytes(payload);
      frame = this.core.request(name, data, (respData) => {
        resolve(this._fromBytes(respData, options.decode));
      });
    });
    await this._send(frame);
    if (this.isWs) return response; // ws：响应由状态机回调 resolve
    // wt：读响应帧喂状态机
    const stream = await this.transport.createBidirectionalStream();
    const writer = stream.writable.getWriter();
    await writer.write(frame);
    await writer.close();
    const reader = stream.readable.getReader();
    const msg = await readFrame(reader);
    if (msg === null) throw new Error("连接已关闭");
    this.core.handle_inbound(msg);
    reader.releaseLock();
    return response;
  }

  // ======================== Event ========================

  onEvent(name, handler) {
    this.core.on_event(name, (eventName, data) => {
      handler(this._fromBytes(data), eventName);
    });
  }

  async emit(name, payload) {
    await this._send(this.core.build_event(name, this._toBytes(payload)));
  }

  /** 发送不可靠事件（WebTransport/QUIC 数据报通道，吞吐更高；WebSocket 降级为可靠发送） */
  emitUnreliable(name, payload) {
    const data = this.core.build_datagram_event(name, this._toBytes(payload));
    if (this.isWs) {
      this.ws.send(data); // ws 无数据报通道：降级可靠发送
    } else if (this.transport) {
      this.transport.sendDatagram(data);
    }
  }

  // ======================== Stream ========================

  async createStream(name) {
    const id = this.core.open_stream(name);
    return {
      send: async (payload) => {
        await this._send(this.core.build_stream_frame(id, name, this._toBytes(payload), BigInt(Date.now())));
      },
      finish: async () => {
        if (this.isWs) {
          await this._send(this.core.build_stream_end(id)); // ws 显式结束标记
        } else {
          const stream = await this.transport.createUnidirectionalStream();
          const writer = stream.getWriter();
          await writer.close();
        }
      },
    };
  }

  // ======================== 内部 ========================

  /** 发送一帧（ws：直接发送；wt：单向流） */
  async _send(frame) {
    if (this.isWs) {
      this.ws.send(frame);
      return;
    }
    const stream = await this.transport.createUnidirectionalStream();
    const writer = stream.getWriter();
    await writer.write(frame);
    await writer.close();
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

// ======================== 帧读取（WebTransport 网络层） ========================

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
