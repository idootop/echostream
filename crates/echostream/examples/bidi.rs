//! EchoStream 双向通信示例
//!
//! 展示：生命周期钩子、服务端主动调用客户端 RPC、客户端事件监听、中间件。
//!
//! 运行：`cargo run -p echostream --example bidi`

use async_trait::async_trait;
use echostream::prelude::*;

// ======================== 服务端处理器 ========================

/// RPC：客户端调用服务端
#[rpc("ping")]
async fn ping(_session: &Session, msg: String) -> Result<String> {
    Ok(format!("pong: {msg}"))
}

/// Event：客户端加入，服务端反向调用客户端 RPC
#[event("join")]
async fn on_join(session: &Session, user: String) -> Result<()> {
    println!("[server] 用户 {user} 加入");
    // 服务端主动调用客户端 RPC（双向调用）
    let reply: String = session.request("client_hello", &user).await?;
    println!("[server] 客户端回应: {reply}");
    Ok(())
}

// ======================== 客户端处理器 ========================

/// RPC：服务端主动调用客户端
#[rpc("client_hello")]
async fn client_hello(_session: &Session, user: String) -> Result<String> {
    println!("[client] 收到服务端调用 client_hello({user})");
    Ok(format!("hello, server! I'm {user}"))
}

/// Event：接收服务端推送的欢迎事件
#[event("welcome")]
async fn on_welcome(_session: &Session, msg: String) -> Result<()> {
    println!("[client] 收到服务端事件 welcome: {msg}");
    Ok(())
}

// ======================== 中间件 ========================

/// 日志中间件：打印所有入站消息
struct LogMiddleware;

#[async_trait]
impl Middleware for LogMiddleware {
    fn name(&self) -> &str {
        "log"
    }

    async fn handle(&self, session: &Session, msg: Message, next: Next) -> Result<Option<Message>> {
        let kind = match &msg {
            Message::Request(_) => "RPC 请求",
            Message::Event(_) => "事件",
            _ => "其他",
        };
        println!("[middleware] 拦截到{kind} <- session {}", session.id());
        next.run(msg).await
    }
}

// ======================== 主入口 ========================

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "127.0.0.1:5001".to_string();
    let server_addr = addr.clone();

    let server = ServerBuilder::new()
        .bind(&server_addr)
        .add_rpc(Ping)
        .add_event(OnJoin)
        .middleware(LogMiddleware)
        .on_start(|ctx| println!("[server] 启动，全局状态: {:?}", ctx.sessions().len()))
        .on_connect(|s| println!("[server] 客户端连接: {}", s.peer_addr()))
        .on_disconnect(|s| println!("[server] 客户端断开: {}", s.peer_addr()))
        .build()
        .await?;
    let server_handle = tokio::spawn(async move { server.run().await });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let client = ClientBuilder::new()
        .add_rpc(ClientHello)
        .add_event(OnWelcome)
        .connect(&addr)
        .await?;
    println!("[client] 已连接");

    // 客户端调用服务端 RPC
    let reply: String = client.request("ping", &"你好".to_string()).await?;
    println!("[client] ping -> {reply}");

    // 发送加入事件，触发服务端反向调用
    client.emit("join", &"alice".to_string()).await?;

    // 等待双向调用与事件完成
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    client.close();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    server_handle.abort();
    println!("[client] 全部完成");
    Ok(())
}

// 说明：优雅关闭见 discovery 示例（server.shutdown()）
