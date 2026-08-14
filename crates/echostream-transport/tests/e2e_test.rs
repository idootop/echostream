//! 框架级集成测试：QUIC 回环，覆盖 RPC / 事件 / 流 / 中间件 / 断线钩子 / 数据报

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use echostream_core::{
    Client, ClientBuilder, EventHandler, Middleware, RpcHandler, Server, ServerBuilder, Session,
    StreamHandler, StreamReceiver,
};
use echostream_proto::{Error, Message, Result, StatusCode};
use echostream_transport::{ClientBuilderExt, ServerBuilderExt};

// ======================== 测试处理器 ========================

/// add(a, b) -> a + b
struct Add;
#[async_trait]
impl RpcHandler for Add {
    type Req = (i64, i64);
    type Resp = i64;
    fn name(&self) -> &str {
        "add"
    }
    async fn handle(&self, _s: &Session, (a, b): (i64, i64)) -> Result<i64> {
        Ok(a + b)
    }
}

/// 返回错误
struct Fail;
#[async_trait]
impl RpcHandler for Fail {
    type Req = ();
    type Resp = ();
    fn name(&self) -> &str {
        "fail"
    }
    async fn handle(&self, _s: &Session, _: ()) -> Result<()> {
        Err(Error::Rpc(9, "业务失败".into()))
    }
}

/// 读取会话状态
struct WhoAmI;
#[async_trait]
impl RpcHandler for WhoAmI {
    type Req = ();
    type Resp = String;
    fn name(&self) -> &str {
        "whoami"
    }
    async fn handle(&self, s: &Session, _: ()) -> Result<String> {
        Ok(format!(
            "{}:{}",
            s.id(),
            s.get::<String>("role")
                .map(|r| r.to_string())
                .unwrap_or_default()
        ))
    }
}

/// 事件计数
struct Counter(Arc<AtomicU64>);
#[async_trait]
impl EventHandler for Counter {
    type Data = u64;
    fn name(&self) -> &str {
        "tick"
    }
    async fn handle(&self, _s: &Session, data: u64) -> Result<()> {
        self.0.fetch_add(data, Ordering::Relaxed);
        Ok(())
    }
}

/// 流：累加帧字节数
struct SumStream(Arc<AtomicU64>);
#[async_trait]
impl StreamHandler for SumStream {
    fn name(&self) -> &str {
        "sum"
    }
    async fn handle(&self, _s: &Session, mut stream: StreamReceiver) -> Result<()> {
        let mut total = 0u64;
        while let Some(frame) = stream.recv_frame().await? {
            total += frame.data.len() as u64;
        }
        self.0.store(total, Ordering::Relaxed);
        Ok(())
    }
}

// ======================== 工具 ========================

async fn start_server() -> (Server, Arc<AtomicU64>, Arc<AtomicU64>, Arc<AtomicU64>) {
    let events = Arc::new(AtomicU64::new(0));
    let stream_bytes = Arc::new(AtomicU64::new(0));
    let role = Arc::new(AtomicU64::new(0));
    let _ = &role;
    let server = ServerBuilder::new()
        .bind("127.0.0.1:0")
        .add_rpc(Add)
        .add_rpc(Fail)
        .add_rpc(WhoAmI)
        .add_event(Counter(events.clone()))
        .add_stream(SumStream(stream_bytes.clone()))
        .on_connect(|s: &Session| {
            s.set("role", "tester".to_string());
        })
        .build()
        .await
        .expect("服务端启动失败");
    (server, events, stream_bytes, role)
}

fn run_server(server: Arc<Server>) -> tokio::task::JoinHandle<Result<()>> {
    tokio::spawn(async move { server.run().await })
}

// ======================== 测试 ========================

