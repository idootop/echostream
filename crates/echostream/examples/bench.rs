//! EchoStream 基准测试：RPC 延迟 / 并发吞吐 / 流吞吐
//!
//! 运行（release 模式）：`cargo run -p echostream --example bench --release`

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use echostream::prelude::*;

/// RPC：回显（最小负载往返）
#[rpc("echo")]
async fn echo(_session: &Session, data: Vec<u8>) -> Result<Vec<u8>> {
    Ok(data)
}

/// RPC：64 字节往返
#[rpc("echo64")]
async fn echo64(_session: &Session, data: Vec<u8>) -> Result<Vec<u8>> {
    Ok(data)
}

/// 流：计数
#[stream("bench_stream")]
async fn on_stream(_session: &Session, mut stream: StreamReceiver) -> Result<()> {
    let mut count: u64 = 0;
    let mut bytes: u64 = 0;
    while let Some(frame) = stream.recv().await? {
        count += 1;
        bytes += frame.data.len() as u64;
    }
    println!("[server] 流结束: {count} 帧 / {bytes} 字节");
    Ok(())
}

/// 事件计数
static EVENT_COUNT: AtomicU64 = AtomicU64::new(0);

#[event("bench_event")]
async fn on_event(_session: &Session, _data: Vec<u8>) -> Result<()> {
    EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = ServerBuilder::new()
        .bind("127.0.0.1:5999")
        .add_rpc(Echo)
        .add_rpc(Echo64)
        .add_stream(OnStream)
        .add_event(OnEvent)
        .build()
        .await?;
    let server_handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let client = ClientBuilder::new().connect("127.0.0.1:5999").await?;

    // ===== 1. RPC 延迟（顺序 1 万次，64 字节载荷） =====
    let payload = vec![0u8; 64];
    const N: u32 = 10_000;
    let start = Instant::now();
    for _ in 0..N {
        let resp: Vec<u8> = client.request("echo64", &payload).await?;
        debug_assert_eq!(resp.len(), 64);
    }
    let elapsed = start.elapsed();
    let per = elapsed.as_secs_f64() / N as f64 * 1e6;
    println!("[bench] 顺序 RPC 延迟: {per:.1} µs/次（{N} 次，64B 载荷）");

    // ===== 2. RPC 并发吞吐（100 并发 × 1000 次） =====
    const CONCURRENCY: usize = 100;
    const PER_TASK: u32 = 1000;
    let start = Instant::now();
    let mut tasks = Vec::new();
    for _ in 0..CONCURRENCY {
        let client = client.clone();
        let payload = payload.clone();
        tasks.push(tokio::spawn(async move {
            for _ in 0..PER_TASK {
                let _: Vec<u8> = client.request("echo64", &payload).await?;
            }
            Ok::<(), echostream::Error>(())
        }));
    }
    for t in tasks {
        t.await.map_err(|e| Error::Io(e.to_string()))??;
    }
    let elapsed = start.elapsed();
    let total = (CONCURRENCY as u64) * (PER_TASK as u64);
    let qps = total as f64 / elapsed.as_secs_f64();
    println!("[bench] 并发 RPC 吞吐: {qps:.0} req/s（{CONCURRENCY} 并发 × {PER_TASK}，64B 载荷）");

    // ===== 3. 事件吞吐（10 万事件） =====
    const EVENTS: u64 = 100_000;
    let start = Instant::now();
    for _ in 0..EVENTS {
        client.emit("bench_event", &payload).await?;
    }
    let elapsed = start.elapsed();
    let eps = EVENTS as f64 / elapsed.as_secs_f64();
    println!("[bench] 事件吞吐: {eps:.0} evt/s（{EVENTS} 事件，64B）");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    println!("[bench] 服务端收到事件: {}", EVENT_COUNT.load(Ordering::Relaxed));

    // ===== 4. 流吞吐（1MB × 200 帧 = 200MB） =====
    let mut stream = client.create_stream("bench_stream").await?;
    let chunk = vec![1u8; 1024 * 1024];
    const FRAMES: u32 = 200;
    let start = Instant::now();
    for _ in 0..FRAMES {
        stream.send(chunk.clone()).await?;
    }
    stream.finish().await?;
    let elapsed = start.elapsed();
    let mib = (FRAMES as f64 * 1.0) / elapsed.as_secs_f64();
    println!("[bench] 流吞吐: {mib:.1} MiB/s（{FRAMES} × 1MiB 帧）");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    client.close();
    server_handle.abort();
    Ok(())
}
