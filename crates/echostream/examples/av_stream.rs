//! 音视频流推流/接收端到端示例（echostream-av 扩展）
//!
//! 客户端模拟推流（record）：90kHz 时钟，1 个关键帧 + 29 个增量帧（约 1 秒 30fps 视频），
//! 携带 codec/width/height/fps 等参数协商；服务端接收（play）并统计帧数 / 关键帧 / pts。
//!
//! 运行：`cargo run -p echostream --example av_stream`

use std::time::Duration;

use bytes::Bytes;
use echostream::prelude::*;
use echostream_av::{AvFrame, AvParams, AvReceiver, AvSender};

const ADDR: &str = "127.0.0.1:5401";

/// 流处理器：播放端（play）—— 逐帧接收并统计
#[stream("video")]
async fn on_video(stream: StreamReceiver) -> Result<()> {
    let mut recv = AvReceiver::open(stream)?;
    let p = recv.params();
    println!(
        "[server] 视频流协商: codec={} {}x{}@{}fps clock-rate={} bitrate={}",
        p.codec, p.width, p.height, p.fps, p.clock_rate, p.bitrate
    );
    let mut last_pts = 0u64;
    while let Some(frame) = recv.recv_frame().await? {
        if frame.key_frame {
            println!(
                "[server] 关键帧 pts={} ({} bytes)",
                frame.pts,
                frame.data.len()
            );
        }
        last_pts = frame.pts;
    }
    println!(
        "[server] 播放结束: {} 帧（关键帧 {}）末帧 pts={} code={}",
        recv.frames_received(),
        recv.key_frames(),
        last_pts,
        recv.end_code(),
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = std::sync::Arc::new(
        ServerBuilder::new()
            .bind(ADDR)
            .add_stream(OnVideo)
            .build()
            .await?,
    );
    let server_handle = tokio::spawn({
        let s = server.clone();
        async move { s.run().await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let params = AvParams {
        codec: "h264".into(),
        samplerate: 0,
        channels: 0,
        bitrate: 2_000_000,
        width: 1920,
        height: 1080,
        fps: 30,
        clock_rate: 90000,
    };

    let client = ClientBuilder::new().connect(ADDR).await?;
    println!("[client] 已连接，开始推流（模拟 1 秒 30fps 视频）...");

    // 推流端（record）：AV 参数经 metadata 协商
    let stream = client
        .create_stream_with_metadata("video", params.metadata())
        .await?;
    let mut sender = AvSender::open(stream, &params)?;
    // 关键帧（I 帧）→ 29 个增量帧，pts 按 90kHz 时钟递增（1 帧 = 3000 刻度）
    sender
        .send_frame(&AvFrame::key(0, Bytes::from_static(b"IDR-NAL")))
        .await?;
    for i in 1..30 {
        sender
            .send_frame(&AvFrame::new(i * 3000, Bytes::from(format!("P-frame-{i}"))))
            .await?;
    }
    sender.finish().await?;
    println!(
        "[client] 推流完成: {} 帧（关键帧 1 个，末帧 pts={}）",
        sender.frames_sent(),
        29 * 3000,
    );

    tokio::time::sleep(Duration::from_millis(500)).await;
    client.close();
    server.shutdown();
    let _ = server_handle.await;
    println!("[client] 全部完成");
    Ok(())
}
