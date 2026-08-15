# echostream (Python)

EchoStream 的 Python 绑定（PyO3）—— 基于 QUIC 的高性能双向 RPC / Event / Stream 框架。

完整 client + server 能力，与 Rust 核心共享同一份实现；**载荷自动编解码**（纯 Python postcard 实现），
业务侧直接传 Python 原生值。

## 安装（uv + Python 3.14）

本仓库已用 uv pin 到最新稳定 Python（仓库根 `.python-version` = 3.14），
所有 `uv run` 命令自动使用该版本与项目 `.venv`：

```bash
cd bindings/python
uv venv .venv                          # 创建 venv（按 .python-version 选择 3.14）
uv pip install maturin
uv run maturin develop                 # 构建并安装（editable；自动用 .venv）
```

> 若 shell 中有 conda 环境变量，直接 `uv run` 即可（uv 不依赖 VIRTUAL_ENV/CONDA_PREFIX 探测）。

发布构建：

```bash
cd bindings/python
.venv/bin/maturin build --release
.venv/bin/pip install target/wheels/echostream-*.whl
```

> ⚠️ 不要直接 `cargo build -p echostream-python`：pyo3 `extension-module` 模式的扩展
> 不链接 libpython（符号由解释器运行时提供），直接 cargo 链接必然报 Undefined symbols；
> 构建必须经 maturin（pyproject.toml 已配置 build-backend）。
> 若需 cargo 侧单独验证 Rust 代码，CI 使用 `--exclude echostream-python`。

## 客户端

```python
import echostream

client = echostream.connect("127.0.0.1:5000")
total = client.request("add", 10, 20)      # 30，自动编解码
client.emit("hello", "world")
client.on_event("hello", lambda data: print(data))
client.on_rpc("ping", lambda: "pong")      # 双向通信

stream = client.create_stream("chat")
stream.send("hi")
stream.finish()
client.close()
```

## 服务端

```python
import echostream

builder = echostream.ServerBuilder()
builder.bind("0.0.0.0:5000")
builder.add_rpc("add", lambda a, b: a + b)   # 参数自动解码、返回值自动编码
builder.add_event("hello", lambda data: print(data))
builder.add_stream("chat", lambda receiver: ...)  # receiver.recv() 自动解码

server = builder.build()
server.run()  # 阻塞；请在独立线程运行，另线程 shutdown
```

## 编解码约定

int → i64 ZigZag varint（超出 i64 范围的非负 int → u64 varint）；float → f64；
bool → 单字节；str/bytes → 长度前缀；list/tuple → 元组字段序；dict → 结构体字段序。
解码默认智能推断；歧义场景传 schema：`client.request("get", {"id": 1}, decode={"id": "number"})`。

底层手动字节 API 通过 `echostream.native` 访问。

## 测试

```bash
.venv/bin/python tests/test_e2e.py          # server + client 闭环
.venv/bin/python tests/cross_server.py [端口]  # 跨端矩阵对端
```
