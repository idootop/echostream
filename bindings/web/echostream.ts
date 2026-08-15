// EchoStream 浏览器 SDK（TypeScript）—— 自动编解码、双传输（WebSocket / WebTransport）
//
// 用法：
//   import { EchoStream } from "./echostream.js";
//
//   const client = new EchoStream("ws://192.168.1.100:8081");  // 或 https://host:4433
//   await client.connect();
//
//   // RPC：参数自动编码（数组 -> 元组），响应自动解码 —— 无需手动编解码！
//   const sum: number = await client.request<number>("add", 10, 20);  // 30
//   const text = await client.request<string>("echo", "hi");          // "hi"
//   const user = await client.request<{ id: number; name: string }>("getUser", { id: 1 },
//     { decode: { id: "number", name: "string" } });                  // 歧义场景可显式 schema
//
//   // 事件：自动编码/解码
//   await client.emit("hello", "world");
//   client.onEvent<[string]>("hello", ([msg]) => console.log("收到事件:", msg));
//
//   // 双向 RPC：处理服务端主动调用（支持异步返回值）
//   client.onRpc<[], string>("ping", async () => "pong");
//
//   // 流：帧自动编码/解码
//   const stream = await client.createStream("chat");
//   await stream.send("frame-1");
//   await stream.finish();
//   client.onStream<string>("notice", async (stream) => {
//     let frame: string | null;
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

// ======================== 公共类型 ========================

/** 载荷 schema（与 postcard 约定一致） */
export type Schema =
  | "number" | "int" | "i64" | "bigint" | "u64" | "bool"
  | "string" | "bytes" | "f64" | "f32" | "list" | "auto" | "json"
  | Schema[] | { [key: string]: Schema };

/** 请求选项：decode/encode 为显式 schema 或自定义解码函数 */
export interface RequestOptions {
  decode?: Schema | ((bytes: Uint8Array) => unknown);
  encode?: Schema | "list";
}

/** SDK 构造选项：wasmModule 可注入 wasm 字节（Node/测试等无 fetch 环境） */
export interface EchoStreamOptions {
  /** wasm 模块字节或加载路径（默认浏览器按相对路径加载） */
  wasmModule?: unknown;
  /** 其余选项透传给 WebTransport */
  [key: string]: unknown;
}

/** 入站流接收器（帧自动解码；流结束返回 null） */
export interface InboundStream<T = unknown> {
  recv(): Promise<T | null>;
}

/** 出站流（帧自动编码） */
export interface OutboundStream {
  name: string;
  send<T = unknown>(payload: T, options?: RequestOptions): Promise<void>;
  finish(): Promise<void>;
}

/** 帧读取器（WebTransport 网络层；4 字节长度前缀 + 载荷） */
export interface FrameReader {
  read(): Promise<{ value?: Uint8Array; done: boolean }>;
}

/** WebTransport 数据报通道（TS DOM lib 尚未收录 sendDatagram） */
interface WebTransportWithDatagram extends WebTransport {
  sendDatagram(data: BufferSource): void;
}

// ======================== SDK ========================

let wasmReady: Promise<unknown> | null = null;
function ensureWasm(input: unknown): Promise<unknown> {
  if (!wasmReady) {
    // 浏览器默认按相对路径加载；Node/测试等环境可注入 wasm 字节或模块
    wasmReady = input ? init({ module_or_path: input }) : init();
  }
  return wasmReady;
}

/** EchoStream 客户端（浏览器 SDK，自动编解码全链路） */
export class EchoStream {
  private readonly url: string;
  private readonly options: EchoStreamOptions;
  private core: ClientCoreHandle | null = null;
  private ws: WebSocket | null = null; // WebSocket 传输
  private transport: WebTransport | null = null; // WebTransport 传输
  private readonly isWs: boolean;
  private _rpcHandlers = new Map<string, (...args: unknown[]) => unknown>(); // name -> (data) => value
  private _streamHandlers = new Map<string, InboundStream<unknown>>(); // name -> receiver
  private _pendingFrame: Uint8Array | null = null;

  constructor(url: string, options: EchoStreamOptions = {}) {
    this.url = url;
    this.options = options;
    this.isWs = url.startsWith("ws:");
  }

  async connect(): Promise<void> {
    await ensureWasm(this.options.wasmModule);
    this.core = new ClientCoreHandle();
    if (this.isWs) {
      await this._connectWs();
    } else {
      await this._connectWt();
    }
  }

