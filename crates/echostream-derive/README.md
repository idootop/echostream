# echostream-derive

过程宏，简化处理器定义。

## 功能列表

- **handler 宏**: 简化请求处理器定义
- **listener 宏**: 简化事件监听器定义
- **stream_handler 宏**: 简化流处理器定义

## 子模块划分

- `handler.rs`: 请求处理器宏
- `listener.rs`: 事件监听器宏
- `stream_handler.rs`: 流处理器宏

## 技术栈

- `syn`: 解析 Rust 语法
- `quote`: 生成 Rust 代码
- `proc-macro2`: 过程宏工具

## 核心 API 设计

### handler 宏

用于定义请求处理器，支持 Request/Response 模式。

```rust
use echostream::prelude::*;

// 基础用法
#[echostream::handler("user.login")]
async fn login(session: Session, req: LoginReq) -> Result<LoginResp> {
    println!("客户端 {} 请求登录", session.peer_addr());

    // 验证用户
    let user = authenticate(&req.username, &req.password)?;

    // 存储会话信息
    session.set("user_id", user.id);

    Ok(LoginResp {
        success: true,
        token: generate_token(user.id),
    })
}

// 支持多种参数组合
#[echostream::handler("user.info")]
async fn get_user_info(session: Session) -> Result<UserInfo> {
    let user_id = session.get::<u64>("user_id")?;
    let user = load_user(user_id).await?;
    Ok(user.into())
}

// 访问服务端全局上下文
#[echostream::handler("data.query")]
async fn query_data(session: Session, query: Query) -> Result<QueryResult> {
    let db = session.server_ctx().get::<Database>("db")?;
    let result = db.query(&query).await?;
    Ok(result)
}
```

### listener 宏

用于定义事件监听器，处理单向消息通知。

```rust
use echostream::prelude::*;

// 基础用法
#[echostream::listener("user.logout")]
async fn on_logout(session: Session, user_id: u64) {
    println!("用户 {} 已登出", user_id);

    // 清理会话数据
    session.remove("user_id");

    // 通知其他服务
    notify_other_services(user_id).await;
}

// 无参数事件
#[echostream::listener("ping")]
async fn on_ping(session: Session) {
    println!("收到 ping 来自: {}", session.peer_addr());
}

// 客户端监听服务端事件
#[echostream::listener("server.broadcast")]
async fn on_broadcast(ctx: ClientContext, msg: String) {
    println!("服务端广播: {}", msg);

    // 访问客户端全局上下文
    let cache = ctx.get::<Cache>("cache")?;
    cache.update(&msg).await;
}
```

### stream_handler 宏

用于定义流处理器，处理实时数据流。

```rust
use echostream::prelude::*;

// 基础用法
#[echostream::stream_handler("audio.stream")]
async fn handle_audio_stream(session: Session, mut stream: StreamReceiver) {
    println!("客户端 {} 开启音频流", session.peer_addr());

    // 接收并处理流数据
    while let Some(frame) = stream.recv().await {
        process_audio_frame(&frame).await;
    }

    println!("音频流结束");
}

// 带时间戳对齐的流
#[echostream::stream_handler("audio.sync")]
async fn handle_synced_stream(session: Session, mut stream: StreamReceiver) {
    // 接收带时间戳的数据
    while let Some((frame, timestamp)) = stream.recv_with_timestamp().await {
        // 根据时间戳调度播放
        schedule_playback(frame, timestamp).await;
    }
}

// 双向流
#[echostream::stream_handler("video.call")]
async fn handle_video_call(session: Session, mut stream: BiStream) {
    // 同时发送和接收
    tokio::spawn(async move {
        while let Some(frame) = capture_video().await {
            stream.send(frame).await.unwrap();
        }
    });

    while let Some(frame) = stream.recv().await {
        display_video(frame).await;
    }
}
```

## 宏展开示例

### handler 宏展开

```rust
// 原始代码
#[echostream::handler("user.login")]
async fn login(session: Session, req: LoginReq) -> Result<LoginResp> {
    // ...
}

// 展开后
pub fn login_handler() -> Handler {
    Handler::new("user.login", |session, payload| {
        Box::pin(async move {
            let req: LoginReq = postcard::from_bytes(&payload)?;
            let resp = login(session, req).await?;
            let bytes = postcard::to_allocvec(&resp)?;
            Ok(bytes)
        })
    })
}
```

### listener 宏展开

```rust
// 原始代码
#[echostream::listener("user.logout")]
async fn on_logout(session: Session, user_id: u64) {
    // ...
}

// 展开后
pub fn on_logout_listener() -> Listener {
    Listener::new("user.logout", |session, payload| {
        Box::pin(async move {
            let user_id: u64 = postcard::from_bytes(&payload)?;
            on_logout(session, user_id).await;
            Ok(())
        })
    })
}
```

## 使用示例

```rust
use echostream::prelude::*;

// 定义处理器
#[echostream::handler("calc.add")]
async fn add(session: Session, req: AddRequest) -> Result<AddResponse> {
    Ok(AddResponse {
        result: req.a + req.b,
    })
}

// 定义监听器
#[echostream::listener("event.notify")]
async fn on_notify(session: Session, msg: String) {
    println!("收到通知: {}", msg);
}

// 定义流处理器
#[echostream::stream_handler("data.stream")]
async fn handle_stream(session: Session, mut stream: StreamReceiver) {
    while let Some(data) = stream.recv().await {
        process(data).await;
    }
}

// 注册到服务器
#[tokio::main]
async fn main() -> Result<()> {
    let server = RpcServer::builder()
        .bind("0.0.0.0:5000")
        .handler(add)
        .listener(on_notify)
        .stream_handler(handle_stream)
        .build()?;

    server.run().await
}
```
