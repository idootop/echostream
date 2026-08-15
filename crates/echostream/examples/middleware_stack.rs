//! 中间件栈端到端示例：timeout + error + transform + metrics + heartbeat + logging
//!
//! 验证：
//! 1. TimeoutMiddleware：慢 RPC 超时回 TIMEOUT 错误
//! 2. ErrorMiddleware：业务错误归一化为错误响应
//! 3. MetricsPlugin：快照 RPC 可查询调用统计
//! 4. HeartbeatPlugin：客户端周期心跳，服务端在线
//!
//! 运行：`cargo run -p echostream --example middleware_stack`

use std::time::Duration;

use echostream::prelude::*;
use echostream_middleware_error::ErrorMiddleware;
use echostream_middleware_logging::LoggingMiddleware;
use echostream_middleware_timeout::TimeoutMiddleware;
use echostream_middleware_transform::TransformMiddleware;
use echostream_plugin_heartbeat::{HeartbeatClientPlugin, HeartbeatServerPlugin};
use echostream_plugin_metrics::MetricsSnapshot;

const ADDR: &str = "127.0.0.1:5200";

// ======================== 处理器定义 ========================

/// RPC：add(a, b) -> a + b
#[rpc("add")]
async fn add((a, b): (i64, i64)) -> Result<i64> {
    Ok(a + b)
}

/// RPC：慢处理（触发超时中间件）
#[rpc("slow")]
async fn slow(ms: u64) -> Result<String> {
    tokio::time::sleep(Duration::from_millis(ms)).await;
    Ok("done".to_string())
}

/// RPC：业务错误（触发错误归一化中间件）
#[rpc("boom")]
async fn boom(_: ()) -> Result<()> {
    Err(Error::Rpc(7, "业务爆炸".to_string()))
}

// ======================== 服务端 ========================

async fn run_server() -> Result<Server> {
    let server = ServerBuilder::new()
        .bind(ADDR)
        .add_rpc(Add)
        .add_rpc(Slow)
        .add_rpc(Boom)
        // 洋葱链（外层 → 内层）：logging → transform → timeout → error → metrics → 处理器
        .middleware(LoggingMiddleware::new())
        // 数据转换：请求载荷剥掉标记字节（配合对端约定；此处演示透传）
        .middleware(TransformMiddleware::new().map_request(|data| {
            Ok(if data.first() == Some(&0xEE) {
                bytes::Bytes::copy_from_slice(&data[1..])
            } else {
                data
            })
        }))
        // 超时控制：整条链（含处理器）上限 500ms
        .middleware(TimeoutMiddleware::new(Duration::from_millis(500)))
        // 错误归一化：下游错误统一转为错误响应
        .middleware(ErrorMiddleware::new())
        // 指标：请求统计 + 快照 RPC
        .plugin(echostream_plugin_metrics::MetricsPlugin::new())
        // 心跳：30s 未心跳断开（演示用大阈值，不影响测试）
        .plugin(HeartbeatServerPlugin::new(Duration::from_secs(30)))
        .build()
        .await?;
    println!("[server] 监听 {ADDR}");
    Ok(server)
}

// ======================== 客户端 ========================

async fn run_client() -> Result<()> {
    let client = ClientBuilder::new()
        .timeout(Duration::from_secs(3))
        .plugin(HeartbeatClientPlugin::new(Duration::from_secs(5)))
        .connect(ADDR)
        .await?;
    println!("[client] 已连接 {ADDR}");

    // 1. 正常 RPC（标记字节协议：客户端发送 0xEE + 载荷）
    let mut tagged = vec![0xEEu8];
    tagged.extend_from_slice(&postcard::to_allocvec(&(10i64, 20i64)).unwrap());
    let resp = client
        .request_raw("add", bytes::Bytes::from(tagged))
        .await?;
    let sum: i64 = postcard::from_bytes(&resp).map_err(|e| Error::Serialization(e.to_string()))?;
    assert_eq!(sum, 30);
    println!("[client] add(10, 20) = {sum} ✓");

    // 2. 超时：慢 RPC 被 TimeoutMiddleware 拦截（TIMEOUT 错误）
    let r: Result<String> = client.request("slow", &2000u64).await;
    assert!(r.is_err(), "慢 RPC 应超时");
    println!("[client] slow 超时被拦截 ✓ ({})", r.unwrap_err());

    // 3. 错误归一化：业务错误透传错误码与消息
    let r: Result<()> = client.request("boom", &()).await;
    match r {
        Err(Error::Rpc(code, msg)) => {
            assert_eq!(code, 7);
            println!("[client] boom 错误归一化 ✓ (code={code} msg={msg})");
        }
        other => panic!("期望业务错误码，得到 {other:?}"),
    }

    // 4. 指标快照：add 的调用统计
    let snapshot: MetricsSnapshot = client.request("metrics.snapshot", &()).await?;
    let add_stat = snapshot.rpc.iter().find(|(name, _)| name == "add");
    assert!(add_stat.is_some(), "快照应包含 add 统计");
    let (_, stats) = add_stat.unwrap();
    assert!(stats.calls >= 1, "add 调用数应 >= 1");
    assert!(stats.latency_us_sum > 0, "add 延迟统计应 > 0");
    println!(
        "[client] 指标快照 ✓ (add calls={} events={} streams={} sessions={})",
        stats.calls, snapshot.events_total, snapshot.streams_total, snapshot.active_sessions
    );

    // 5. 心跳：等待两个心跳周期，连接仍在线
    tokio::time::sleep(Duration::from_secs(12)).await;
    assert!(client.is_connected(), "心跳保活后连接应在线");
    println!("[client] 心跳保活 ✓ (12s 后连接在线)");

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

    let server = std::sync::Arc::new(run_server().await?);
    let server_task = tokio::spawn({
        let s = server.clone();
        async move { s.run().await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    if let Err(e) = run_client().await {
        eprintln!("[client] 出错: {e}");
        server.shutdown();
        let _ = server_task.await;
        std::process::exit(1);
    }

    server.shutdown();
    let _ = server_task.await;
    Ok(())
}
