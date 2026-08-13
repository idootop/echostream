/* tslint:disable */
/* eslint-disable */

export class ClientCoreHandle {
  free(): void;
  [Symbol.dispose](): void;
  /**
   * 构造事件帧
   */
  build_event(name: string, payload: Uint8Array): Uint8Array;
  /**
   * 打开流：分配流 id
   */
  open_stream(name: string): bigint;
  /**
   * 构造响应帧（服务端主动调用的异步回复）
   */
  build_response(id: bigint, payload: Uint8Array): Uint8Array;
  /**
   * 处理入站帧：返回需要写回对端的响应帧（对端主动调用且同步完成时）
   */
  handle_inbound(frame: Uint8Array): Uint8Array | undefined;
  /**
   * 构造流数据帧（自动递增序号；senderTs 为毫秒时间戳）
   */
  build_stream_frame(id: bigint, name: string, payload: Uint8Array, sender_ts: bigint): Uint8Array;
  /**
   * 创建状态机
   */
  constructor();
  /**
   * 注册 RPC 处理器（处理对端主动调用；回调返回响应字节或 null 表示异步处理）
   */
  on_rpc(name: string, callback: Function): void;
  /**
   * 发起 RPC：返回请求帧（长度前缀 + Message），响应到达时调用 `resolve(data: Uint8Array)`
   */
  request(name: string, payload: Uint8Array, resolve: Function): Uint8Array;
  /**
   * 注册事件监听（回调：`(name: string, data: Uint8Array) => void`）
   */
  on_event(name: string, callback: Function): void;
}

/**
 * 解码 bytes（长度前缀 + 字节）
 */
export function decode_bytes(bytes: Uint8Array): Uint8Array;

/**
 * 解码消息：postcard 字节 → JS 对象
 */
export function decode_message(bytes: Uint8Array): any;

/**
 * 解码 string
 */
export function decode_string(bytes: Uint8Array): string;

/**
 * 解码 u64（varint）
 */
export function decode_u64(bytes: Uint8Array): number;

/**
 * 编码帧：4 字节小端长度前缀 + 消息载荷
 */
export function encode_frame(msg: any): Uint8Array;

/**
 * 编码消息：JS 对象 → postcard 字节
 *
 * 输入：`{ type, id, name, data, ... }`
 * - request/event：`{ type, id, name, data: Uint8Array }`
 * - response：`{ type, id, code, message?, data }`
 * - stream：`{ type, id, name, seq, senderTs, data }`
 */
export function encode_message(msg: any): Uint8Array;

/**
 * 编码载荷：JS 值 → postcard 字节
 *
 * 支持：number（非负整数 → u64 varint，负数 → i64 zigzag）、bigint、
 * string（长度前缀 + UTF-8）、Uint8Array（长度前缀 + 字节）、
 * Array（按 Rust 元组/结构体字段顺序编码，无长度前缀）、
 * Object（字段按插入序编码，等价结构体字段序）。
 */
export function encode_payload(value: any): Uint8Array;
