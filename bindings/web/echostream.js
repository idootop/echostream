// EchoStream 浏览器 SDK —— 自动编解码、双传输（WebSocket / WebTransport）
//
// 用法：
//   import { EchoStream } from "./echostream.js";
//
//   const client = new EchoStream("ws://192.168.1.100:8081");  // 或 https://host:4433
//   await client.connect();
//
//   // RPC：参数自动编码（数组 -> 元组），响应自动解码 —— 无需手动编解码！
//   const sum = await client.request("add", 10, 20);        // 30
//   const text = await client.request("echo", "hi");        // "hi"
//   const user = await client.request("getUser", { id: 1 },
//     { decode: { id: "number", name: "string" } });        // 歧义场景可显式 schema
//
//   // 事件：自动编码/解码
//   await client.emit("hello", "world");
//   client.onEvent("hello", (data) => console.log("收到事件:", data));
//
//   // 双向 RPC：处理服务端主动调用（支持异步返回值）
//   client.onRpc("ping", async () => "pong");
//
//   // 流：帧自动编码/解码
//   const stream = await client.createStream("chat");
//   await stream.send("frame-1");
//   await stream.finish();
//   client.onStream("notice", async (stream) => {
//     let frame;
//     while ((frame = await stream.recv()) !== null) console.log("流帧:", frame);
//   });
//
// 载荷编码约定（与 echostream-proto::dynamic 一致）：
//   整数 -> i64 ZigZag；BigInt -> u64 varint；浮点 -> f64；布尔 -> 单字节；
//   字符串/字节 -> 长度前缀；数组 -> 元组字段序；对象 -> 结构体字段序。
// 解码默认智能推断；歧义场景传 options.decode 显式 schema。

import init, {
  ClientCoreHandle,
  encode_payload,
  decode_payload,
} from "./wasm/echostream_wasm.js";

let wasmReady = null;
function ensureWasm(input) {
  if (!wasmReady) {
    // 浏览器默认按相对路径加载；Node/测试等环境可注入 wasm 字节或模块
    wasmReady = input ? init({ module_or_path: input }) : init();
  }
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
    this._rpcHandlers = new Map();    // name -> (data) => value
    this._streamHandlers = new Map(); // name -> receiver
    this.wasmModule = options.wasmModule; // 可选注入（Node 等无 fetch 环境）
  }

  async connect() {
    await ensureWasm(this.wasmModule);
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

  /**
   * 发起 RPC 请求，响应自动解码。
   * 调用形式：
   *   request("add", 10, 20)            多参数自动组成元组 (i64, i64)
   *   request("add", [10, 20])          数组作为元组载荷
   *   request("echo", "hi")             标量载荷
   *   request("get", { id: 1 }, opts)   对象作为结构体载荷 + 选项
   * @param options { decode?, encode? } schema 或解码函数
   */
  async request(name, ...args) {
    const { payload, options } = splitArgs(args);
    const data = encodePayload(payload, options);
    const response = new Promise((resolve, reject) => {
      const frame = this.core.request(name, data, (respData, errMsg) => {
        if (errMsg !== null) {
          reject(new Error(errMsg));
        } else {
          resolve(decodeArgs(respData, options.decode));
        }
      });
      this._pendingFrame = frame; // 同步设置，await 前读取（见下）
    });
    if (this.isWs) {
      this.ws.send(this._pendingFrame);
      return response;
    }
    // wt：双向流承载请求/响应
    const frame = this._pendingFrame;
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

  /** 注册事件监听（载荷自动解码；元组载荷按参数展开） */
  onEvent(name, handler) {
    this.core.on_event(name, (eventName, data) => {
      handler(...spreadArgs(data));
    });
  }

  /** 发送事件（载荷自动编码） */
  async emit(name, payload, options = {}) {
    await this._send(this.core.build_event(name, encodePayload(payload, options)));
  }

  /** 发送不可靠事件（WebTransport/QUIC 数据报；WebSocket 降级可靠发送） */
  emitUnreliable(name, payload, options = {}) {
    const data = this.core.build_datagram_event(name, encodePayload(payload, options));
    if (this.isWs) {
      this.ws.send(data);
    } else if (this.transport) {
      this.transport.sendDatagram(data);
    }
  }

  // ======================== 双向 RPC（处理服务端主动调用） ========================

  /**
   * 注册 RPC 处理器（响应自动编码）
   * @param handler async (data) => value；返回 undefined 时回错误响应
   */
  onRpc(name, handler) {
    this._rpcHandlers.set(name, handler);
    this.core.on_rpc(name, (rpcName, data, id) => {
      const h = this._rpcHandlers.get(rpcName);
      if (!h) return null;
      try {
        const result = h(...spreadArgs(data));
        if (result === undefined) return null; // 无响应
        Promise.resolve(result)
          .then((value) => this._sendResponse(id, value))
          .catch((e) => this._sendError(id, e));
        return null; // 统一走异步响应路径
      } catch (e) {
        return this._errorFrame(id, e); // 同步异常：直接回错误帧
      }
    });
  }

  _sendResponse(id, value) {
    const bytes = encodePayload(value);
    this._send(this.core.build_response(id, bytes)).catch(() => {});
  }

  _sendError(id, err) {
    const msg = err instanceof Error ? err.message : String(err);
    this._send(this.core.build_error_response(id, msg)).catch(() => {});
  }

  // ======================== Stream ========================

  /** 创建出站流（帧自动编码） */
  async createStream(name) {
    const id = this.core.open_stream(name);
    return {
      name,
      send: async (payload) => {
        await this._send(this.core.build_stream_frame(id, name, encodePayload(payload), BigInt(Date.now())));
      },
      finish: async () => {
        if (this.isWs) {
          await this._send(this.core.build_stream_end(id));
        } else {
          const stream = await this.transport.createUnidirectionalStream();
          const writer = stream.getWriter();
          await writer.close();
        }
      },
    };
  }

  /**
   * 注册入站流处理器（服务端推送；帧自动解码）
   * @param handler async (stream) => void；stream.recv() 取下一帧，流结束返回 null
   */
  onStream(name, handler) {
    if (this._streamHandlers.has(name)) {
      throw new Error("同名流处理器已注册: " + name);
    }
    const receiver = createStreamReceiver();
    this._streamHandlers.set(name, receiver);
    this.core.on_stream(name, (frame) => {
      if (frame === null) {
        receiver.push(null);
      } else {
        receiver.push(decodePayload(frame));
      }
    });
    handler(receiver).catch((e) => console.error("[echostream] 流处理器出错:", e));
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
}

// ======================== 参数解析 ========================

/** 解析 request/emit 的多参数形式：payload | (payload, options) | 多参数 -> 元组 */
function splitArgs(args) {
  if (args.length === 0) return { payload: undefined, options: {} };
  if (args.length === 1) return { payload: args[0], options: {} };
  if (args.length === 2 && isOptions(args[1])) {
    return { payload: args[0], options: args[1] };
  }
  return { payload: args, options: {} }; // 多参数 -> 元组载荷
}

/** 是否为请求选项对象（含 decode / encode 键） */
function isOptions(v) {
  return (
    typeof v === "object" && v !== null && !Array.isArray(v) &&
    (v.decode !== undefined || v.encode !== undefined)
  );
}

// ======================== 自动编解码 ========================

/**
 * 载荷编码（options.encode 为显式 schema，如 "list" 表示长度前缀列表）
 */
function encodePayload(payload, options = {}) {
  const schema = options.encode;
  const value = payload === undefined ? null : payload;
  if (schema === "list") {
    return encodeList(value);
  }
  if (typeof schema === "object" && schema !== null && !Array.isArray(schema)) {
    return encodeStruct(value, schema);
  }
  return encode_payload(value);
}

/** 结构体编码（显式字段 schema，如 { items: "list" }） */
function encodeStruct(value, schema) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("结构体编码需要对象载荷");
  }
  const parts = [];
  for (const key of Object.keys(schema)) {
    const fieldSchema = schema[key];
    const fieldValue = value[key];
    if (fieldSchema === "list") {
      parts.push(encodeList(fieldValue));
    } else if (typeof fieldSchema === "object" && fieldSchema !== null && !Array.isArray(fieldSchema)) {
      parts.push(encodeStruct(fieldValue, fieldSchema));
    } else {
      parts.push(encode_payload(fieldValue === undefined ? null : fieldValue));
    }
  }
  return concatBytes(parts);
}

