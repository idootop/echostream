//! 插件栈端到端示例：AuthPlugin + LoggingMiddleware + ReconnectPlugin
//!
//! 验证：
//! 1. 未认证会话的消息被 AuthMiddleware 拦截（RPC 超时）
//! 2. 认证后 RPC 正常
//! 3. 服务端重启后客户端自动重连（指数退避）并重新认证
//!
//! 运行：`cargo run -p echostream --example plugin_stack`

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use echostream::prelude::*;
use echostream_middleware_logging::LoggingMiddleware;
use echostream_plugin_auth::{AuthPlugin, authenticate};
use echostream_plugin_reconnect::ReconnectPlugin;
use echostream_plugin_retry::{RetryPolicy, request_with_retry};

const ADDR: &str = "127.0.0.1:5100";
const TOKEN: &str = "my-secret-token";

// ======================== 处理器定义 ========================

/// RPC：add(a, b) -> a + b
#[rpc("add")]
async fn add((a, b): (i64, i64)) -> Result<i64> {
    Ok(a + b)
}

// ======================== 服务端 ========================

async fn run_server() -> Result<Server> {
    let server = ServerBuilder::new()
        .bind(ADDR)
        .add_rpc(Add)
        .plugin(AuthPlugin::new(TOKEN))
        .middleware(LoggingMiddleware::new())
        .build()
        .await?;
    println!("[server] 监听 {ADDR}");
    Ok(server)
}

// ======================== 客户端 ========================

async fn run_client() -> Result<()> {
    // 断线标志：服务端重启后由断开回调置位
    static DISCONNECTED: AtomicBool = AtomicBool::new(false);
    let client = ClientBuilder::new()
        .timeout(Duration::from_secs(2))
        // 自动重连：断线后指数退避重连
        .plugin(ReconnectPlugin::new(ADDR).base_delay(Duration::from_millis(200)))
        // 重连成功后自动重新认证（on_connect 在初始连接与重连成功时均触发）
        .on_connect(|c: &Client| {
            if DISCONNECTED.swap(false, Ordering::Relaxed) {
                let c = c.clone();
                tokio::spawn(async move {
                    loop {
                        if authenticate(&c.session(), TOKEN).await.is_ok() {
                            println!("[client] 已重新认证");
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                });
            }
        })
        .on_disconnect(|_c: &Client| {
            DISCONNECTED.store(true, Ordering::Relaxed);
        })
        .connect(ADDR)
        .await?;
    println!("[client] 已连接 {ADDR}");

    // 1. 未认证：RPC 被拦截（中间件不回复 → 客户端超时）
    let r: Result<i64> = client.request("add", &(1, 2)).await;
    assert!(r.is_err(), "未认证 RPC 应被拦截");
    println!("[client] 未认证 RPC 被拦截 ✓");

    // 2. 认证后 RPC 成功
    authenticate(&client.session(), TOKEN).await?;
    let sum: i64 = client.request("add", &(10, 20)).await?;
    assert_eq!(sum, 30);
    println!("[client] 认证后 add(10, 20) = {sum} ✓");

    // 3. 服务端重启 → 自动重连 + 重新认证
    // 等待断线发生（main 在 4.5s 时重启服务端）
    for _ in 0..100 {
        if DISCONNECTED.load(Ordering::Relaxed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(DISCONNECTED.load(Ordering::Relaxed), "未检测到断线");
    // 断线后立即发起请求：第一次必然失败，retry 插件自动重试
    // 直到重连 + 重新认证完成
    let sum: i64 = request_with_retry(
        &client,
        "add",
        &(3, 4),
        &RetryPolicy::new(50, Duration::from_millis(200)),
    )
    .await
    .expect("自动重连失败");
    assert_eq!(sum, 7);
    let sum: i64 = client.request("add", &(5, 6)).await?;
    assert_eq!(sum, 11);
    println!("[client] 自动重连 + 重新认证成功，add(5, 6) = {sum} ✓");

    client.close();
    println!("[client] 全部完成");
    Ok(())
}

// ======================== 主入口 ========================

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    // 服务端任务（run(&self)，用 Arc 共享）
    let server = std::sync::Arc::new(run_server().await?);
    let server_task = tokio::spawn({
        let s = server.clone();
        async move { s.run().await }
    });

    // 等待服务端就绪
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 客户端任务
    let client_task = tokio::spawn(async move {
        if let Err(e) = run_client().await {
            eprintln!("[client] 出错: {e}");
            std::process::exit(1);
        }
    });

    // 客户端完成前两步后，重启服务端（模拟故障）
    // 注：未认证消息有 2s 认证等待窗口，第 1 步约在 t=2s 完成
    tokio::time::sleep(Duration::from_millis(4800)).await;
    println!("[main] 重启服务端...");
    server.shutdown();
    let _ = server_task.await;
    drop(server); // 释放监听端口
    // UDP 端口释放有延迟，重试绑定
    let mut server2 = None;
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        match run_server().await {
            Ok(s) => {
                server2 = Some(s);
                break;
            }
            Err(e) => println!("[main] 绑定重试中: {e}"),
        }
    }
    let server2 = std::sync::Arc::new(server2.expect("服务端重启失败"));
    let server2_task = tokio::spawn({
        let s = server2.clone();
        async move { s.run().await }
    });

    // 等待客户端全部完成
    let _ = client_task.await;
    server2.shutdown();
    let _ = server2_task.await;
    Ok(())
}
