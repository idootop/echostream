# echostream-derive

`echostream-derive` 负责将普通的 Rust 异步函数映射为框架的 `Trait` 协议，在编译期完成类型转换、依赖注入和样板代码生成。

## 核心设计哲学

- **声明式 API**：隐藏 `Bytes` 处理细节，开发者仅需关注强类型业务函数。
- **零成本抽象**：使用影子结构体 (Shadow Structs) 实现零大小类型 (ZST)，无运行时性能损耗。
- **编译期验证**：通过过程宏在编译阶段拦截不符合 `Session` 或 `Codec` 要求的函数签名。

---

## 技术架构与链路

### 1. 内部模块

- `parser.rs`: 使用 `darling` 提取宏属性（如 `name`, `timeout`），使用 `syn` 解析函数签名（Args, Return）。
- `extractors.rs`: 根据函数参数自动匹配提取策略（Session-Only, Payload-Only, Full 等）。
- `codegen/`: 包含 `rpc`, `event`, `stream` 三大生成器，负责实现框架核心 Trait。

### 2. 代码展开工作流 (以 `#[rpc]` 为例)

**宏处理逻辑示例：**

```rust
// --- 输入：原始业务代码 ---
#[echostream::rpc("user.login")]
async fn login(session: Session, req: LoginReq) -> Result<LoginResp> { ... }

// --- 展开：生成的底层支撑 ---
#[allow(non_camel_case_types)]
pub struct login; // 1. 影子 ZST 结构体

impl echostream::core::RpcHandler for login {
    fn name(&self) -> &'static str { "user.login" }

    fn handle(&self, session: Session, payload: Bytes) -> BoxFuture<'static, Result<Bytes, EchoError>> {
        Box::pin(async move {
            // 2. 依赖注入与反序列化 (Extracting)
            let req: LoginReq = postcard::from_bytes(&payload).map_err(..)?;

            // 3. 调用业务函数
            let result = super::login(session, req).await.map_err(..)?;

            // 4. 序列化结果 (Encoding)
            Ok(Bytes::from(postcard::to_allocvec(&result)?))
        })
    }
}

```

---

## 提取器匹配规则 (Argument Patterns)

宏根据函数签名自动推断逻辑，支持以下模式：

| 模式             | 签名示例           | 生成逻辑                             |
| ---------------- | ------------------ | ------------------------------------ |
| **Full**         | `fn(Session, Req)` | 获取 Session 并反序列化 Payload。    |
| **Session Only** | `fn(Session)`      | 仅注入 Session，忽略 Payload。       |
| **Req Only**     | `fn(Req)`          | 仅反序列化 Payload，忽略会话上下文。 |
| **Pure**         | `fn()`             | 无参数触发逻辑。                     |

---

## 三大核心宏规格

### 1. `#[rpc]` (Request/Response)

适用于需要确认返回值的双向调用。

- **输入**：支持 `Session` 注入及反序列化请求体。
- **输出**：必须返回 `Result<T, E>`，其中 `T` 需实现 `Serialize`。

### 2. `#[event]` (One-way)

适用于单向通知或广播。

- **特点**：无返回值（`()`）

### 3. `#[stream]` (Streaming)

适用于实时数据传输（如音频流、日志流）。

- **参数**：支持 `StreamReceiver`。


---

## 示例代码

### rpc 宏

用于定义请求处理器，支持 Request/Response 模式。

```rust
use echostream::prelude::*;

// 基础用法
#[echostream::rpc("user.login")]
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

// 支持不指定方法名称，自动取函数名称
#[echostream::rpc]
async fn add(a: u32, b: u32) -> Result<u32> {
    Ok(a + b)
}

// 无参数请求
#[echostream::event("server.info")]
async fn get_server_info() -> Result<ServerInfo> {
    Ok(ServerInfo { ... })
} 
```

### event 宏

用于定义事件监听器，处理单向消息通知。

```rust
use echostream::prelude::*;

// 基础用法
#[echostream::event("user.logout")]
async fn on_logout(session: Session, user_id: u64) {
    println!("用户 {} 已登出", user_id);

    // 清理会话数据
    session.remove("user_id");

    // 通知其他服务
    notify_other_services(user_id).await;
}

// 无参数事件
#[echostream::event("ping")]
async fn on_ping(session: Session) {
    println!("收到 ping 来自: {}", session.peer_addr());
}
```

### stream 宏

用于定义流处理器，处理实时数据流。

```rust
use echostream::prelude::*;

// 基础用法
#[echostream::stream("audio.stream")]
async fn handle_audio_stream(session: Session, mut stream: StreamReceiver) {
    println!("客户端 {} 开启音频流", session.peer_addr());

    // 接收并处理流数据
    while let Some(frame) = stream.recv().await {
        process_audio_frame(&frame).await;
    }

    println!("音频流结束");
}
```
