//! EchoStream WebSocket 服务端示例
//!
//! 局域网 Web 端零证书通信：浏览器 ws:// 直连，帧协议与 QUIC 完全一致。
//!
//! 运行：`cargo run -p echostream-ws --example ws_chat_server`

use echostream::prelude::*;
use echostream_ws::WsServerBuilder;

/// RPC：加法
#[rpc("add")]
async fn add(_session: &Session, (a, b): (u64, u64)) -> Result<u64> {
    Ok(a + b)
}

/// 事件：客户端消息
#[event("hello")]
async fn on_hello(session: &Session, data: String) -> Result<()> {
    println!("[server] 收到事件 hello: {data} <- {}", session.peer_addr());
    Ok(())
}

/// 流：聊天流
#[stream("chat")]
async fn on_chat(_session: &Session, mut stream: StreamReceiver) -> Result<()> {
    let mut count = 0u64;
    while let Some(frame) = stream.recv().await? {
        count += 1;
        println!(
            "[server] 流帧 #{count}: {}",
            String::from_utf8_lossy(&frame.data)
        );
    }
    println!("[server] 流 chat 结束（{count} 帧）");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = WsServerBuilder::new()
        .bind("0.0.0.0:8081")
        .add_rpc(Add)
        .add_event(OnHello)
        .add_stream(OnChat)
        .on_connect(|s| println!("[server] 浏览器连接: {}", s.peer_addr()))
        .on_disconnect(|s| println!("[server] 浏览器断开: {}", s.peer_addr()))
        .build()
        .await?;
    println!("[server] WebSocket 监听 ws://0.0.0.0:8081（常驻）");
    server.run().await
}