#[tokio::test(flavor = "multi_thread")]
async fn rpc_roundtrip_and_error_propagation() {
    let (server, _, _, _) = start_server().await;
    let addr = server.endpoint_addr().unwrap();
    let handle = run_server(Arc::new(server));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = ClientBuilder::new().connect(addr).await.unwrap();

    // 正常返回
    let sum: i64 = client.request("add", &(20, 22)).await.unwrap();
    assert_eq!(sum, 42);

    // 错误传播（错误码 + 信息）
    let err = client.request::<(), ()>("fail", &()).await.unwrap_err();
    match err {
        Error::Rpc(code, msg) => {
            assert_eq!(code, 9);
            assert!(msg.contains("业务失败"), "错误信息不符: {msg}");
        }
        other => panic!("期望 Rpc 错误，得到 {other:?}"),
    }

    // 未知方法
    let err = client.request::<(), ()>("nope", &()).await.unwrap_err();
    assert!(matches!(err, Error::Rpc(code, _) if code == StatusCode::NOT_FOUND.0));

    client.close();
    handle.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn session_state_and_connect_hook() {
    let (server, _, _, _) = start_server().await;
    let addr = server.endpoint_addr().unwrap();
    let handle = run_server(Arc::new(server));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = ClientBuilder::new().connect(addr).await.unwrap();
    let who: String = client.request("whoami", &()).await.unwrap();
    // on_connect 钩子写入的 role 可在 RPC 中读到
    assert!(who.contains("tester"), "会话状态未生效: {who}");
    client.close();
    handle.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn middleware_blocks_unauthenticated() {
    // 拦截中间件：只放行 name == "add" 的请求
    struct AllowAdd;
    #[async_trait]
    impl Middleware for AllowAdd {
        fn name(&self) -> &str {
            "allow-add"
        }
        async fn on_message(&self, _s: &Session, msg: Message) -> Result<Option<Message>> {
            if matches!(&msg, Message::Request(r) if r.name == "add") {
                Ok(Some(msg))
            } else {
                Ok(None)
            }
        }
    }

    let server = ServerBuilder::new()
        .bind("127.0.0.1:0")
        .add_rpc(Add)
        .add_rpc(Fail)
        .middleware(AllowAdd)
        .build()
        .await
        .unwrap();
    let addr = server.endpoint_addr().unwrap();
    let handle = run_server(Arc::new(server));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = ClientBuilder::new().connect(addr).await.unwrap();

    // 放行的请求成功
    let sum: i64 = client.request("add", &(1, 2)).await.unwrap();
    assert_eq!(sum, 3);

    // 拦截的请求收到 FORBIDDEN
    let err = client.request::<(), ()>("fail", &()).await.unwrap_err();
    match err {
        Error::Rpc(code, _) => assert_eq!(code, StatusCode::FORBIDDEN.0),
        other => panic!("期望 FORBIDDEN，得到 {other:?}"),
    }
    client.close();
    handle.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn events_and_stream_roundtrip() {
    let (server, events, stream_bytes, _) = start_server().await;
    let addr = server.endpoint_addr().unwrap();
    let handle = run_server(Arc::new(server));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = ClientBuilder::new().connect(addr).await.unwrap();

    // 事件通道
    for _ in 0..10 {
        client.emit("tick", &1u64).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(events.load(Ordering::Relaxed), 10, "事件未全部送达");

    // 流
    let mut stream = client.create_stream("sum").await.unwrap();
    for _ in 0..5 {
        stream
            .send_raw(bytes::Bytes::from(vec![0u8; 1024]))
            .await
            .unwrap();
    }
    stream.finish().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        stream_bytes.load(Ordering::Relaxed),
        5 * 1024,
        "流数据不完整"
    );

    client.close();
    handle.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnect_hooks_fire_on_close() {
    // 服务端侧断线钩子
    let disconnected = Arc::new(AtomicU64::new(0));
    let disconnected2 = disconnected.clone();
    let server = ServerBuilder::new()
        .bind("127.0.0.1:0")
        .add_rpc(Add)
        .on_disconnect(move |_: &Session| {
            disconnected2.fetch_add(1, Ordering::Relaxed);
        })
        .build()
        .await
        .unwrap();
    let addr = server.endpoint_addr().unwrap();
    let handle2 = run_server(Arc::new(server));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = ClientBuilder::new().connect(addr).await.unwrap();
    let _: i64 = client.request("add", &(1, 1)).await.unwrap();
    client.close();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        disconnected.load(Ordering::Relaxed),
        1,
        "服务端断线钩子未触发"
    );

    // 客户端主动关闭：客户端侧断线回调不应触发
    let server2 = ServerBuilder::new()
        .bind("127.0.0.1:0")
        .add_rpc(Add)
        .build()
        .await
        .unwrap();
    let addr2 = server2.endpoint_addr().unwrap();
    let handle3 = run_server(Arc::new(server2));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client_hooks = Arc::new(AtomicU64::new(0));
    let c = {
        let hooks = client_hooks.clone();
        ClientBuilder::new()
            .on_disconnect(move |_: &Client| {
                hooks.fetch_add(1, Ordering::Relaxed);
            })
            .connect(addr2)
            .await
            .unwrap()
    };
    c.close();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        client_hooks.load(Ordering::Relaxed),
        0,
        "主动关闭不应触发断开回调"
    );

    handle2.abort();
    handle3.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn server_initiates_rpc_to_client() {
    // 服务端在收到 "ping" 后主动调用客户端 "pong"
    // 服务端收到 "ping" 后主动调用客户端 "add"
    struct Ping;
    #[async_trait]
    impl RpcHandler for Ping {
        type Req = ();
        type Resp = i64;
        fn name(&self) -> &str {
            "ping"
        }
        async fn handle(&self, s: &Session, _: ()) -> Result<i64> {
            // 服务端主动调用客户端注册的 add
            let sum: i64 = s.request("add", &(2, 3)).await?;
            Ok(sum)
        }
    }
    let server = ServerBuilder::new()
        .bind("127.0.0.1:0")
        .add_rpc(Ping)
        .build()
        .await
        .unwrap();
    let addr = server.endpoint_addr().unwrap();
    let handle = run_server(Arc::new(server));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = ClientBuilder::new()
        .add_rpc(Add)
        .connect(addr)
        .await
        .unwrap();

    // 客户端调服务端 ping → 服务端主动调客户端 add → 结果返回客户端
    let sum: i64 = client.request("ping", &()).await.unwrap();
    assert_eq!(sum, 5, "服务端主动调用客户端失败");
    client.close();
    handle.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn datagram_events_are_delivered() {
    let (server, events, _, _) = start_server().await;
    let addr = server.endpoint_addr().unwrap();
    let handle = run_server(Arc::new(server));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = ClientBuilder::new().connect(addr).await.unwrap();
    for _ in 0..50 {
        client
            .emit_unreliable_raw("tick", Bytes::from(postcard::to_allocvec(&1u64).unwrap()))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    // 数据报可能丢失，但本机回环下应全部到达
    let got = events.load(Ordering::Relaxed);
    assert!(got >= 50, "数据报事件丢失: {got}/50");
    client.close();
    handle.abort();
}
