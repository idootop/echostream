# EchoStream 数据转换中间件

数据面扩展：请求 / 响应载荷转换（压缩、加密、格式包装等）。

## 用法

```rust
use echostream_middleware_transform::TransformMiddleware;

// 示例：对请求载荷追加版本头、响应载荷加后缀（演示性转换）
ServerBuilder::new()
    .middleware(
        TransformMiddleware::new()
            .map_request(|data| Ok(Bytes::from([&b"v1:"[..], &data[..]].concat())))
            .map_response(|data| Ok(Bytes::from([&data[..], &b":ok"[..]].concat()))),
    )
    .build().await?;
```

- `map_request`：进入处理器前转换（RPC 请求 / 事件 / 流帧载荷）
- `map_response`：处理器返回后转换（RPC 响应载荷）
