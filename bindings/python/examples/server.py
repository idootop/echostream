"""EchoStream Python 示例：服务端

注册 add RPC（(a,b) -> a+b）、hello 事件、chat 流（接收并打印帧），监听 127.0.0.1:5102。
运行：python3 examples/server.py（另开终端运行 examples/client.py），Ctrl+C 优雅退出。
"""
import signal
import threading

import echostream


def decode_u64(data: bytes) -> int:
    """postcard varint 解码（u64）"""
    result = 0
    shift = 0
    for b in data:
        result |= (b & 0x7F) << shift
        if not (b & 0x80):
            break
        shift += 7
    return result


def encode_u64(n: int) -> bytes:
    """postcard varint 编码（u64）"""
    out = bytearray()
    while n >= 0x80:
        out.append((n & 0x7F) | 0x80)
        n >>= 7
    out.append(n)
    return bytes(out)


server = None  # 供信号处理器引用


def handle_sigint(sig, frame):  # noqa: ARG001
    print("\n[server] 收到 Ctrl+C，正在关闭...")
    if server is not None:
        server.shutdown()


def main():
    global server
    builder = echostream.ServerBuilder()
    builder.bind("127.0.0.1:5102")

    # add RPC：载荷 (u64, u64) → 响应 u64
    def handle_add(data: bytes) -> bytes:
        a = decode_u64(data[:1])
        b = decode_u64(data[1:2])
        print(f"[server] add({a}, {b})")
        return encode_u64(a + b)

    # hello 事件：打印收到的文本
    def handle_hello(data: bytes) -> None:
        print(f"[server] 收到事件: {data.decode('utf-8')}")

    # chat 流：拉帧直到结束
    def handle_chat(receiver) -> None:
        count = 0
        while True:
            frame = receiver.recv()
            if frame is None:
                break
            count += 1
            print(f"[server] 流帧 #{count}: {frame.decode('utf-8')}")
        print(f"[server] 流 chat 结束，共 {count} 帧")

    builder.add_rpc("add", handle_add)
    builder.add_event("hello", handle_hello)
    builder.add_stream("chat", handle_chat)

    server = builder.build()
    print(f"[server] 监听 {server.addr()}")

    # run 阻塞，放后台线程；主线程等待 Ctrl+C（信号处理器调用 shutdown 优雅退出）
    signal.signal(signal.SIGINT, handle_sigint)
    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()
    signal.pause()  # 阻塞直到信号到达，处理器返回后继续
    thread.join()
    print("[server] 已退出")


if __name__ == "__main__":
    main()
