//! EchoStream 端到端示例：RPC / Event / Stream 三种通信模式
//!
//! 使用 `#[rpc]` / `#[event]` / `#[stream]` 声明式宏定义处理器，
//! 同一进程内启动服务端与客户端，验证完整链路。
//!
//! 运行：`cargo run -p echostream --example simple_rpc`

use echostream::prelude::*;

// ======================== 处理器定义（声明式宏） ========================

/// RPC：add(a, b) -> a + b
#[rpc("add")]
async fn add(_session: &Session, (a, b): (i64, i64)) -> Result<i64> {
    Ok(a + b)
}

/// Event：处理客户端发来的 "hello" 事件
#[event("hello")]
async fn on_hello(session: &Session, data: String) -> Result<()> {
    println!(
        "[server] 收到事件 hello: {data:?} <- {}",
        session.peer_addr()
    );
    Ok(())
}

/// Stream：处理客户端推送的 "chat" 流
#[stream("chat")]
async fn on_chat(session: &Session, mut stream: StreamReceiver) -> Result<()> {
    println!("[server] 流 chat 开始 <- {}", session.peer_addr());
    while let Some(frame) = stream.recv().await? {
        let text = String::from_utf8_lossy(&frame.data);
        println!("[server] 流帧 #{}: {text}", frame.seq);
    }
    println!("[server] 流 chat 结束");
    Ok(())
}

// ======================== 服务端 ========================

async fn run_server(addr: String) -> Result<()> {
    let server = ServerBuilder::new()
        .bind(&addr)
        .add_rpc(Add)
        .add_event(OnHello)
        .add_stream(OnChat)
        .build()
        .await?;
    println!("[server] 监听 {addr}");
    server.run().await
}

// ======================== 客户端 ========================

async fn run_client(addr: String) -> Result<()> {
    let client = ClientBuilder::new().connect(&addr).await?;
    println!("[client] 已连接 {addr}");

    // RPC 调用
    let sum: i64 = client.request("add", &(10, 20)).await?;
    println!("[client] add(10, 20) = {sum}");
    assert_eq!(sum, 30);

    // 发送事件
    client.emit("hello", &"world".to_string()).await?;

    // 推送流数据
    let mut stream = client.create_stream("chat").await?;
    for i in 0..3 {
        stream.send(format!("第 {i} 帧")).await?;
    }
    stream.finish().await?;

    // 等待服务端处理完成
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    client.close();
    println!("[client] 全部完成");
    Ok(())
}

// ======================== 主入口 ========================

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "127.0.0.1:5000".to_string();
    let server_handle = tokio::spawn(async move { run_server(addr).await });

    // 等待服务端就绪
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    if let Err(e) = run_client("127.0.0.1:5000".to_string()).await {
        eprintln!("[client] 出错: {e}");
        std::process::exit(1);
    }

    server_handle.abort();
    Ok(())
}
