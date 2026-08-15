// EchoStream Node.js binding（TypeScript，ESM）—— 自动编解码 DX
//
// 用法：
//   import { connect, ServerBuilder } from "echostream-node";
//
//   // 客户端（泛型：响应类型 / 事件参数 / RPC 参数与响应均可标注）
//   const client = await connect("127.0.0.1:5000");
//   const sum: number = await client.request<number>("add", 10, 20); // 30，自动编解码
//   client.onEvent<[string]>("hello", ([msg]) => console.log(msg));
//   client.onRpc<[], string>("ping", async () => "pong");
//
//   // 服务端
//   const builder = new ServerBuilder();
//   builder.bind("0.0.0.0:5000");
//   builder.addRpc<[number, number], number>("add", async (a, b) => a + b);
//   builder.addEvent<[string]>("hello", ([msg]) => console.log(msg));
//   builder.addStream<[string]>("chat", async (receiver) => {
//     let frame: string | null;
//     while ((frame = await receiver.recv<string>()) !== null) console.log(frame);
//   });
//   const server = await builder.build();
//   await server.run();
//
// 底层 API（手动字节）仍可通过 nativeApi 使用：request_raw 等。

import { createRequire } from "node:module";
import type { NativeModule } from "./native.js";
import { Schema, encodePayload, decodePayload } from "./postcard.js";

const require = createRequire(import.meta.url);
// 原生二进制位于包根（构建产物在 dist/，向上一级）
const native = require("../echostream-node.node") as NativeModule;

// ======================== 载荷参数解析 ========================

/** 请求选项：decode/encode 为显式 schema 或解码函数 */
export interface RequestOptions {
  /** 响应解码：schema（字符串/数组/对象）或自定义解码函数 */
  decode?: Schema | ((bytes: Uint8Array) => unknown);
  /** 载荷编码：显式 schema（如 "list" 表示长度前缀列表） */
  encode?: Schema | "list";
}

function splitArgs(args: unknown[]): { payload: unknown; options: RequestOptions } {
  if (args.length === 0) return { payload: undefined, options: {} };
  if (args.length === 1) return { payload: args[0], options: {} };
  if (args.length === 2 && isOptions(args[1])) return { payload: args[0], options: args[1] };
  return { payload: args, options: {} }; // 多参数 -> 元组载荷
}

function isOptions(v: unknown): v is RequestOptions {
  return (
    typeof v === "object" && v !== null && !Array.isArray(v) &&
    ("decode" in v || "encode" in v)
  );
}

function encodeArgs(payload: unknown, options: RequestOptions): Uint8Array {
  return encodePayload(payload, options.encode);
}

function decodeArgs(bytes: Uint8Array, options: RequestOptions): unknown {
  const decode = options.decode;
  if (typeof decode === "function") return decode(bytes);
  return decodePayload(bytes, decode);
}

/** 载荷解码并按元组约定展开为多参数（空载荷 -> 无参数） */
function spreadArgs(payload: Uint8Array): unknown[] {
  const decoded = decodePayload(payload);
  if (decoded === undefined || decoded === null) return [];
  return Array.isArray(decoded) ? decoded : [decoded];
}

// ======================== 客户端 ========================

/** 客户端：RPC / Event / Stream（自动编解码 DX） */
export class Client {
  private readonly _n: import("./native.js").NativeClient;

  constructor(nativeClient: import("./native.js").NativeClient) {
    this._n = nativeClient;
  }

  /** 发起 RPC（参数自动编码、响应自动解码；T 为响应类型） */
  async request<T = unknown>(name: string, ...args: unknown[]): Promise<T> {
    const { payload, options } = splitArgs(args);
    const resp = await this._n.request(name, encodeArgs(payload, options));
    return decodeArgs(resp, options) as T;
  }

  /** 发送事件（载荷自动编码） */
  async emit<T = unknown>(name: string, payload?: T, options?: RequestOptions): Promise<void> {
    await this._n.emit(name, encodeArgs(payload, options ?? {}));
  }

  /** 发送不可靠事件（数据报通道；连接不支持时返回错误） */
  async emitUnreliable<T = unknown>(name: string, payload?: T, options?: RequestOptions): Promise<void> {
    await this._n.emitUnreliable(name, encodeArgs(payload, options ?? {}));
  }

  /** 创建流（帧自动编码） */
  async createStream(name: string): Promise<Stream> {
    return new Stream(await this._n.createStream(name));
  }

  /** 注册事件监听（载荷自动解码并展开；TArgs 为解码后的参数元组） */
  onEvent<TArgs extends unknown[] = unknown[]>(name: string, handler: (...args: TArgs) => void): void {
    this._n.onEvent(name, (err, payload) => {
      if (err) throw err;
      handler(...(spreadArgs(payload) as TArgs));
    });
  }

  /** 注册 RPC 处理器（处理服务端主动调用；TResp 为响应类型） */
  onRpc<TArgs extends unknown[] = unknown[], TResp = unknown>(
    name: string,
    handler: (...args: TArgs) => TResp | Promise<TResp>,
  ): void {
    this._n.onRpc(name, async (err, payload) => {
      if (err) throw err;
      const result = await handler(...(spreadArgs(payload) as TArgs));
      if (result === undefined) throw new Error("RPC 处理器未返回响应");
      return encodePayload(result);
    });
  }

