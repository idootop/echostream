"""EchoStream 跨端矩阵：Python 服务端（供 Rust / Node 客户端连接）

新 DX：处理器参数自动解码、返回值自动编码，线缆格式与 Rust 核心一致。
用法：python3 cross_server.py [端口]（默认 5110）
"""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "python"))

import echostream


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 5110
    addr = f"127.0.0.1:{port}"
    builder = echostream.ServerBuilder()
    builder.bind(addr)

    def handle_add(a, b):
        print(f"E2E_RPC add({a}, {b})", flush=True)
        return a + b

    def handle_hello(data):
        print(f"E2E_EVENT_RECEIVED: {data}", flush=True)

    def handle_chat(receiver):
        n = 0
        while True:
            frame = receiver.recv()
            if frame is None:
                break
            print(f"E2E_STREAM_FRAME {n}: {frame}", flush=True)
            n += 1
        print(f"E2E_STREAM_FRAMES={n}", flush=True)

    builder.add_rpc("add", handle_add)
    builder.add_event("hello", handle_hello)
    builder.add_stream("chat", handle_chat)

    server = builder.build()
    print(f"E2E_SERVER_READY {addr}", flush=True)
    server.run()


if __name__ == "__main__":
    main()