/** 列表编码（长度前缀 + 各元素） */
function encodeList(items) {
  if (!Array.isArray(items)) throw new Error("列表编码需要数组载荷");
  const parts = [encodeVarint(items.length)];
  for (const item of items) {
    parts.push(encode_payload(item === undefined ? null : item));
  }
  return concatBytes(parts);
}

function encodeVarint(n) {
  const out = [];
  let v = BigInt(n);
  while (v >= 0x80n) {
    out.push(Number((v & 0x7fn) | 0x80n));
    v >>= 7n;
  }
  out.push(Number(v));
  return new Uint8Array(out);
}

function concatBytes(parts) {
  let len = 0;
  for (const p of parts) len += p.length;
  const out = new Uint8Array(len);
  let pos = 0;
  for (const p of parts) {
    out.set(p, pos);
    pos += p.length;
  }
  return out;
}

/**
 * 响应解码：decode 为 schema（字符串/数组/对象/函数）或 undefined（智能推断）
 */
function decodeArgs(bytes, decode) {
  if (decode === undefined || decode === "auto") {
    return decodePayload(bytes);
  }
  if (typeof decode === "function") {
    return decode(bytes);
  }
  return decodePayload(bytes, decode);
}

function decodePayload(bytes, schema) {
  return decode_payload(bytes, schema === undefined ? undefined : schema);
}

/** 解码并按元组约定展开为多参数（空载荷 -> 无参数） */
function spreadArgs(bytes) {
  const decoded = decodePayload(bytes);
  if (decoded === undefined || decoded === null) return [];
  return Array.isArray(decoded) ? decoded : [decoded];
}

// ======================== 入站流接收器 ========================

function createStreamReceiver() {
  let queue = [];   // 帧队列（null 标记流结束）
  let waiters = []; // 等待中的 recv() 调用
  let ended = false;
  return {
    async recv() {
      if (queue.length > 0) return queue.shift();
      if (ended) return null;
      return new Promise((resolve) => waiters.push(resolve));
    },
    push(value) {
      if (value === null) {
        ended = true;
        for (const w of waiters) w(null);
        waiters = [];
        return;
      }
      if (waiters.length > 0) {
        waiters.shift()(value);
      } else {
        queue.push(value);
      }
    },
  };
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
