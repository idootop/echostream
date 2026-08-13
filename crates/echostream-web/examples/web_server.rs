//! WebTransport 服务端示例
//!
//! 启动 WebTransport（HTTP/3）服务端，用 wtransport 客户端模拟浏览器连接，
//! 验证 RPC / Event / Stream 三种模式（与原生 QUIC 客户端同一套协议）。
//!
//! 运行：`cargo run -p echostream-web --example web_server`

use echostream::prelude::*;
use echostream_web::WebServerBuilder;
use wtransport::endpoint::endpoint_side::Client;
use wtransport::{ClientConfig, Endpoint};

/// RPC：加法
#[rpc("add")]
async fn add(_session: &Session, (a, b): (i64, i64)) -> Result<i64> {
    Ok(a + b)
}

/// Event：接收客户端事件
#[event("hello")]
async fn on_hello(session: &Session, data: String) -> Result<()> {
    println!(
        "[server] 收到 WebTransport 事件: {data} <- {}",
        session.peer_addr()
    );
    Ok(())
}

/// Stream：接收客户端流
#[stream("chat")]
async fn on_chat(_session: &Session, mut stream: StreamReceiver) -> Result<()> {
    while let Some(frame) = stream.recv().await? {
        println!(
            "[server] 流帧 #{}: {}",
            frame.seq,
            String::from_utf8_lossy(&frame.data)
        );
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "127.0.0.1:4433";
    let server = WebServerBuilder::new()
        .bind(addr)
        .add_rpc(Add)
        .add_event(OnHello)
        .add_stream(OnChat)
        .build()
        .await?;
    println!("[server] WebTransport 监听 {addr}");
    let server_handle = tokio::spawn(async move { server.run().await });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // ===== wtransport 客户端（模拟浏览器） =====
    let config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .build();
    let endpoint = Endpoint::<Client>::client(config).map_err(|e| Error::Io(e.to_string()))?;
    let conn = endpoint
        .connect(format!("https://{addr}"))
        .await
        .map_err(|e| Error::Io(e.to_string()))?;
    println!("[client] WebTransport 已连接");

    // RPC：请求走双向流
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| Error::Io(e.to_string()))?
        .await
        .map_err(|e| Error::Io(e.to_string()))?;
    let req = echostream::Message::Request(echostream::RequestMsg {
        id: 1,
        name: "add".into(),
        data: echostream::codec::encode(&(10i64, 20i64))?,
    });
    let frame = {
        let payload =
            postcard::to_allocvec(&req).map_err(|e| Error::Serialization(e.to_string()))?;
        let mut buf = Vec::with_capacity(4 + payload.len());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&payload);
        buf
    };
    send.write_all(&frame)
        .await
        .map_err(|e| Error::Io(e.to_string()))?;
    send.finish().await.map_err(|e| Error::Io(e.to_string()))?;

    // 读取响应
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|e| Error::Io(e.to_string()))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf)
        .await
        .map_err(|e| Error::Io(e.to_string()))?;
    let msg: echostream::Message =
        postcard::from_bytes(&buf).map_err(|e| Error::Serialization(e.to_string()))?;
    if let echostream::Message::Response(resp) = msg {
        let sum: i64 =
            postcard::from_bytes(&resp.data).map_err(|e| Error::Serialization(e.to_string()))?;
        println!("[client] add(10, 20) = {sum}");
        assert_eq!(sum, 30);
    } else {
        return Err(Error::Protocol("响应类型错误".into()));
    }

    // Event：发送单向事件
    let mut uni = conn
        .open_uni()
        .await
        .map_err(|e| Error::Io(e.to_string()))?
        .await
        .map_err(|e| Error::Io(e.to_string()))?;
    let event = echostream::Message::Event(echostream::EventMsg {
        id: 2,
        name: "hello".into(),
        data: echostream::codec::encode(&"world".to_string())?,
    });
    let frame = {
        let payload =
            postcard::to_allocvec(&event).map_err(|e| Error::Serialization(e.to_string()))?;
        let mut buf = Vec::with_capacity(4 + payload.len());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&payload);
        buf
    };
    uni.write_all(&frame)
        .await
        .map_err(|e| Error::Io(format!("写事件帧: {e}")))?;
    uni.finish()
        .await
        .map_err(|e| Error::Io(format!("事件流 finish: {e}")))?;

    // 等待服务端处理
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    conn.close(0u32.into(), b"done");
    server_handle.abort();
    println!("全部完成");
    Ok(())
}
