# echostream-proto

> 底层协议定义、Wire Format 与基础类型（零运行时依赖）

## 模块职责

整个框架最底层的模块，定义通信协议的核心类型与数据结构：

- 协议类型：Message（Request / Response / Event / Stream / StreamEnd）、StatusCode、Timestamp
- 传输接口：Endpoint / FrameIo / FrameRead / FrameWrite / Listener（各传输实现）
- 帧编解码：长度前缀 + postcard（encode_message / read_message_frame）
- 动态值编解码（dynamic）：跨语言载荷的自动序列化约定（i64 ZigZag 等，四端一致）

## 设计原则

- 零运行时依赖：不引入 tokio / quinn 等异步运行时与网络实现
- 稳定 API：所有上层模块依赖此处类型，变更需谨慎并保持向后兼容
- 一个源：跨端线缆格式（含 dynamic 编码约定）以本 crate 为准

## 核心类型

### Message（消息帧）

```rust
use echostream_proto::{Message, RequestMsg};

let frame = Message::Request(RequestMsg {
    id: 1,
    name: "add".into(),
    data: bytes::Bytes::new(),
});
// 还有 Response / Event / Stream / StreamEnd 变体
```

### 传输接口

- Endpoint：连接抽象（open_bi / open_uni / accept_bi / accept_uni / datagram）
- FrameIo：帧级流读写（write_message / read_message / finish / split）
- Listener：监听器抽象（accept / local_addr / close）

各传输实现（echostream-transport 的 quic / ws / web）遵循同一帧协议。

### 动态值编解码（dynamic）

跨语言（Rust / Node / Python / Web）载荷自动序列化的单一事实来源：

| 值 | 线缆格式 | Rust 对应 |
|----|----------|-----------|
| 整数（含负数） | i64 ZigZag varint | i64 |
| 非负 BigInt | u64 普通 varint | u64 |
| 浮点数 | f64 小端 | f64 |
| 布尔 | 单字节 | bool |
| 字符串 | 长度前缀 + UTF-8 | String |
| 字节数组 | 长度前缀 | Vec<u8> |
| 数组 / 对象 | 字段序（无长度前缀） | 元组 / 结构体 |

```rust
use echostream_proto::{Dynamic, Schema, encode, decode, decode_with};

let bytes = encode(&Dynamic::Seq(vec![Dynamic::Int(10), Dynamic::Int(20)]))?;
assert_eq!(decode(&bytes)?, Dynamic::Seq(vec![Dynamic::Int(10), Dynamic::Int(20)]));
```

解码默认智能推断（Schema::Auto），歧义场景显式传 Schema。
