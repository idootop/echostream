# echostream-av

EchoStream 音视频流扩展 —— 实时音视频推流 / 接收（record / play 场景）。

## 能力

- **参数协商**：StreamOpen 携带 codec / samplerate / channels / bitrate / width / height / fps / clock-rate
- **帧封装**：载荷 `[flags:u8][pts:u64 LE][编码数据]`，flags bit0 = 关键帧（视频 I 帧 / 音频首包）
- **时间同步**：pts 以 clock-rate 为刻度（如音频 48000、视频 90000 RTP 风格），播放端可据此调度
- **统计**：发送/接收帧数、关键帧数、末帧 pts；结束 trailers 携带统计信息
- **解耦**：只负责封装与传输，编码由上层 codec 产生（H.264 NAL / Opus 包等）

## 用法

```rust
use echostream_av::{AvFrame, AvParams, AvReceiver, AvSender};

// 推流端（record）：30fps 视频，90kHz 时钟
let params = AvParams { codec: "h264".into(), width: 1920, height: 1080, fps: 30, bitrate: 2_000_000, ..Default::default() };
let stream = client.create_stream_with_metadata("video", params.metadata()).await?;
let mut sender = AvSender::open(stream, &params)?;
sender.send_frame(&AvFrame::key(0, nal_idr)).await?;          // 关键帧
sender.send_frame(&AvFrame::new(3000, nal_p)).await?;         // 增量帧（1 帧 = 3000 刻度）
sender.finish().await?;

// 播放端（play）：#[stream("video")] 处理器内
#[stream("video")]
async fn on_video(stream: StreamReceiver) -> Result<()> {
    let mut recv = AvReceiver::open(stream)?;
    while let Some(frame) = recv.recv_frame().await? {
        if frame.key_frame { /* 快进/追帧可从此处开始渲染 */ }
    }
    Ok(())
}
```

## 元数据约定

| 键 | 说明 |
|----|------|
| frame-format | 载荷格式标记（echostream-av/1；不匹配拒绝接收） |
| codec | 编码器（h264 / opus / aac ...） |
| samplerate / channels | 音频采样率 / 声道数（0 = 无音频） |
| bitrate | 比特率（bps） |
| width / height / fps | 视频分辨率 / 帧率（0 = 无视频） |
| clock-rate | 采样时钟率（pts 单位；默认 90000） |

## 测试

```bash
cargo test -p echostream-av
cargo run -p echostream --example av_stream   # 端到端：1 关键帧 + 29 增量帧推流/播放
```