  async close(): Promise<void> {
    if (this.ws) { try { this.ws.close(); } catch (_) { /* 忽略 */ } }
    if (this.transport) { try { this.transport.close(); } catch (_) { /* 忽略 */ } }
  }

  // ======================== WebSocket 传输 ========================

  private async _connectWs(): Promise<void> {
    this.ws = new WebSocket(this.url);
    this.ws.binaryType = "arraybuffer";
    await new Promise<void>((resolve, reject) => {
      if (!this.ws) return reject(new Error("WebSocket 未初始化"));
      this.ws.onopen = () => resolve();
      this.ws.onerror = () => reject(new Error("WebSocket 连接失败"));
    });
    this.ws.onmessage = (e: MessageEvent) => {
      const outbound = this.core!.handle_inbound(new Uint8Array(e.data as ArrayBuffer));
      if (outbound) this._wsSend(outbound); // 服务端主动调用的响应
    };
  }

  // ======================== WebTransport 传输 ========================

  private async _connectWt(): Promise<void> {
    this.transport = new WebTransport(this.url, this.options as WebTransportOptions);
    await this.transport.ready;
    this._receiveLoop();
  }

  private _receiveLoop(): void {
    (async () => {
      try {
        const reader = this.transport!.incomingUnidirectionalStreams.getReader();
        while (true) {
          const { value: stream, done } = await reader.read();
          if (done) break;
          if (stream) this._handleUni(stream).catch(() => { /* 忽略单流错误 */ });
        }
      } catch (_) { /* 传输关闭 */ }
    })();
    (async () => {
      try {
        const reader = this.transport!.incomingBidirectionalStreams.getReader();
        while (true) {
          const { value: stream, done } = await reader.read();
          if (done) break;
          if (stream) this._handleBi(stream).catch(() => { /* 忽略单流错误 */ });
        }
      } catch (_) { /* 传输关闭 */ }
    })();
  }

  private async _handleUni(stream: ReadableStream<Uint8Array>): Promise<void> {
    const reader = stream.getReader();
    let payload: Uint8Array | null;
    while ((payload = await readFrame(reader)) !== null) {
      this.core!.handle_inbound(payload);
    }
  }

