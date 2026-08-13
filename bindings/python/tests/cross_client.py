"""EchoStream 跨端矩阵：Python 客户端（连接 Rust / Node 服务端）

线缆格式约定（与 Rust 核心一致）：
- RPC 载荷 = postcard 编码（i64 为 ZigZag varint，String 为长度前缀 + UTF-8）
- 事件载荷 = postcard 编码的 String
- 流帧载荷 = 原始 UTF-8 字节

用法：python3 cross_client.py <ip:port>（默认 127.0.0.1:5110）
"""
import sys
import time

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


def encode_tuple(a: int, b: int) -> bytes:
    """(u64, u64) 载荷：顺序 varint 编码"""
    return encode_u64(a) + encode_u64(b)


def zigzag_encode(n: int) -> int:
    """i64 ZigZag 编码（postcard 对 i64 的 varint 语义）；正值即 2n"""
    return (n << 1) ^ (n >> 63)


def zigzag_decode(z: int) -> int:
    """ZigZag 解码：z -> 原值"""
    return (z >> 1) ^ -(z & 1)


def encode_tuple_i64(a: int, b: int) -> bytes:
    """(i64, i64) 载荷：add RPC 请求，postcard i64 为 ZigZag varint"""
    return encode_u64(zigzag_encode(a)) + encode_u64(zigzag_encode(b))


def main():
    addr = sys.argv[1] if len(sys.argv) > 1 else "127.0.0.1:5110"
    client = echostream.connect(addr)
    print("[py-client] 已连接", flush=True)

    # RPC：add(10, 20) = 30（载荷为 postcard (i64, i64) = 两个 ZigZag varint）
    resp = client.request("add", encode_tuple_i64(10, 20))
    total = zigzag_decode(decode_u64(resp)[0])
    print(f"add(10, 20) = {total}", flush=True)
    assert total == 30, f"add 期望 30，实际 {total}"

    # 事件：hello（postcard 编码的 String）
    client.emit("hello", encode_string("hello from python client"))

    # 流：chat 3 帧（原始 UTF-8 字节）
    stream = client.create_stream("chat")
    for i in range(3):
        stream.send(f"py frame {i}".encode("utf-8"))
    stream.finish()

    # 等服务端处理完事件与流帧
    time.sleep(0.5)
    client.close()
    print("E2E_CLIENT_DONE", flush=True)


if __name__ == "__main__":
    main()
