//! EchoStream 常驻聊天服务端（供 Node / Python / Web 绑定测试使用）
//!
//! 运行：`cargo run -p echostream --example chat_server`
//!
//! 提供：RPC `add`、事件 `hello`、流 `chat`、以及连接即广播 `welcome` 事件。

use echostream::prelude::*;

/// RPC：加法
#[rpc("add")]
async fn add(_session: &Session, (a, b): (u64, u64)) -> Result<u64> {
    Ok(a + b)
}

/// 事件：客户端加入
#[event("hello")]
async fn on_hello(session: &Session, data: String) -> Result<()> {
    println!("[server] 收到事件 hello: {data} <- {}", session.peer_addr());
    // 服务端反向调用客户端 RPC（双向通信演示）
    let reply: String = session.request("client_echo", &data).await?;
    println!("[server] 客户端回应: {reply}");
    Ok(())
}

/// 流：聊天流
#[stream("chat")]
async fn on_chat(session: &Session, mut stream: StreamReceiver) -> Result<()> {
    println!("[server] 流 chat 开始 <- {}", session.peer_addr());
    let mut seq = 0u64;
    while let Some(text) = stream.recv::<String>().await? {
        println!("[server] 流帧 #{seq}: {text}");
        seq += 1;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = ServerBuilder::new()
        .bind("127.0.0.1:5000")
        .add_rpc(Add)
        .add_event(OnHello)
        .add_stream(OnChat)
        .on_connect(|s| {
            println!("[server] 客户端连接: {}", s.peer_addr());
        })
        .on_disconnect(|s| {
            println!("[server] 客户端断开: {}", s.peer_addr());
        })
        .build()
        .await?;
    println!("[server] EchoStream 监听 127.0.0.1:5000");
    server.run().await
}