  /** 注册流处理器（服务端推送；帧自动解码） */
  onStream(name: string, handler: (receiver: StreamReceiver) => void): void {
    this._n.onStream(name, (err, receiver) => {
      if (err) throw err;
      handler(new StreamReceiver(receiver));
    });
  }

  /** 主动关闭连接 */
  close(): void {
    this._n.close();
  }
}

/** 出站流：发送帧（自动编码） */
export class Stream {
  private readonly _n: import("./native.js").NativeStream;

  constructor(nativeStream: import("./native.js").NativeStream) {
    this._n = nativeStream;
  }

  /** 发送一帧（自动编码） */
  async send<T = unknown>(payload: T, options?: RequestOptions): Promise<void> {
    await this._n.send(encodeArgs(payload, options ?? {}));
  }

  /** 关闭流 */
  async finish(): Promise<void> {
    await this._n.finish();
  }
}

/** 入站流接收器：逐帧拉取（自动解码；流结束返回 null） */
export class StreamReceiver {
  private readonly _n: import("./native.js").NativeStreamReceiver;

  constructor(nativeReceiver: import("./native.js").NativeStreamReceiver) {
    this._n = nativeReceiver;
  }

  /** 读取下一帧（自动解码）；流结束返回 null */
  async recv<T = unknown>(): Promise<T | null> {
    const frame = await this._n.recv();
    return frame === null ? null : (decodePayload(frame) as T);
  }
}

// ======================== 服务端 ========================

/** 服务端构建器 */
export class ServerBuilder {
  private readonly _n: import("./native.js").NativeServerBuilder;

  constructor() {
    this._n = new native.JsServerBuilder();
  }

  /** 绑定监听地址 */
  bind(addr: string): void {
    this._n.bind(addr);
  }

  /** 注册 RPC 处理器（参数自动解码展开、返回值自动编码） */
  addRpc<TArgs extends unknown[] = unknown[], TResp = unknown>(
    name: string,
    handler: (...args: TArgs) => TResp | Promise<TResp>,
  ): void {
    this._n.addRpc(name, async (err, payload) => {
      if (err) throw err;
      const result = await handler(...(spreadArgs(payload) as TArgs));
      if (result === undefined) throw new Error("RPC 处理器未返回响应");
      return encodePayload(result);
    });
  }

  /** 注册事件处理器（载荷自动解码并展开） */
  addEvent<TArgs extends unknown[] = unknown[]>(name: string, handler: (...args: TArgs) => void): void {
    this._n.addEvent(name, (err, payload) => {
      if (err) throw err;
      handler(...(spreadArgs(payload) as TArgs));
    });
  }

  /** 注册流处理器（receiver.recv() 自动解码） */
  addStream(name: string, handler: (receiver: StreamReceiver) => void): void {
    this._n.addStream(name, (err, receiver) => {
      if (err) throw err;
      handler(new StreamReceiver(receiver));
    });
  }

  /** 构建服务端 */
  async build(): Promise<Server> {
    return new Server(await this._n.build());
  }
}

/** 服务端 */
export class Server {
  private readonly _n: import("./native.js").NativeServer;

  constructor(nativeServer: import("./native.js").NativeServer) {
    this._n = nativeServer;
  }

  /** 运行服务（阻塞直到 shutdown；请勿 await 于事件循环热路径） */
  async run(): Promise<void> {
    await this._n.run();
  }

  /** 优雅关闭 */
  shutdown(): void {
    this._n.shutdown();
  }

  /** 本地监听地址 */
  addr(): string {
    return this._n.addr() ?? ""; // build 后必已绑定
  }

  /** 广播事件到所有连接客户端 */
  async broadcast<T = unknown>(name: string, payload?: T, options?: RequestOptions): Promise<void> {
    await this._n.broadcast(name, encodeArgs(payload, options ?? {}));
  }

  /** 所有在线会话（可主动调用客户端） */
  sessions(): Session[] {
    return this._n.sessions().map((s) => new Session(s));
  }
}

/** 会话：服务端视角的单个客户端连接（可主动双向调用） */
export class Session {
  private readonly _n: import("./native.js").NativeSession;

  constructor(nativeSession: import("./native.js").NativeSession) {
    this._n = nativeSession;
  }

  id(): number {
    return this._n.id();
  }

  peerAddr(): string {
    return this._n.peerAddr();
  }

  /** 主动调用客户端 RPC */
  async request<T = unknown>(name: string, ...args: unknown[]): Promise<T> {
    const { payload, options } = splitArgs(args);
    const resp = await this._n.request(name, encodeArgs(payload, options));
    return decodeArgs(resp, options) as T;
  }

  /** 向客户端发送事件 */
  async emit<T = unknown>(name: string, payload?: T, options?: RequestOptions): Promise<void> {
    await this._n.emit(name, encodeArgs(payload, options ?? {}));
  }

  /** 关闭连接 */
  close(): void {
    this._n.close();
  }
}

// ======================== 入口 ========================

/** 连接服务端（QUIC） */
export async function connect(url: string): Promise<Client> {
  return new Client(await native.connect(url));
}

/** 底层原生 API（手动字节编解码的高级用法） */
export const nativeApi = native;

export { encodePayload, decodePayload } from "./postcard.js";
export type { Schema } from "./postcard.js";
