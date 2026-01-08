# echostream-core

核心框架，实现 RPC 和流传输能力。

## 功能列表

- **连接管理**: QUIC 连接生命周期管理
- **协议层**: 帧定义、编解码、时间同步协议
- **RPC 框架**: 请求路由、处理器注册
- **流管理**: 流创建、时间戳对齐、抖动缓冲
- **插件系统**: ServerPlugin、ClientPlugin trait 定义
- **服务端/客户端**: 完整的服务端和客户端实现

## 子模块划分

- `connection/`: QUIC 连接生命周期管理
- `protocol/`: 帧定义、编解码、时间同步协议
- `rpc/`: RPC 框架（请求路由、处理器注册）
- `stream/`: 流管理、时间戳对齐、抖动缓冲
- `plugin/`: 插件系统（ServerPlugin、ClientPlugin trait 定义）
- `server/`: 服务端实现
- `client/`: 客户端实现

## 技术栈

- `quinn`: QUIC 协议实现
- `tokio`: 异步运行时
- `postcard`: 零拷贝序列化/反序列化
- `serde`: 序列化框架
- `bytes`: 零拷贝字节操作
- `tracing`: 结构化日志

## 核心 API 设计

### 服务端

```rust
use echostream_core::{RpcServer, Session};

#[tokio::main]
async fn main() -> Result<()> {
    let server = RpcServer::builder()
        .bind("0.0.0.0:5000")
        .handler("method", handle_request)
        .listener("event", handle_event)
        .on_start(|ctx| async move {
            println!("服务启动");
            Ok(())
        })
        .on_connect(|session| async move {
            println!("客户端连接: {}", session.peer_addr());
            Ok(())
        })
        .build()?;

    server.run().await
}

async fn handle_request(session: Session, payload: Vec<u8>) -> Result<Vec<u8>> {
    // 处理请求
    Ok(response)
}

async fn handle_event(session: Session, payload: Vec<u8>) {
    // 处理事件
}
```

### 客户端

```rust
use echostream_core::RpcClient;

#[tokio::main]
async fn main() -> Result<()> {
    let client = RpcClient::connect("127.0.0.1:5000").await?;

    // 发送请求
    let response: Vec<u8> = client.request("method", payload).await?;

    // 发送事件
    client.emit("event", payload).await?;

    // 创建流
    let stream = client.create_stream("stream_name").await?;
    stream.send(data).await?;

    Ok(())
}
```

### 插件系统

```rust
use echostream_core::{ServerPlugin, ServerBuilder};

struct MyPlugin {
    config: String,
}

impl ServerPlugin for MyPlugin {
    fn name(&self) -> &str {
        "my-plugin"
    }

    fn install(self, builder: ServerBuilder) -> Result<ServerBuilder> {
        Ok(builder
            .set("plugin.config", self.config)
            .handler("plugin.method", handle_method)
            .on_connect(|session| async move {
                println!("插件: 客户端连接");
                Ok(())
            }))
    }
}

// 使用插件
let server = RpcServer::builder()
    .bind("0.0.0.0:5000")
    .plugin(MyPlugin {
        config: "value".into(),
    })
    .build()?;
```

### 生命周期 Hook

```rust
let server = RpcServer::builder()
    .bind("0.0.0.0:5000")
    .on_start(|ctx| async move {
        // 服务启动时执行
        ctx.set("db", Database::connect().await?);
        Ok(())
    })
    .on_shutdown(|ctx| async move {
        // 服务关闭时执行
        if let Some(db) = ctx.get::<Database>("db") {
            db.close().await?;
        }
        Ok(())
    })
    .on_connect(|session| async move {
        // 客户端连接时执行
        println!("客户端 {} 已连接", session.peer_addr());
        Ok(())
    })
    .on_disconnect(|session| async move {
        // 客户端断开时执行
        println!("客户端 {} 断开", session.peer_addr());
        Ok(())
    })
    .build()?;
```

### Context 和 Session

```rust
// ServerContext: 服务端全局上下文
let server = RpcServer::builder()
    .bind("0.0.0.0:5000")
    .on_start(|ctx: ServerContext| async move {
        ctx.set("shared_data", SharedData::new());
        Ok(())
    })
    .build()?;

// Session: 单个客户端会话上下文
async fn handle_request(session: Session, req: Request) -> Result<Response> {
    // 访问服务端全局上下文
    let shared = session.server_ctx().get::<SharedData>("shared_data")?;

    // 存储会话级数据
    session.set("user_id", req.user_id);

    // 向该客户端发送消息
    session.emit("notification", "消息内容").await?;

    Ok(response)
}

// ClientContext: 客户端全局上下文
let client = RpcClient::builder()
    .connect("127.0.0.1:5000")
    .on_connect(|ctx: ClientContext| async move {
        ctx.set("local_cache", Cache::new());
        Ok(())
    })
    .build()
    .await?;
```
