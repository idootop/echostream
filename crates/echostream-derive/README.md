# echostream-derive

过程宏模块，将普通 Rust 异步函数映射为框架 Trait 协议，完成类型转换、依赖注入和样板代码生成。

## 设计原则

- **声明式**: 隐藏 `Bytes` 处理细节，开发者仅需关注强类型业务函数
- **零成本**: 使用影子结构体 (ZST) 实现，无运行时性能损耗
- **编译期验证**: 通过过程宏在编译阶段拦截不符合要求的函数签名

## 核心宏

### `#[rpc]` - RPC 请求处理

适用于需要返回值的双向调用。

- 支持 `Session` 注入及请求体反序列化
- 必须返回 `Result<T, E>`，其中 `T` 实现 `Serialize`

```rust
use echostream_derive::rpc;

// 完整签名：Session + 请求体
#[rpc("user.login")]
async fn login(session: Session, req: LoginReq) -> Result<LoginResp> {
    println!("客户端 {} 请求登录", session.peer_addr());

    let user = authenticate(&req.username, &req.password)?;
    session.set("user_id", user.id);

    Ok(LoginResp {
        success: true,
        token: generate_token(user.id),
    })
}

// 自动取函数名：方法名为 "add"
#[rpc]
async fn add(a: u32, b: u32) -> Result<u32> {
    Ok(a + b)
}

// 仅 Session：忽略 Payload
#[rpc("server.status")]
async fn status(session: Session) -> Result<Status> {
    Ok(Status::from_session(&session))
}
```

### `#[event]` - 单向事件监听

适用于单向通知或广播，无返回值。

```rust
use echostream_derive::event;

// Session + 事件体
#[event("user.logout")]
async fn on_logout(session: Session, user_id: u64) {
    println!("用户 {} 已登出", user_id);
    session.remove("user_id");
    notify_services(user_id).await;
}

// 仅事件体
#[event("metrics.report")]
async fn on_metrics(data: MetricsData) {
    save_to_database(data).await;
}

// 无参数：纯触发逻辑
#[event("ping")]
async fn on_ping() {
    println!("收到心跳");
}
```

### `#[stream]` - 流式数据处理

适用于实时数据传输（音频流、日志流等）。

```rust
use echostream_derive::stream;

#[stream("audio.stream")]
async fn handle_audio(session: Session, mut stream: StreamReceiver) {
    println!("客户端 {} 开启音频流", session.peer_addr());

    while let Some(frame) = stream.recv().await {
        process_audio_frame(&frame).await;
    }

    println!("音频流结束");
}
```

## 参数提取规则

宏根据函数签名自动推断处理逻辑：

| 模式             | 签名示例           | 生成逻辑                             |
| ---------------- | ------------------ | ------------------------------------ |
| **Full**         | `fn(Session, Req)` | 注入 Session 并反序列化 Payload      |
| **Session Only** | `fn(Session)`      | 仅注入 Session，忽略 Payload         |
| **Req Only**     | `fn(Req)`          | 仅反序列化 Payload，忽略会话上下文   |
| **Pure**         | `fn()`             | 无参数，纯逻辑触发                   |

## 展开示例

以 `#[rpc]` 为例，宏将业务函数转换为框架 Trait 实现：

```rust
// --- 原始代码 ---
#[rpc("user.login")]
async fn login(session: Session, req: LoginReq) -> Result<LoginResp> { ... }

// --- 展开后 ---
#[allow(non_camel_case_types)]
pub struct login; // 影子 ZST 结构体

impl RpcHandler for login {
    fn name(&self) -> &'static str { "user.login" }

    fn handle(&self, session: Session, payload: Bytes) -> BoxFuture<'static, Result<Bytes>> {
        Box::pin(async move {
            // 1. 反序列化请求
            let req: LoginReq = postcard::from_bytes(&payload)?;

            // 2. 调用业务函数
            let result = super::login(session, req).await?;

            // 3. 序列化响应
            Ok(Bytes::from(postcard::to_allocvec(&result)?))
        })
    }
}
```

## 子模块划分

- `parser.rs`: 使用 `darling` + `syn` 解析宏属性和函数签名
- `extractors.rs`: 根据函数参数匹配提取策略
- `codegen/`: 包含 `rpc`、`event`、`stream` 三大代码生成器
