//! EchoStream 跨端 E2E 矩阵：通用对端（server / client 双角色）
//!
//! 与 Node / Python 绑定交叉组合，验证四端线缆格式（长度前缀 + postcard）跨端一致。
//!
//! 线缆格式约定（与核心一致）：
//! - RPC 载荷 = postcard 编码（varint）
//! - 事件载荷 = postcard 编码的 String
//! - 流帧载荷 = 原始 UTF-8 字节
//!
//! server 模式：注册 add RPC / hello 事件 / chat 流，打印 E2E_SERVER_READY 后运行。
//! client 模式：调用 add(10,20)、发送 hello 事件、推送 chat 流 3 帧后退出。
//!
//! 用法：
//!   cargo run -p echostream --release --example e2e_peer -- --server --addr 127.0.0.1:5110
//!   cargo run -p echostream --release --example e2e_peer -- --client --addr 127.0.0.1:5110

use echostream::prelude::*;

/// 运行模式
enum Mode {
    Server,
    Client,
}

// ======================== 处理器定义（声明式宏） ========================

/// RPC：add(a, b) -> a + b
#[rpc("add")]
async fn add(_session: &Session, (a, b): (i64, i64)) -> Result<i64> {
    println!("E2E_RPC add({a}, {b})");
    Ok(a + b)
}

/// Event：接收客户端 hello 事件（载荷为 postcard 编码的 String）
#[event("hello")]
async fn on_hello(_session: &Session, data: String) -> Result<()> {
    println!("E2E_EVENT_RECEIVED: {data:?}");
    Ok(())
}

/// Stream：接收客户端 chat 流（帧载荷为原始 UTF-8 字节）
#[stream("chat")]
async fn on_chat(_session: &Session, mut stream: StreamReceiver) -> Result<()> {
    let mut n = 0usize;
    while let Some(text) = stream.recv::<String>().await? {
        println!("E2E_STREAM_FRAME {n}: {text}");
        n += 1;
    }
    println!("E2E_STREAM_FRAMES={n}");
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
    // 就绪标记：build 已完成端口绑定，客户端可安全连接
    println!("E2E_SERVER_READY {addr}");
    server.run().await
}

// ======================== 客户端 ========================

async fn run_client(addr: String) -> Result<()> {
    let client = ClientBuilder::new().connect(&addr).await?;
    println!("[client] 已连接 {addr}");

    // RPC：add(10, 20) = 30
    let sum: i64 = client.request("add", &(10_i64, 20_i64)).await?;
    println!("add(10, 20) = {sum}");
    assert_eq!(sum, 30, "add 结果应为 30");

    // 事件：hello（postcard 编码的 String）
    client
        .emit("hello", &"hello from rust client".to_string())
        .await?;

    // 流：chat 3 帧（原始 UTF-8 字节）
    let mut stream = client.create_stream("chat").await?;
    for i in 0..3 {
        stream.send(format!("rust frame {i}")).await?;
    }
    stream.finish().await?;

    // 等服务端处理完事件与流帧
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    client.close();
    println!("E2E_CLIENT_DONE");
    Ok(())
}

// ======================== 命令行解析（无外部依赖） ========================

fn parse_args() -> (Mode, String) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mode = Mode::Client;
    let mut addr = "127.0.0.1:5110".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--server" => mode = Mode::Server,
            "--client" => mode = Mode::Client,
            "--addr" => {
                i += 1;
                addr = args
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| panic!("--addr 缺少参数"));
            }
            other => panic!("未知参数: {other}"),
        }
        i += 1;
    }
    (mode, addr)
}

// ======================== 主入口 ========================

#[tokio::main]
async fn main() -> Result<()> {
    let (mode, addr) = parse_args();
    match mode {
        Mode::Server => run_server(addr).await,
        Mode::Client => {
            if let Err(e) = run_client(addr).await {
                eprintln!("[client] 出错: {e}");
                std::process::exit(1);
            }
            Ok(())
        }
    }
}
