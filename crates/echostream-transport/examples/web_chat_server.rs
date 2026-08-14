//! EchoStream WebTransport 常驻服务端（浏览器 E2E 测试用）
//!
//! 运行：`cargo run -p echostream-transport --example web_chat_server --release --features web`

use echostream::prelude::*;
use echostream_transport::web::WebServerBuilder;

/// RPC：加法
#[rpc("add")]
async fn add(_session: &Session, (a, b): (i64, i64)) -> Result<i64> {
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
    while let Some(text) = stream.recv::<String>().await? {
        count += 1;
        println!("[server] 流帧 #{count}: {text}");
    }
    println!("[server] 流 chat 结束（{count} 帧）");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_target(false)
        .init();
    // 证书：优先用环境变量指定（E2E 的 CA 签发证书），否则自签名（7 天有效期）
    let cert_file =
        std::env::var("ECHO_CERT").unwrap_or_else(|_| "target/web_chat_cert.pem".into());
    let key_file = std::env::var("ECHO_KEY").unwrap_or_else(|_| "target/web_chat_key.pem".into());
    if std::env::var("ECHO_CERT").is_err() && !std::path::Path::new(&cert_file).exists() {
        let mut params =
            rcgen::CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
        params.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(1);
        params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(7);
        // Chrome WebTransport 要求 serverAuth EKU
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
        let key_pair = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key_pair).unwrap();
        std::fs::write(&cert_file, cert.pem()).unwrap();
        std::fs::write(&key_file, key_pair.serialize_pem()).unwrap();
        println!("[server] 已生成 7 天有效期证书");
    }
    let identity = wtransport::Identity::load_pemfiles(&cert_file, &key_file)
        .await
        .unwrap();
    let hash = identity.certificate_chain().as_slice()[0].hash();
    let hash_hex = hash
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    println!("[cert-hash] {hash_hex}");

    let bind = std::env::var("ECHO_BIND").unwrap_or_else(|_| "0.0.0.0:4433".into());
    let server = WebServerBuilder::new()
        .bind(&bind)
        .add_rpc(Add)
        .add_event(OnHello)
        .add_stream(OnChat)
        .identity(identity)
        .build()
        .await?;
    println!("[server] WebTransport 监听 {bind}（常驻）");
    server.run().await
}
