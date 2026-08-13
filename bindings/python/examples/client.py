"""EchoStream Python 示例：客户端

连接 127.0.0.1:5102，调用 add(10,20)、发送 hello 事件、推送 3 帧 chat 流后退出。
运行：python3 examples/client.py（需先启动 examples/server.py）
"""
import time

import echostream


def decode_i64(data: bytes) -> int:
    """postcard varint 解码（i64，ZigZag：与 Rust 核心一致）"""
    v = 0
    shift = 0
    for b in data:
        v |= (b & 0x7F) << shift
        if not (b & 0x80):
            break
        shift += 7
    return (v >> 1) ^ -(v & 1)


def encode_i64(n: int) -> bytes:
    """postcard varint 编码（i64，ZigZag：10 → 0x14）"""
    zz = ((n << 1) ^ (n >> 63)) & ((1 << 64) - 1)
    out = bytearray()
    while zz >= 0x80:
        out.append((zz & 0x7F) | 0x80)
        zz >>= 7
    out.append(zz)
    return bytes(out)


def main():
    client = echostream.connect("127.0.0.1:5102")
    print("[client] 已连接")

    # RPC：add(10, 20) -> 30
    resp = client.request("add", encode_i64(10) + encode_i64(20))
    print(f"[client] add(10, 20) = {decode_i64(resp)}")

    # 事件：hello
    client.emit("hello", "来自 python 客户端".encode("utf-8"))

    # 流：chat 推送 3 帧
    stream = client.create_stream("chat")
    for i in range(3):
        stream.send(f"py frame {i}".encode("utf-8"))
    stream.finish()

    # 等服务端处理完再退出
    time.sleep(0.3)
    client.close()
    print("[client] 完成")


if __name__ == "__main__":
    main()
