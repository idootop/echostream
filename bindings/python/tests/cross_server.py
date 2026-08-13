"""EchoStream 跨端矩阵：Python 服务端（供 Rust / Node 客户端连接）

线缆格式约定（与 Rust 核心一致）：
- RPC 载荷 = postcard 编码（i64 为 ZigZag varint，String 为长度前缀 + UTF-8）
- 事件载荷 = postcard 编码的 String
- 流帧载荷 = 原始 UTF-8 字节

用法：python3 cross_server.py [端口]（默认 5110）
"""
import sys

import echostream


def decode_u64(data: bytes, offset: int = 0):
    """postcard varint 解码（u64），返回 (值, 消耗字节数)"""
    result = 0
    shift = 0
    i = offset
    while True:
        b = data[i]
        i += 1
        result |= (b & 0x7F) << shift
        if not (b & 0x80):
            break
        shift += 7
    return result, i


def encode_u64(n: int) -> bytes:
    """postcard varint 编码（u64）"""
    out = bytearray()
    while n >= 0x80:
        out.append((n & 0x7F) | 0x80)
        n >>= 7
    out.append(n)
    return bytes(out)


def encode_string(s: str) -> bytes:
    """String -> postcard 编码（varint 长度 + UTF-8 字节）"""
    b = s.encode("utf-8")
    return encode_u64(len(b)) + b


def decode_string(data: bytes) -> str:
    """postcard 编码 -> String"""
    length, end = decode_u64(data)
    return data[end : end + length].decode("utf-8")


def zigzag_encode(n: int) -> int:
    """i64 ZigZag 编码（postcard 对 i64 的 varint 语义）；正值即 2n"""
    return (n << 1) ^ (n >> 63)


def zigzag_decode(z: int) -> int:
    """ZigZag 解码：z -> 原值"""
    return (z >> 1) ^ -(z & 1)


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 5110
    addr = f"127.0.0.1:{port}"
    builder = echostream.ServerBuilder()
    builder.bind(addr)

    def handle_add(data: bytes) -> bytes:
        # 请求载荷为 postcard (i64, i64) = 两个 ZigZag varint
        a_raw, end = decode_u64(data)
        b_raw, _ = decode_u64(data, end)
        a, b = zigzag_decode(a_raw), zigzag_decode(b_raw)
        print(f"E2E_RPC add({a}, {b})", flush=True)
        return encode_u64(zigzag_encode(a + b))

    def handle_hello(data: bytes) -> None:
        print(f"E2E_EVENT_RECEIVED: {decode_string(data)}", flush=True)

    builder.add_rpc("add", handle_add)
    builder.add_event("hello", handle_hello)

    def handle_chat(receiver):
        n = 0
        while True:
            frame = receiver.recv()
            if frame is None:
                break
            print(f"E2E_STREAM_FRAME {n}: {frame.decode('utf-8')}", flush=True)
            n += 1
        print(f"E2E_STREAM_FRAMES={n}", flush=True)

    builder.add_stream("chat", handle_chat)

    server = builder.build()
    print(f"E2E_SERVER_READY {addr}", flush=True)
    server.run()  # 阻塞至进程被终止


if __name__ == "__main__":
    main()
