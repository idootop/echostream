//! 处理器签名全形态示例
//!
//! `#[rpc]` / `#[event]` / `#[stream]` 宏自动识别 Session 与数据参数：
//! - Session 参数可写可不写（`session: &Session` / `_session: &Session` / 省略）
//! - 数据参数可写可不写（无参数处理器）
//! - 无参数的 impl 生成下划线参数，无 unused 警告
//!
//! 运行：`cargo run -p echostream --example handler_forms`

use std::time::Duration;

use echostream::prelude::*;

// ======================== RPC：四种签名 ========================

/// 带 Session + 请求参数（需要会话时）
#[rpc("with_session")]
async fn with_session(session: &Session, msg: String) -> Result<String> {
    Ok(format!("{} <- {}", msg, session.peer_addr()))
}

/// 省略 Session，只留请求参数（最常见）
#[rpc("echo")]
async fn echo(msg: String) -> Result<String> {
    Ok(msg)
}

/// 带 Session 但无请求参数
#[rpc("whoami")]
async fn whoami(session: &Session) -> Result<String> {
    Ok(format!("session {}", session.id()))
}

/// 最简形式：无 Session 无参数
#[rpc("ping")]
async fn ping() -> Result<String> {
    Ok("pong".to_string())
}

// ======================== Event：四种签名 ========================

/// 带 Session + 载荷
#[event("ev_with_session")]
async fn ev_with_session(session: &Session, msg: String) -> Result<()> {
    println!("[server] ev_with_session: {msg} <- {}", session.peer_addr());
    Ok(())
}

/// 省略 Session，只留载荷
#[event("ev_payload")]
async fn ev_payload(msg: String) -> Result<()> {
    println!("[server] ev_payload: {msg}");
    Ok(())
}

/// 无载荷事件（带 Session）
#[event("ev_tick_session")]
async fn ev_tick_session(_session: &Session) -> Result<()> {
    println!("[server] ev_tick_session");
    Ok(())
}

/// 无载荷事件（最简）
#[event("ev_tick")]
async fn ev_tick() -> Result<()> {
    println!("[server] ev_tick");
    Ok(())
}

// ======================== Stream：两种签名 ========================

/// 带 Session + StreamReceiver
#[stream("st_with_session")]
async fn st_with_session(session: &Session, stream: StreamReceiver) -> Result<()> {
    use futures::StreamExt;
    println!(
        "[server] 流 st_with_session 开始 <- {}",
        session.peer_addr()
    );
    let frames = stream.into_stream_typed::<String>();
    futures::pin_mut!(frames);
    let mut n = 0;
    while let Some(frame) = frames.next().await {
        println!("[server] st_with_session 帧: {frame:?}");
        n += 1;
    }
    println!("[server] 流 st_with_session 结束（{n} 帧）");
    Ok(())
}

/// 省略 Session
#[stream("st_plain")]
async fn st_plain(stream: StreamReceiver) -> Result<()> {
    use futures::StreamExt;
    println!("[server] 流 st_plain 开始");
    let frames = stream.into_stream_typed::<String>();
    futures::pin_mut!(frames);
    let mut n = 0;
    while let Some(frame) = frames.next().await {
        println!("[server] st_plain 帧: {frame:?}");
        n += 1;
    }
    println!("[server] 流 st_plain 结束（{n} 帧）");
    Ok(())
}

// ======================== 主入口 ========================

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "127.0.0.1:5300";
    let server = std::sync::Arc::new(
        ServerBuilder::new()
            .bind(addr)
            .add_rpc(WithSession)
            .add_rpc(Echo)
            .add_rpc(Whoami)
            .add_rpc(Ping)
            .add_event(EvWithSession)
            .add_event(EvPayload)
            .add_event(EvTickSession)
            .add_event(EvTick)
            .add_stream(StWithSession)
            .add_stream(StPlain)
            .build()
            .await?,
    );
    let server_handle = tokio::spawn({
        let s = server.clone();
        async move { s.run().await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let client = ClientBuilder::new().connect(addr).await?;

    // RPC 四形态
    let r: String = client.request("with_session", &"hi".to_string()).await?;
    assert!(r.starts_with("hi <-"));
    println!("[client] with_session: {r}");

    let r: String = client.request("echo", &"hello".to_string()).await?;
    assert_eq!(r, "hello");
    println!("[client] echo: {r}");

    let r: String = client.request("whoami", &()).await?;
    assert!(r.starts_with("session "));
    println!("[client] whoami: {r}");

    let r: String = client.request("ping", &()).await?;
    assert_eq!(r, "pong");
    println!("[client] ping: {r}");

    // Event 四形态
    client.emit("ev_with_session", &"a".to_string()).await?;
    client.emit("ev_payload", &"b".to_string()).await?;
    client.emit("ev_tick_session", &()).await?;
    client.emit("ev_tick", &()).await?;

    // Stream 两形态
    let mut s1 = client.create_stream("st_with_session").await?;
    s1.send("frame-1").await?;
    s1.finish().await?;
    let mut s2 = client.create_stream("st_plain").await?;
    s2.send("frame-2").await?;
    s2.finish().await?;

    tokio::time::sleep(Duration::from_millis(300)).await;
    client.close();
    server.shutdown();
    let _ = server_handle.await;
    println!("[client] 全部完成");
    Ok(())
}
