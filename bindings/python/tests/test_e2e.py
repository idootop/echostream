"""EchoStream Python binding 端到端测试：进程内 server + client 完整闭环（自动编解码 DX）"""
import os
import sys
import threading
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "python"))

import echostream


def main():
    # ===== Python 侧服务端（新 DX：参数自动解码、返回值自动编码） =====
    builder = echostream.ServerBuilder()
    builder.bind("127.0.0.1:5005")

    received = []
    stream_frames = []

    def handle_add(a, b):
        print(f"[py-server] add({a}, {b})")
        return a + b

    def handle_hello(data):
        print(f"[py-server] 收到事件: {data}")
        received.append(data)

    def handle_chat(receiver):
        while True:
            frame = receiver.recv()
            if frame is None:
                break
            stream_frames.append(frame)

    builder.add_rpc("add", handle_add)
    builder.add_event("hello", handle_hello)
    builder.add_stream("chat", handle_chat)

    server = builder.build()
    print(f"[py-server] 监听 {server.addr()}")

    errors = []

    def run_server():
        try:
            server.run()
        except Exception as e:  # noqa: BLE001
            errors.append(e)

    thread = threading.Thread(target=run_server, daemon=True)
    thread.start()
    time.sleep(0.3)

    # ===== Python 侧客户端（新 DX） =====
    client = echostream.connect("127.0.0.1:5005")
    print("[py-client] 已连接")

    # RPC：多参数自动元组 + 响应自动解码
    total = client.request("add", 10, 20)
    assert total == 30, f"期望 30，实际 {total}"
    print(f"[py-client] add(10, 20) = {total}")

    # 事件
    client.emit("hello", "from python")
    time.sleep(0.2)

    # 双向：客户端注册 RPC 处理器
    client.on_rpc("ping", lambda: "pong")
    sessions = server.sessions()
    print(f"[py-server] 在线会话: {len(sessions)}")
    for s in sessions:
        reply = s.request("ping")
        print(f"[py-server] 主动调用客户端 ping = {reply}")
        assert reply == "pong"

    # 流
    stream = client.create_stream("chat")
    for i in range(3):
        stream.send(f"py frame {i}")
    stream.finish()
    time.sleep(0.3)

    client.close()
    server.shutdown()
    time.sleep(0.2)

    assert received == ["from python"], f"服务端未收到事件: {received}"
    assert stream_frames == ["py frame 0", "py frame 1", "py frame 2"], f"流帧不符: {stream_frames}"
    assert not errors, f"服务端异常: {errors}"
    print("🎉 Python binding 端到端测试通过（自动编解码 + 双向通信 + 流）")


if __name__ == "__main__":
    main()
