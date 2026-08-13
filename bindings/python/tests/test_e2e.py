"""EchoStream Python binding 端到端测试：进程内 server + client 完整闭环"""
import threading
import time

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


def encode_tuple(a: int, b: int) -> bytes:
    """(u64, u64) 载荷：顺序 varint 编码"""
    return encode_u64(a) + encode_u64(b)


def main():
    # ===== Python 侧服务端 =====
    builder = echostream.ServerBuilder()
    builder.bind("127.0.0.1:5005")

    received = []

    def handle_add(data: bytes) -> bytes:
        a = decode_u64(data[:1])
        b = decode_u64(data[1:2])
        print(f"[py-server] add({a}, {b})")
        return encode_u64(a + b)

    def handle_hello(data: bytes) -> None:
        msg = data.decode("utf-8")
        print(f"[py-server] 收到事件: {msg}")
        received.append(msg)

    builder.add_rpc("add", handle_add)
    builder.add_event("hello", handle_hello)

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
        a = decode_u64(data[:1])
        b = decode_u64(data[1:2])
        return encode_u64(a + b)

    client.add_rpc("add", client_add)

    resp = client.request("add", encode_tuple(10, 20))
    assert decode_u64(resp) == 30, f"期望 30，实际 {decode_u64(resp)}"
    print(f"[py-client] add(10, 20) = {decode_u64(resp)}")

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
        print(f"[py-server] 主动调用客户端 add(1,1) = {decode_u64(reply)}")

    # 流
    stream = client.create_stream("chat")
    for i in range(3):
        stream.send(f"py frame {i}".encode("utf-8"))
    stream.finish()
    time.sleep(0.2)

    client.close()
    server.shutdown()
    time.sleep(0.2)

    assert "from python" in received, f"服务端未收到事件: {received}"
    assert not errors, f"服务端异常: {errors}"
    print("🎉 Python binding 端到端测试通过")


if __name__ == "__main__":
    main()
