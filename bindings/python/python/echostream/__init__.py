"""EchoStream Python SDK —— 自动编解码 DX

用法：
    import echostream

    # 客户端
    client = echostream.connect("127.0.0.1:5000")
    total = client.request("add", 10, 20)      # 30，自动编解码
    client.emit("hello", "world")
    client.on_event("hello", lambda data: print(data))
    client.on_rpc("ping", lambda: "pong")     # 双向

    # 服务端
    builder = echostream.ServerBuilder()
    builder.bind("0.0.0.0:5000")
    builder.add_rpc("add", lambda a, b: a + b)  # 参数自动解码、返回值自动编码
    builder.add_event("hello", lambda data: print(data))
    server = builder.build()
    server.run()

底层 API（手动字节）通过 echostream.native 访问。
"""

from __future__ import annotations

import inspect

from . import _native
from .postcard import decode_payload, encode_payload

__all__ = [
    "connect",
    "Client",
    "Stream",
    "StreamReceiver",
    "ServerBuilder",
    "Server",
    "Session",
    "encode_payload",
    "decode_payload",
    "native",
]


def _split_args(args, kw):
    """解析多参数形式：payload | (payload, options) | 多参数 -> 元组"""
    options = {k: v for k, v in kw.items() if k in ("decode", "encode")}
    if not args:
        return None, options
    if len(args) == 1:
        return args[0], options
    if len(args) == 2 and isinstance(args[1], dict) and (
        "decode" in args[1] or "encode" in args[1]
    ):
        return args[0], {**options, **args[1]}
    return list(args), options  # 多参数 -> 元组载荷


def _encode_args(payload, options):
    schema = options.get("encode") if options else None
    return encode_payload(payload, schema)


def _decode_args(data, options):
    if not options:
        return decode_payload(data)
    decode = options.get("decode")
    if decode is None:
        return decode_payload(data)
    if callable(decode):
        return decode(data)
    return decode_payload(data, decode)


def _spread(payload):
    """载荷解码并按元组约定展开为多参数（空载荷 -> 无参数）"""
    decoded = decode_payload(payload)
    if decoded is None:
        return []
    return decoded if isinstance(decoded, list) else [decoded]


class Client:
    """客户端（自动编解码）"""

    def __init__(self, native):
        self._n = native

    def request(self, name, *args, **kw):
        payload, options = _split_args(args, kw)
        resp = self._n.request(name, _encode_args(payload, options))
        return _decode_args(resp, options)

    def emit(self, name, payload=None, **kw):
        self._n.emit(name, _encode_args(payload, kw))

    def emit_unreliable(self, name, payload=None, **kw):
        self._n.emit_unreliable(name, _encode_args(payload, kw))

    def create_stream(self, name):
        return Stream(self._n.create_stream(name))

    def on_event(self, name, handler):
        """注册事件监听，返回取消注册函数（off）"""
        token = self._n.on_event(name, lambda data: handler(*_spread(data)))

        def off():
            self._n.off_event(token)

        return off

    def on_rpc(self, name, handler):
        """注册 RPC 处理器（处理服务端主动调用），返回取消注册函数（off）"""

        def wrapped(data):
            result = handler(*_spread(data))
            if result is None:
                raise ValueError("RPC 处理器未返回响应")
            return _encode_args(result, None)

        token = self._n.add_rpc(name, wrapped)

        def off():
            self._n.off_rpc(token)

        return off

    def on_stream(self, name, handler):
        """注册流处理器（服务端推送），返回取消注册函数（off）"""
        token = self._n.add_stream(name, lambda receiver: handler(StreamReceiver(receiver)))

        def off():
            self._n.off_stream(token)

        return off

    def close(self):
        self._n.close()


class Stream:
    """流发送器（帧自动编码）"""

    def __init__(self, native):
        self._n = native

    def send(self, payload, **kw):
        self._n.send(_encode_args(payload, kw))

    def finish(self):
        self._n.finish()


class StreamReceiver:
    """流接收器（帧自动解码）"""

    def __init__(self, native):
        self._n = native

    def recv(self):
        frame = self._n.recv()
        return None if frame is None else decode_payload(frame)


class ServerBuilder:
    """服务端构建器（处理器参数自动解码、返回值自动编码）"""

    def __init__(self):
        self._n = _native.ServerBuilder()

    def bind(self, addr):
        self._n.bind(addr)

    def add_rpc(self, name, handler):
        def wrapped(data):
            result = handler(*_spread(data))
            if result is None:
                raise ValueError("RPC 处理器未返回响应")
            return _encode_args(result, None)

        self._n.add_rpc(name, wrapped)

    def add_event(self, name, handler):
        self._n.add_event(name, lambda data: handler(*_spread(data)))

    def add_stream(self, name, handler):
        self._n.add_stream(name, lambda receiver: handler(StreamReceiver(receiver)))

    def build(self):
        return Server(self._n.build())


class Server:
    """服务端"""

    def __init__(self, native):
        self._n = native

    def run(self):
        self._n.run()

    def shutdown(self):
        self._n.shutdown()

    def addr(self):
        return self._n.addr()

    def broadcast(self, name, payload=None, **kw):
        self._n.broadcast(name, _encode_args(payload, kw))

    def sessions(self):
        return [Session(s) for s in self._n.sessions()]


class Session:
    """会话（服务端视角：可主动调用客户端）"""

    def __init__(self, native):
        self._n = native

    def id(self):
        return self._n.id()

    def peer_addr(self):
        return self._n.peer_addr()

    def request(self, name, *args, **kw):
        payload, options = _split_args(args, kw)
        resp = self._n.request(name, _encode_args(payload, options))
        return _decode_args(resp, options)

    def emit(self, name, payload=None, **kw):
        self._n.emit(name, _encode_args(payload, kw))

    def close(self):
        self._n.close()


def connect(url):
    """连接服务端（QUIC）"""
    return Client(_native.connect(url))

