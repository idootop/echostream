# echostream (Python)

EchoStream 的 Python 绑定（PyO3）—— 基于 QUIC 的高性能双向 RPC / Event / Stream 框架。

完整 client + server 能力，与 Rust 核心共享同一份实现。

## 安装

```bash
maturin build --release
pip install target/wheels/echostream-*.whl
```

## 服务端

```python
import echostream

builder = echostream.ServerBuilder()
builder.bind("0.0.0.0:5000")

def handle_add(data: bytes) -> bytes:
    return encode_u64(decode_u64(data[:1]) + decode_u64(data[1:2]))

builder.add_rpc("add", handle_add)
builder.add_event("hello", lambda data: print("事件:", data))
builder.add_stream("chat", lambda receiver: ...)  # receiver.recv() 拉帧

server = builder.build()
# 在独立线程运行（run 阻塞直到 shutdown）
import threading
threading.Thread(target=server.run, daemon=True).start()
```

## 客户端

```python
client = echostream.connect("127.0.0.1:5000")
resp = client.request("add", b"\x0a\x14")   # postcard 编码的 (10, 20)
client.emit("hello", b"world")
stream = client.create_stream("chat")
stream.send(b"frame-1")
stream.finish()
client.close()
```

## 载荷约定

所有载荷为 **postcard 编码字节**（与 Rust 线缆格式一致，varint 编码）。
同步 API：内部使用 tokio runtime；阻塞期间自动释放 GIL，线程安全。

## 测试

```bash
python tests/test_e2e.py   # server + client 进程内闭环
```

## 示例

两个进程演示完整链路（先开服务端，再开客户端）：

```bash
# 终端 1：服务端（监听 127.0.0.1:5102，Ctrl+C 优雅退出）
python3 examples/server.py

# 终端 2：客户端（调用 add、发送事件、推送流后退出）
python3 examples/client.py
```
