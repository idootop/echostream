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
   * 构造流结束标记（WebSocket 传输的流关闭）
   */
  build_stream_end(id: bigint): Uint8Array;
  /**
   * 构造流数据帧（自动递增序号；senderTs 为毫秒时间戳）
   */
  build_stream_frame(id: bigint, name: string, payload: Uint8Array, sender_ts: bigint): Uint8Array;
  /**
   * 构造数据报事件载荷（不可靠通道；WebTransport.sendDatagram / QUIC datagram）
   */
  build_datagram_event(name: string, payload: Uint8Array): Uint8Array;
  /**
   * 构造错误响应帧（处理对端主动调用失败时回复）
   */
  build_error_response(id: bigint, message: string): Uint8Array;
  /**
   * 创建状态机
   */
  constructor();
  /**
   * 注册 RPC 处理器（处理对端主动调用）
   * 回调签名：(name: string, data: Uint8Array, id: number) => Uint8Array | null
   * 返回 null 表示异步处理（稍后通过 build_response(id, payload) 补响应）
   */
  on_rpc(name: string, callback: Function): void;
  /**
   * 发起 RPC：返回请求帧（长度前缀 + Message）
   * 响应到达时调用 resolve(data: Uint8Array, error: string | null)
   */
  request(name: string, payload: Uint8Array, resolve: Function): Uint8Array;
  /**
   * 注册事件监听（回调：name 与 data 两个参数）
   */
  on_event(name: string, callback: Function): void;
  /**
   * 注册入站流处理器（处理对端推送的流；回调：frame: Uint8Array | null）
   */
  on_stream(name: string, callback: Function): void;
}

/**
 * 解码 bytes（长度前缀 + 字节）
 */
export function decode_bytes(bytes: Uint8Array): Uint8Array;

/**
 * 解码 i64（ZigZag varint）
 */
export function decode_i64(bytes: Uint8Array): number;

/**
 * 解码消息：postcard 字节 -> JS 对象
 */
export function decode_message(bytes: Uint8Array): any;

/**
 * 解码载荷：postcard 字节 -> JS 值（智能推断）
 *
 * schema 可选（字符串 / 数组 / 对象），用于歧义场景精确解码：
 * - "auto" | "number" | "bigint" | "u64" | "bool" | "string" | "bytes" | "f64" | "f32" | "list"
 * - 数组 = 元组逐字段；对象 = 结构体具名字段
 */
export function decode_payload(bytes: Uint8Array, schema: any): any;

/**
 * 解码 string
 */
export function decode_string(bytes: Uint8Array): string;

/**
 * 解码 u64（普通 varint）
 */
export function decode_u64(bytes: Uint8Array): number;

/**
 * 编码帧：4 字节小端长度前缀 + 消息载荷
 */
export function encode_frame(msg: any): Uint8Array;

/**
 * 编码 i64（ZigZag varint，与 postcard 有符号整数一致）
 */
export function encode_i64(n: bigint): Uint8Array;

/**
 * 编码消息：JS 对象 -> postcard 字节
 *
 * 输入：{ type, id, name, data, ... }
 * - request/event：{ type, id, name, data: Uint8Array }
 * - response：{ type, id, code, message?, data }
 * - stream：{ type, id, name, seq, senderTs, data }
 */
export function encode_message(msg: any): Uint8Array;

/**
 * 编码载荷：JS 值 -> postcard 字节（约定见模块文档）
 */
export function encode_payload(value: any): Uint8Array;
