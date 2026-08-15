/* tslint:disable */
/* eslint-disable */

export class ClientCoreHandle {
  free(): void;
  [Symbol.dispose](): void;
  /**
   * 取消注册流处理器（按 on_stream 返回的 id）
   */
  off_stream(id: number): boolean;
  /**
   * 查询流的结束信息（StreamEnd 记录；返回 { code, message, metadata } 或 null）
   */
  stream_end(id: bigint): any | undefined;
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
   * 查询流的元数据（StreamOpen 记录；返回 Record<string, string | Uint8Array>）
   */
  stream_metadata(id: bigint): any | undefined;
  /**
   * 构造流结束帧（WebSocket 传输的流关闭；code=0 正常，非 0 异常/取消）
   */
  build_stream_end(id: bigint, code: number, message?: string | null): Uint8Array;
  /**
   * 构造流开始帧（流协商：名称 + 元数据；流首帧必须为此帧）
   * metadata：Record<string, string | number | Uint8Array>
   */
  build_stream_open(id: bigint, name: string, metadata: any): Uint8Array;
  /**
   * 构造流数据帧（自动递增序号；senderTs 为毫秒墙钟）
   */
  build_stream_frame(id: bigint, payload: Uint8Array, sender_ts: bigint): Uint8Array;
  /**
   * 清理已结束流的内部状态（避免元数据累积）
   */
  remove_stream_state(id: bigint): void;
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
   * 注册 RPC 处理器（处理对端主动调用），返回监听 id（off_rpc 取消注册）
   * 回调签名：(name: string, data: Uint8Array, id: number) => Uint8Array | null
   * 返回 null 表示异步处理（稍后通过 build_response(id, payload) 补响应）
   */
  on_rpc(name: string, callback: Function): number;
  /**
   * 取消注册 RPC 处理器（按 on_rpc 返回的 id）
   */
  off_rpc(id: number): boolean;
  /**
   * 发起 RPC：返回请求帧（长度前缀 + Message）
   * 响应到达时调用 resolve(data: Uint8Array, error: string | null)
   */
  request(name: string, payload: Uint8Array, resolve: Function): Uint8Array;
  /**
   * 注册事件监听（回调：name 与 data 两个参数），返回监听 id（off_event 取消注册）
   */
  on_event(name: string, callback: Function): number;
  /**
   * 取消注册事件监听（按 on_event 返回的 id）
   */
  off_event(id: number): boolean;
  /**
   * 注册入站流处理器（处理对端推送的流；回调：frame 对象或 null），
   * 帧对象含 { id, seq, senderTs, data: Uint8Array }，返回监听 id（off_stream 取消注册）
   */
  on_stream(name: string, callback: Function): number;
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

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_clientcorehandle_free: (a: number, b: number) => void;
  readonly clientcorehandle_build_datagram_event: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
  readonly clientcorehandle_build_error_response: (a: number, b: number, c: bigint, d: number, e: number) => void;
  readonly clientcorehandle_build_event: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
  readonly clientcorehandle_build_response: (a: number, b: number, c: bigint, d: number, e: number) => void;
  readonly clientcorehandle_build_stream_end: (a: number, b: number, c: bigint, d: number, e: number, f: number) => void;
  readonly clientcorehandle_build_stream_frame: (a: number, b: number, c: bigint, d: number, e: number, f: bigint) => void;
  readonly clientcorehandle_build_stream_open: (a: number, b: number, c: bigint, d: number, e: number, f: number) => void;
  readonly clientcorehandle_handle_inbound: (a: number, b: number, c: number, d: number) => void;
  readonly clientcorehandle_new: () => number;
  readonly clientcorehandle_off_event: (a: number, b: number) => number;
  readonly clientcorehandle_off_rpc: (a: number, b: number) => number;
  readonly clientcorehandle_off_stream: (a: number, b: number) => number;
  readonly clientcorehandle_on_event: (a: number, b: number, c: number, d: number) => number;
  readonly clientcorehandle_on_rpc: (a: number, b: number, c: number, d: number) => number;
  readonly clientcorehandle_on_stream: (a: number, b: number, c: number, d: number) => number;
  readonly clientcorehandle_open_stream: (a: number, b: number, c: number) => bigint;
  readonly clientcorehandle_remove_stream_state: (a: number, b: bigint) => void;
  readonly clientcorehandle_request: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
  readonly clientcorehandle_stream_end: (a: number, b: bigint) => number;
  readonly clientcorehandle_stream_metadata: (a: number, b: bigint) => number;
  readonly decode_bytes: (a: number, b: number, c: number) => void;
  readonly decode_i64: (a: number, b: number, c: number) => void;
  readonly decode_message: (a: number, b: number, c: number) => void;
  readonly decode_payload: (a: number, b: number, c: number, d: number) => void;
  readonly decode_string: (a: number, b: number, c: number) => void;
  readonly decode_u64: (a: number, b: number, c: number) => void;
  readonly encode_frame: (a: number, b: number) => void;
  readonly encode_i64: (a: number, b: bigint) => void;
  readonly encode_message: (a: number, b: number) => void;
  readonly encode_payload: (a: number, b: number) => void;
  readonly __wbindgen_export: (a: number, b: number) => number;
  readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_export3: (a: number) => void;
  readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
  readonly __wbindgen_export4: (a: number, b: number, c: number) => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
