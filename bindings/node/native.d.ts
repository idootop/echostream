// 原生 napi 模块类型声明（echostream-node.node）
//
// 回调遵循 Node error-first 约定：成功时 err 为 null。
// 载荷均为原始 postcard 字节（Buffer / Uint8Array）。

/** 底层客户端（手动字节 API） */
export interface NativeClient {
  request(name: string, payload: Uint8Array): Promise<Uint8Array>;
  emit(name: string, payload: Uint8Array): Promise<void>;
  emitUnreliable(name: string, payload: Uint8Array): Promise<void>;
  createStream(name: string): Promise<NativeStream>;
  /** 创建流并携带元数据（Record<string, string | number | boolean>） */
  createStreamWithMetadata(name: string, metadata: Record<string, string | number | boolean>): Promise<NativeStream>;
  onEvent(name: string, callback: (err: Error | null, payload: Uint8Array) => void): number;
  offEvent(token: number): boolean;
  onRpc(
    name: string,
    callback: (err: Error | null, payload: Uint8Array) => Promise<Uint8Array> | Uint8Array,
  ): number;
  offRpc(token: number): boolean;
  onStream(name: string, callback: (err: Error | null, receiver: NativeStreamReceiver) => void): number;
  offStream(token: number): boolean;
  close(): void;
}

/** 底层出站流 */
export interface NativeStream {
  send(payload: Uint8Array): Promise<void>;
  finish(): Promise<void>;
}

/** 底层入站流接收器（recv 返回 null 表示流结束） */
export interface NativeStreamReceiver {
  recv(): Promise<Uint8Array | null>;
  /** 流元数据（StreamOpen 协商） */
  metadata(): Promise<Record<string, string>>;
  /** 结束码（0 正常 / 非 0 异常；流结束后有效） */
  endCode(): Promise<number>;
  /** 结束原因（流结束后有效） */
  endMessage(): Promise<string | null>;
}

/** 底层服务端构建器 */
export interface NativeServerBuilder {
  bind(addr: string): void;
  addRpc(
    name: string,
    callback: (err: Error | null, payload: Uint8Array) => Promise<Uint8Array> | Uint8Array,
  ): void;
  addEvent(name: string, callback: (err: Error | null, payload: Uint8Array) => void): void;
  addStream(name: string, callback: (err: Error | null, receiver: NativeStreamReceiver) => void): void;
  build(): Promise<NativeServer>;
}

/** 底层服务端 */
export interface NativeServer {
  run(): Promise<void>;
  shutdown(): void;
  addr(): string | null;
  broadcast(name: string, payload: Uint8Array): Promise<void>;
  sessions(): NativeSession[];
}

/** 底层会话（服务端视角的连接） */
export interface NativeSession {
  id(): number;
  peerAddr(): string;
  request(name: string, payload: Uint8Array): Promise<Uint8Array>;
  emit(name: string, payload: Uint8Array): Promise<void>;
  close(): void;
}

/** 原生模块导出 */
export interface NativeModule {
  connect(url: string): Promise<NativeClient>;
  JsServerBuilder: new () => NativeServerBuilder;
}
