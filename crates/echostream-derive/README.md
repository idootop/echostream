# echostream-derive

EchoStream 过程宏：将普通 Rust async 函数转换为框架 Handler（零大小类型），
自动完成参数提取与统一编解码，业务代码只面对强类型参数。

## 设计原则

- 声明式：隐藏载荷字节细节，开发者仅关注强类型业务函数
- 零成本：生成零大小结构体（ZST），无运行时性能损耗
- 编译期验证：非法签名（非 async、多个数据参数、流处理器缺 StreamReceiver）编译报错

## 核心宏

### #[rpc(name)] —— RPC 请求处理

- 支持 Session 注入（&Session）与数据参数（自动反序列化）
- 返回 Result<T> 或直接 T（自动序列化）
- name 缺省时取函数名

    #[rpc("add")]
    async fn add(_session: &Session, (a, b): (i64, i64)) -> Result<i64> {
        Ok(a + b)
    }

    #[rpc]                       // 方法名为 "login"
    async fn login(session: &Session, req: LoginReq) -> Result<LoginResp> {
        Ok(LoginResp::from(session, req))
    }

    #[rpc("server.status")]
    async fn status(_session: &Session) -> Result<Status> { ... }

### #[event(name)] —— 单向事件监听

- 返回 Result<()> 或 ()
- 支持 Session 注入

    #[event("hello")]
    async fn on_hello(session: &Session, msg: String) -> Result<()> {
        println!("[{}] {msg}", session.peer_addr());
        Ok(())
    }

### #[stream(name)] —— 流式数据处理

- 数据参数必须是 StreamReceiver（recv 自动反序列化帧）

    #[stream("chat")]
    async fn on_chat(_session: &Session, mut stream: StreamReceiver) -> Result<()> {
        while let Some(text) = stream.recv::<String>().await? {
            println!("{text}");
        }
        Ok(())
    }

## 参数提取规则

| 模式 | 签名示例 | 生成逻辑 |
|------|----------|----------|
| Full | fn(&Session, Req) | 注入 Session + 反序列化载荷 |
| Session Only | fn(&Session) | 仅注入 Session |
| Req Only | fn(Req) | 仅反序列化载荷 |
| Pure | fn() | 无参数 |

## 注册

宏生成同名 PascalCase 结构体（add -> Add），注册到 Builder：

    ServerBuilder::new()
        .add_rpc(Add)        // #[rpc] 生成的处理器
        .add_event(OnHello)
        .add_stream(OnChat)
        .build()
        .await?;
