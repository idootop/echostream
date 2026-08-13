/* tslint:disable */
/* eslint-disable */

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
