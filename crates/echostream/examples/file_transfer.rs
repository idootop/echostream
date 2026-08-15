//! 文件流传输端到端示例（echostream-file 扩展）
//!
//! 生成随机文件 → 客户端分块发送（filename/size/mime 协商 + sha256 校验和 trailers）
//! → 服务端接收写入磁盘并校验内容与校验和。
//!
//! 运行：`cargo run -p echostream --example file_transfer`

use std::time::Duration;

use echostream::prelude::*;
use echostream_file::{FileStreamExt, recv_file};

const ADDR: &str = "127.0.0.1:5400";

/// 流处理器：接收文件并写入目标路径（业务侧一行完成）
#[stream("upload")]
async fn on_upload(_stream: StreamReceiver) -> Result<()> {
    let dest = "/tmp/echostream-received.bin";
    let summary = recv_file(_stream, dest).await?;
    println!(
        "[server] 收到文件 {dest}: {} bytes / {} 帧 / sha256 {}",
        summary.size, summary.frames, summary.checksum
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = std::sync::Arc::new(
        ServerBuilder::new()
            .bind(ADDR)
            .add_stream(OnUpload)
            .build()
            .await?,
    );
    let server_handle = tokio::spawn({
        let s = server.clone();
        async move { s.run().await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 生成 1MiB 确定性源文件（可复现校验）
    let src = "/tmp/echostream-source.bin";
    let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 251) as u8).collect();
    std::fs::write(src, &data).expect("写源文件失败");

    let client = ClientBuilder::new().connect(ADDR).await?;
    println!("[client] 已连接，开始发送文件（1MiB，分块 64KiB）...");

    // 一行创建文件流：自动携带 filename/size/mime 元数据
    let sender = client
        .create_file_stream("upload", src, Some("application/octet-stream".to_string()))
        .await?;
    let summary = sender.send_all().await?;
    println!(
        "[client] 发送完成: {} bytes / {} 帧 / sha256 {}",
        summary.size, summary.frames, summary.checksum
    );

    tokio::time::sleep(Duration::from_millis(500)).await;
    client.close();
    server.shutdown();
    let _ = server_handle.await;

    // 校验：接收内容与源一致 + 校验和一致
    let received = std::fs::read("/tmp/echostream-received.bin").expect("读接收文件失败");
    assert_eq!(received.len(), data.len(), "文件大小不一致");
    assert_eq!(received, data, "文件内容不一致");
    println!("[client] 内容与校验和全部一致 ✓");
    Ok(())
}
