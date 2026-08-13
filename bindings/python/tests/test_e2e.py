"""EchoStream Python binding 端到端测试：进程内 server + client 完整闭环"""
import threading
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


def encode_tuple(a: int, b: int) -> bytes:
    """(i64, i64) 载荷：顺序 ZigZag varint 编码"""
    return encode_i64(a) + encode_i64(b)


def main():
    # ===== Python 侧服务端 =====
    builder = echostream.ServerBuilder()
    builder.bind("127.0.0.1:5005")

    received = []

    def handle_add(data: bytes) -> bytes:
        a = decode_i64(data[:1])
        b = decode_i64(data[1:2])
        print(f"[py-server] add({a}, {b})")
        return encode_i64(a + b)

    def handle_hello(data: bytes) -> None:
        msg = data.decode("utf-8")
        print(f"[py-server] 收到事件: {msg}")
        received.append(msg)

    builder.add_rpc("add", handle_add)
    builder.add_event("hello", handle_hello)

    # 流接收：Python 侧拉帧
    stream_frames = []

    def handle_chat(receiver):
        while True:
            frame = receiver.recv()
            if frame is None:
                break
            stream_frames.append(frame.decode("utf-8"))

    builder.add_stream("chat", handle_chat)

    server = builder.build()
    print(f"[py-server] 监听 {server.addr()}")

    # run 会阻塞，放后台线程
    errors = []
    def run_server():
        try:
            server.run()
        except Exception as e:  # noqa: BLE001
            errors.append(e)
    thread = threading.Thread(target=run_server, daemon=True)
    thread.start()
    time.sleep(0.3)

    # ===== Python 侧客户端 =====
    client = echostream.connect("127.0.0.1:5005")
    print("[py-client] 已连接")

    # 客户端也注册 add 处理器（支持服务端主动调用，双向通信）
    def client_add(data: bytes) -> bytes:
        a = decode_i64(data[:1])
        b = decode_i64(data[1:2])
        return encode_i64(a + b)

    client.add_rpc("add", client_add)

    resp = client.request("add", encode_tuple(10, 20))
    assert decode_i64(resp) == 30, f"期望 30，实际 {decode_i64(resp)}"
    print(f"[py-client] add(10, 20) = {decode_i64(resp)}")

    client.emit("hello", "from python".encode("utf-8"))
    time.sleep(0.2)

    # 广播
    server.broadcast("hello", "broadcast!".encode("utf-8"))
    time.sleep(0.2)

    # 会话主动调用（服务端视角）
    sessions = server.sessions()
    print(f"[py-server] 在线会话: {len(sessions)}")
    for s in sessions:
        reply = s.request("add", encode_tuple(1, 1))
        print(f"[py-server] 主动调用客户端 add(1,1) = {decode_i64(reply)}")

    # 流
    stream = client.create_stream("chat")
    for i in range(3):
        stream.send(f"py frame {i}".encode("utf-8"))
    stream.finish()
    time.sleep(0.3)

    client.close()
    server.shutdown()
    time.sleep(0.2)

    assert "from python" in received, f"服务端未收到事件: {received}"
    assert stream_frames == ["py frame 0", "py frame 1", "py frame 2"], f"流帧不符: {stream_frames}"
    assert not errors, f"服务端异常: {errors}"
    print("🎉 Python binding 端到端测试通过（含流接收）")


if __name__ == "__main__":
    main()
