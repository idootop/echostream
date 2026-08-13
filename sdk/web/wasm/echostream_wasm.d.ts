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

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_clientcorehandle_free: (a: number, b: number) => void;
  readonly clientcorehandle_build_event: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
  readonly clientcorehandle_build_response: (a: number, b: number, c: bigint, d: number, e: number) => void;
  readonly clientcorehandle_build_stream_end: (a: number, b: number, c: bigint) => void;
  readonly clientcorehandle_build_stream_frame: (a: number, b: number, c: bigint, d: number, e: number, f: number, g: number, h: bigint) => void;
  readonly clientcorehandle_handle_inbound: (a: number, b: number, c: number, d: number) => void;
  readonly clientcorehandle_new: () => number;
  readonly clientcorehandle_on_event: (a: number, b: number, c: number, d: number) => void;
  readonly clientcorehandle_on_rpc: (a: number, b: number, c: number, d: number) => void;
  readonly clientcorehandle_open_stream: (a: number, b: number, c: number) => bigint;
  readonly clientcorehandle_request: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
  readonly decode_bytes: (a: number, b: number, c: number) => void;
  readonly decode_message: (a: number, b: number, c: number) => void;
  readonly decode_string: (a: number, b: number, c: number) => void;
  readonly decode_u64: (a: number, b: number, c: number) => void;
  readonly encode_frame: (a: number, b: number) => void;
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