  private async _handleBi(stream: WebTransportBidirectionalStream): Promise<void> {
    const reader = stream.readable.getReader();
    const payload = await readFrame(reader);
    if (payload === null) return;
    const outbound = this.core!.handle_inbound(payload);
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
  async request<T = unknown>(name: string, ...args: unknown[]): Promise<T> {
    const { payload, options } = splitArgs(args);
    const data = encodePayload(payload, options);
    const response = new Promise<T>((resolve, reject) => {
      const frame = this.core!.request(name, data, (respData: Uint8Array, errMsg: string | null) => {
        if (errMsg !== null) {
          reject(new Error(errMsg));
        } else {
          resolve(decodeArgs(respData, options.decode) as T);
        }
      });
      this._pendingFrame = frame; // 同步设置，await 前读取（见下）
    });
    if (this.isWs) {
      this._wsSend(this._pendingFrame!);
      return response;
    }
    // wt：双向流承载请求/响应
    const frame = this._pendingFrame!;
    const stream = await this.transport!.createBidirectionalStream();
    const writer = stream.writable.getWriter();
    await writer.write(frame);
    await writer.close();
    const reader = stream.readable.getReader();
    const msg = await readFrame(reader);
    if (msg === null) throw new Error("连接已关闭");
    this.core!.handle_inbound(msg);
    reader.releaseLock();
    return response;
  }

  // ======================== Event ========================

  /** 注册事件监听（载荷自动解码；元组载荷按参数展开） */
  onEvent<TArgs extends unknown[] = unknown[]>(name: string, handler: (...args: TArgs) => void): void {
    this.core!.on_event(name, (_eventName: string, data: Uint8Array) => {
      handler(...(spreadArgs(data) as TArgs));
    });
  }

  /** 发送事件（载荷自动编码） */
  async emit<T = unknown>(name: string, payload?: T, options: RequestOptions = {}): Promise<void> {
    await this._send(this.core!.build_event(name, encodePayload(payload, options)));
  }

  /** 发送不可靠事件（WebTransport/QUIC 数据报；WebSocket 降级可靠发送） */
  emitUnreliable<T = unknown>(name: string, payload?: T, options: RequestOptions = {}): void {
    const data = this.core!.build_datagram_event(name, encodePayload(payload, options));
    if (this.isWs) {
      this._wsSend(data);
    } else if (this.transport) {
      (this.transport as WebTransportWithDatagram).sendDatagram(data as unknown as BufferSource);
    }
  }

  // ======================== 双向 RPC（处理服务端主动调用） ========================

  /**
   * 注册 RPC 处理器（响应自动编码）
   * @param handler async (...args) => value；返回 undefined 时回错误响应
   */
  onRpc<TArgs extends unknown[] = unknown[], TResp = unknown>(
    name: string,
    handler: (...args: TArgs) => TResp | Promise<TResp>,
  ): void {
    this._rpcHandlers.set(name, handler as (...args: unknown[]) => unknown);
    this.core!.on_rpc(name, (_rpcName: string, data: Uint8Array, id: bigint) => {
      const h = this._rpcHandlers.get(name);
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

  private _sendResponse(id: bigint, value: unknown): void {
    const bytes = encodePayload(value);
    this._send(this.core!.build_response(id, bytes)).catch(() => { /* 忽略 */ });
  }

  private _sendError(id: bigint, err: unknown): void {
    this._send(this._errorFrame(id, err)).catch(() => { /* 忽略 */ });
  }

  /** 构造错误响应帧（同步异常直接回错误帧） */
  private _errorFrame(id: bigint, err: unknown): Uint8Array {
    const msg = err instanceof Error ? err.message : String(err);
    return this.core!.build_error_response(id, msg);
  }

  // ======================== Stream ========================

  /** 创建出站流（帧自动编码） */
  async createStream(name: string): Promise<OutboundStream> {
    const id = this.core!.open_stream(name);
    return {
      name,
      send: async <T = unknown>(payload: T, options: RequestOptions = {}): Promise<void> => {
        await this._send(this.core!.build_stream_frame(id, name, encodePayload(payload, options), BigInt(Date.now())));
      },
      finish: async (): Promise<void> => {
        if (this.isWs) {
          await this._send(this.core!.build_stream_end(id));
        } else {
          const stream = await this.transport!.createUnidirectionalStream();
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
  onStream<T = unknown>(name: string, handler: (stream: InboundStream<T>) => void | Promise<void>): void {
    if (this._streamHandlers.has(name)) {
      throw new Error("同名流处理器已注册: " + name);
    }
    const receiver = createStreamReceiver<T>();
    this._streamHandlers.set(name, receiver);
    this.core!.on_stream(name, (frame: Uint8Array | null) => {
      if (frame === null) {
        receiver.push(null);
      } else {
        receiver.push(decodePayload(frame) as T);
      }
    });
    const ret = handler(receiver);
    if (ret instanceof Promise) {
      ret.catch((e) => console.error("[echostream] 流处理器出错:", e));
    }
  }

  // ======================== 内部 ========================

  /** 发送一帧（ws：直接发送；wt：单向流） */
  private _wsSend(frame: Uint8Array): void {
    // 类型收窄：Uint8Array<ArrayBufferLike> -> BufferSource
    this.ws!.send(frame as unknown as BufferSource);
  }

  private async _send(frame: Uint8Array): Promise<void> {
    if (this.isWs) {
      this._wsSend(frame);
      return;
    }
    const stream = await this.transport!.createUnidirectionalStream();
    const writer = stream.getWriter();
    await writer.write(frame);
    await writer.close();
  }
}

// ======================== 参数解析 ========================

/** 解析 request/emit 的多参数形式：payload | (payload, options) | 多参数 -> 元组 */
function splitArgs(args: unknown[]): { payload: unknown; options: RequestOptions } {
  if (args.length === 0) return { payload: undefined, options: {} };
  if (args.length === 1) return { payload: args[0], options: {} };
  if (args.length === 2 && isOptions(args[1])) {
    return { payload: args[0], options: args[1] };
  }
  return { payload: args, options: {} }; // 多参数 -> 元组载荷
}

/** 是否为请求选项对象（含 decode / encode 键） */
function isOptions(v: unknown): v is RequestOptions {
  return (
    typeof v === "object" && v !== null && !Array.isArray(v) &&
    ("decode" in v || "encode" in v)
  );
}

// ======================== 自动编解码 ========================

/**
 * 载荷编码（options.encode 为显式 schema，如 "list" 表示长度前缀列表）
 */
function encodePayload(payload: unknown, options: RequestOptions = {}): Uint8Array {
  const schema = options.encode;
  const value = payload === undefined ? null : payload;
  if (schema === "list") {
    return encodeList(value as unknown[]);
  }
  if (typeof schema === "object" && schema !== null && !Array.isArray(schema)) {
    return encodeStruct(value, schema as Record<string, Schema>);
  }
  return encode_payload(value);
}

/** 结构体编码（显式字段 schema，如 { items: "list" }） */
function encodeStruct(value: unknown, schema: Record<string, Schema>): Uint8Array {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("结构体编码需要对象载荷");
  }
  const parts: Uint8Array[] = [];
  const record = value as Record<string, unknown>;
  for (const key of Object.keys(schema)) {
    const fieldSchema = schema[key];
    const fieldValue = record[key];
    if (fieldSchema === "list") {
      parts.push(encodeList(fieldValue as unknown[]));
    } else if (typeof fieldSchema === "object" && fieldSchema !== null && !Array.isArray(fieldSchema)) {
      parts.push(encodeStruct(fieldValue, fieldSchema as Record<string, Schema>));
    } else {
      parts.push(encode_payload(fieldValue === undefined ? null : fieldValue));
    }
  }
  return concatBytes(parts);
}

/** 列表编码（长度前缀 + 各元素） */
function encodeList(items: unknown[]): Uint8Array {
  if (!Array.isArray(items)) throw new Error("列表编码需要数组载荷");
  const parts: Uint8Array[] = [encodeVarint(items.length)];
  for (const item of items) {
    parts.push(encode_payload(item === undefined ? null : item));
  }
  return concatBytes(parts);
}

function encodeVarint(n: number | bigint): Uint8Array {
  const out: number[] = [];
  let v = BigInt(n);
  while (v >= 0x80n) {
    out.push(Number((v & 0x7fn) | 0x80n));
    v >>= 7n;
  }
  out.push(Number(v));
  return new Uint8Array(out);
}

function concatBytes(parts: Uint8Array[]): Uint8Array {
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
function decodeArgs(bytes: Uint8Array, decode: RequestOptions["decode"]): unknown {
  if (decode === undefined || decode === "auto") {
    return decodePayload(bytes);
  }
  if (typeof decode === "function") {
    return decode(bytes);
  }
  return decodePayload(bytes, decode);
}

function decodePayload(bytes: Uint8Array, schema?: Schema): unknown {
  return decode_payload(bytes, schema === undefined ? undefined : schema);
}

/** 解码并按元组约定展开为多参数（空载荷 -> 无参数） */
function spreadArgs(bytes: Uint8Array): unknown[] {
  const decoded = decodePayload(bytes);
  if (decoded === undefined || decoded === null) return [];
  return Array.isArray(decoded) ? decoded : [decoded];
}

// ======================== 入站流接收器 ========================

/** 拉取式流接收器：帧队列 + 等待者（null 标记流结束） */
interface StreamReceiverImpl<T> extends InboundStream<T> {
  push(value: T | null): void;
}

function createStreamReceiver<T>(): StreamReceiverImpl<T> {
  let queue: (T | null)[] = [];   // 帧队列（null 标记流结束）
  let waiters: ((v: T | null) => void)[] = []; // 等待中的 recv() 调用
  let ended = false;
  return {
    async recv(): Promise<T | null> {
      if (queue.length > 0) return queue.shift() as T;
      if (ended) return null;
      return new Promise<T | null>((resolve) => waiters.push(resolve));
    },
    push(value: T | null): void {
      if (value === null) {
        ended = true;
        for (const w of waiters) w(null);
        waiters = [];
        return;
      }
      if (waiters.length > 0) {
        waiters.shift()!(value);
      } else {
        queue.push(value);
      }
    },
  };
}

// ======================== 帧读取（WebTransport 网络层） ========================

/** 读取一帧：4 字节小端长度前缀 + 载荷；流正常结束返回 null */
export async function readFrame(reader: FrameReader): Promise<Uint8Array | null> {
  const lenBytes = await readExactly(reader, 4);
  if (lenBytes === null) return null;
  const len = new DataView(lenBytes.buffer).getUint32(0, true);
  const payload = await readExactly(reader, len);
  if (payload === null) throw new Error("帧数据不完整");
  return payload;
}

async function readExactly(reader: FrameReader, len: number): Promise<Uint8Array | null> {
  const buf = new Uint8Array(len);
  let got = 0;
  while (got < len) {
    const { value, done } = await reader.read();
    if (done) return got === 0 ? null : (() => { throw new Error("帧数据不完整"); })();
    if (value) {
      buf.set(value.subarray(0, len - got), got);
      got += value.length;
    }
  }
  return buf;
}
