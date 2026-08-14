// EchoStream Node.js binding（ESM）—— 自动编解码 DX
//
// 用法：
//   import { connect, ServerBuilder } from "echostream-node";
//
//   // 客户端
//   const client = await connect("127.0.0.1:5000");
//   const sum = await client.request("add", 10, 20);   // 30，自动编解码
//   client.onEvent("hello", (data) => console.log(data));
//   client.onRpc("ping", async () => "pong");
//
//   // 服务端
//   const builder = new ServerBuilder();
//   builder.bind("0.0.0.0:5000");
//   builder.addRpc("add", async (a, b) => a + b);      // 自动解码参数、编码响应
//   builder.addEvent("hello", (data) => console.log(data));
//   builder.addStream("chat", async (receiver) => {
//     let frame;
//     while ((frame = await receiver.recv()) !== null) console.log(frame);
//   });
//   const server = await builder.build();
//   await server.run();
//
// 底层 API（手动字节）仍可通过 native 导出使用：request_raw 等。

import { createRequire } from "node:module";
import { encodePayload, decodePayload } from "./postcard.js";

const require = createRequire(import.meta.url);
const native = require("./echostream-node.node");

// ======================== 载荷参数解析 ========================

function splitArgs(args) {
  if (args.length === 0) return { payload: undefined, options: {} };
  if (args.length === 1) return { payload: args[0], options: {} };
  if (args.length === 2 && isOptions(args[1])) return { payload: args[0], options: args[1] };
  return { payload: args, options: {} }; // 多参数 -> 元组载荷
}

function isOptions(v) {
  return (
    typeof v === "object" && v !== null && !Array.isArray(v) &&
    (v.decode !== undefined || v.encode !== undefined)
  );
}

function encodeArgs(payload, options) {
  return encodePayload(payload, options && options.encode);
}

function decodeArgs(bytes, options) {
  const decode = options && options.decode;
  if (typeof decode === "function") return decode(bytes);
  return decodePayload(bytes, decode);
}

/** 载荷解码并按元组约定展开为多参数（空载荷 -> 无参数） */
function spreadArgs(payload) {
  const decoded = decodePayload(payload);
  if (decoded === undefined || decoded === null) return [];
  return Array.isArray(decoded) ? decoded : [decoded];
}

// ======================== 客户端 ========================

export class Client {
  constructor(nativeClient) {
    this._n = nativeClient;
  }

  /** 发起 RPC（参数自动编码、响应自动解码） */
  async request(name, ...args) {
    const { payload, options } = splitArgs(args);
    const resp = await this._n.request(name, encodeArgs(payload, options));
    return decodeArgs(resp, options);
  }

  /** 发送事件（载荷自动编码） */
  async emit(name, payload, options) {
    await this._n.emit(name, encodeArgs(payload, options));
  }

  /** 发送不可靠事件（数据报通道；连接不支持时返回错误） */
  async emitUnreliable(name, payload, options) {
    await this._n.emitUnreliable(name, encodeArgs(payload, options));
  }

  /** 创建流（帧自动编码） */
  async createStream(name) {
    return new Stream(await this._n.createStream(name));
  }

  /** 注册事件监听（载荷自动解码；同名事件支持多个监听器） */
  onEvent(name, handler) {
    this._n.onEvent(name, (err, payload) => {
      if (err) throw err;
      handler(...spreadArgs(payload));
    });
  }

  /** 注册 RPC 处理器（处理服务端主动调用；返回 Promise 亦可） */
  onRpc(name, handler) {
    this._n.onRpc(name, async (err, payload) => {
      if (err) throw err;
      const result = await handler(...spreadArgs(payload));
      if (result === undefined) throw new Error("RPC 处理器未返回响应");
      return encodePayload(result);
    });
  }

  /** 注册流处理器（服务端推送；帧自动解码） */
  onStream(name, handler) {
    this._n.onStream(name, (err, receiver) => {
      if (err) throw err;
      handler(new StreamReceiver(receiver));
    });
  }

  /** 主动关闭连接 */
  close() {
    this._n.close();
  }
}

export class Stream {
  constructor(nativeStream) {
    this._n = nativeStream;
  }

  /** 发送一帧（自动编码） */
  async send(payload, options) {
    await this._n.send(encodeArgs(payload, options));
  }

  /** 关闭流 */
  async finish() {
    await this._n.finish();
  }
}

export class StreamReceiver {
  constructor(nativeReceiver) {
    this._n = nativeReceiver;
  }

  /** 读取下一帧（自动解码）；流结束返回 null */
  async recv() {
    const frame = await this._n.recv();
    return frame === null ? null : decodePayload(frame);
  }
}

// ======================== 服务端 ========================

export class ServerBuilder {
  constructor() {
    this._n = new native.JsServerBuilder();
  }

  /** 绑定监听地址 */
  bind(addr) {
    this._n.bind(addr);
  }

  /** 注册 RPC 处理器（回调参数自动解码展开；返回值自动编码） */
  addRpc(name, handler) {
    this._n.addRpc(name, async (err, payload) => {
      if (err) throw err;
      const result = await handler(...spreadArgs(payload));
      if (result === undefined) throw new Error("RPC 处理器未返回响应");
      return encodePayload(result);
    });
  }

  /** 注册事件处理器（载荷自动解码） */
  addEvent(name, handler) {
    this._n.addEvent(name, (err, payload) => {
      if (err) throw err;
      handler(...spreadArgs(payload));
    });
  }

  /** 注册流处理器（receiver.recv() 自动解码） */
  addStream(name, handler) {
    this._n.addStream(name, (err, receiver) => {
      if (err) throw err;
      handler(new StreamReceiver(receiver));
    });
  }

  /** 构建服务端 */
  async build() {
    return new Server(await this._n.build());
  }
}

export class Server {
  constructor(nativeServer) {
    this._n = nativeServer;
    this._ctx = null;
  }

  /** 运行服务（阻塞直到 shutdown；请勿 await 于事件循环热路径） */
  async run() {
    await this._n.run();
  }

  /** 优雅关闭 */
  shutdown() {
    this._n.shutdown();
  }

  /** 本地监听地址 */
  addr() {
    return this._n.addr();
  }

  /** 广播事件到所有连接客户端 */
  async broadcast(name, payload, options) {
    await this._n.broadcast(name, encodeArgs(payload, options));
  }

  /** 所有在线会话（可主动调用客户端） */
  sessions() {
    return this._n.sessions().map((s) => new Session(s));
  }
}

export class Session {
  constructor(nativeSession) {
    this._n = nativeSession;
  }

  id() {
    return this._n.id();
  }

  peerAddr() {
    return this._n.peerAddr();
  }

  /** 主动调用客户端 RPC */
  async request(name, ...args) {
    const { payload, options } = splitArgs(args);
    const resp = await this._n.request(name, encodeArgs(payload, options));
    return decodeArgs(resp, options);
  }

  /** 向客户端发送事件 */
  async emit(name, payload, options) {
    await this._n.emit(name, encodeArgs(payload, options));
  }

  /** 关闭连接 */
  close() {
    this._n.close();
  }
}

// ======================== 入口 ========================

/** 连接服务端（QUIC） */
export async function connect(url) {
  return new Client(await native.connect(url));
}

/** 底层原生 API（手动字节编解码的高级用法） */
export const nativeApi = native;

export { encodePayload, decodePayload } from "./postcard.js";
