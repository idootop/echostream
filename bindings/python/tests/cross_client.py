"""EchoStream 跨端矩阵：Python 客户端（连接 Rust / Node 服务端）

用法：python3 cross_client.py [地址]（默认 127.0.0.1:5110）
"""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "python"))

import echostream


def main():
    addr = sys.argv[1] if len(sys.argv) > 1 else "127.0.0.1:5110"
    client = echostream.connect(addr)
    print("[client] 已连接")

    # RPC：add(10, 20) -> 30（自动编解码）
    total = client.request("add", 10, 20)
    print(f"add(10, 20) = {total}")
    if total != 30:
        raise AssertionError(f"期望 30，实际 {total}")

    # 事件：hello
    client.emit("hello", "from python client")

    # 流：chat 推送 3 帧
    stream = client.create_stream("chat")
    for i in range(3):
        stream.send(f"py frame {i}")
    stream.finish()

    import time

    time.sleep(0.2)
    client.close()
    print("[client] 完成")


if __name__ == "__main__":
    main()
