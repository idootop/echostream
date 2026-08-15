//! EchoStream 音视频流扩展
//!
//! 基于流三帧协议的实时音视频推流 / 接收（play / record 场景）：
//! - **StreamOpen 协商**：codec / samplerate / channels / bitrate / width / height / fps /
//!   clock-rate（采样时钟，pts 单位）等参数，声明帧载荷格式（frame-format = echostream-av/1）
//! - **数据帧载荷**：`[flags:u8][pts:u64 LE][编码数据]` —— flags bit0 = 关键帧
//!   （视频 I 帧 / 音频首包，可独立解码，接收方可做快进 / 丢帧）
//! - **结束**：finish_with 携带帧数 / 末帧 pts 等 trailers
//!
//! 典型场景：摄像头/麦克风采集推流（record）、播放器逐帧接收渲染（play）、
//! 音视频文件转码管道等。编码数据由上层 codec 产生（如 H.264 NAL、Opus 包），
//! 本扩展只负责封装与传输。
//!
//! ```text
//! // 推流端（record）：30fps 视频，90kHz 时钟
//! let stream = session.create_stream_with_metadata("video", av_metadata(&params)).await?;
//! let mut sender = AvSender::open(stream, &params).await?;
//! sender.send_frame(&AvFrame::key(0, nal_data)).await?;
//! sender.send_frame(&AvFrame::new(3000, p_frame_data)).await?;
//! sender.finish().await?;
//!
//! // 播放端（play）：#[stream("video")] 处理器内
//! let mut recv = AvReceiver::open(stream).await?;
//! while let Some(frame) = recv.recv_frame().await? { /* 渲染 frame */ }
//! ```

use bytes::{BufMut, Bytes};
use echostream_core::{StreamReceiver, StreamSender};
use echostream_proto::{Error, Result, StreamMetaEntry};

/// 帧载荷格式标记（metadata frame-format 的值）
pub const FRAME_FORMAT: &str = "echostream-av/1";
/// 帧载荷格式声明键
pub const KEY_FRAME_FORMAT: &str = "frame-format";
/// 编码器键（如 h264 / opus / aac）
pub const KEY_CODEC: &str = "codec";
/// 音频采样率（Hz；0 = 无音频）
pub const KEY_SAMPLERATE: &str = "samplerate";
/// 声道数
pub const KEY_CHANNELS: &str = "channels";
/// 比特率（bps）
pub const KEY_BITRATE: &str = "bitrate";
/// 视频宽（像素；0 = 无视频）
pub const KEY_WIDTH: &str = "width";
/// 视频高（像素）
pub const KEY_HEIGHT: &str = "height";
/// 帧率（fps）
pub const KEY_FPS: &str = "fps";
/// 采样时钟率（pts 单位；如音频 48000、视频 90000 RTP 风格）
pub const KEY_CLOCK_RATE: &str = "clock-rate";

/// 关键帧标志（帧载荷 flags bit0）
pub const FLAG_KEY_FRAME: u8 = 1 << 0;

/// 音视频流参数（StreamOpen 协商）
#[derive(Debug, Clone, Default)]
pub struct AvParams {
    /// 编码器（h264 / opus / aac / vp9 ...）
    pub codec: String,
    /// 音频采样率（Hz；0 = 无音频）
    pub samplerate: u64,
    /// 声道数
    pub channels: u64,
    /// 比特率（bps）
    pub bitrate: u64,
    /// 视频宽（像素；0 = 无视频）
    pub width: u64,
    /// 视频高（像素）
    pub height: u64,
    /// 帧率（fps）
    pub fps: u64,
    /// 采样时钟率（pts 单位；默认 90000，RTP 风格）
    pub clock_rate: u64,
}

impl AvParams {
    /// 构造 open metadata（供 create_stream_with_metadata 使用）
    pub fn metadata(&self) -> Vec<StreamMetaEntry> {
        let mut meta = vec![
            StreamMetaEntry::str(KEY_FRAME_FORMAT, FRAME_FORMAT),
            StreamMetaEntry::str(KEY_CODEC, self.codec.clone()),
            // 采样时钟率：默认 90000（RTP 风格），音频场景常用 48000
            StreamMetaEntry::num(
                KEY_CLOCK_RATE,
                if self.clock_rate > 0 {
                    self.clock_rate
                } else {
                    90000
                },
            ),
        ];
        if self.samplerate > 0 {
            meta.push(StreamMetaEntry::num(KEY_SAMPLERATE, self.samplerate));
            meta.push(StreamMetaEntry::num(KEY_CHANNELS, self.channels));
        }
        if self.bitrate > 0 {
            meta.push(StreamMetaEntry::num(KEY_BITRATE, self.bitrate));
        }
        if self.width > 0 {
            meta.push(StreamMetaEntry::num(KEY_WIDTH, self.width));
            meta.push(StreamMetaEntry::num(KEY_HEIGHT, self.height));
            meta.push(StreamMetaEntry::num(KEY_FPS, self.fps));
        }
        meta
    }

    /// 从流元数据解析（AvReceiver::open 内部使用）
    pub fn from_metadata(meta: &[StreamMetaEntry]) -> Result<Self> {
        let get = |key: &str| -> Option<String> {
            meta.iter()
                .find(|m| m.key == key)
                .map(|m| String::from_utf8_lossy(&m.value).to_string())
        };
        let get_num = |key: &str| -> u64 { get(key).and_then(|s| s.parse().ok()).unwrap_or(0) };
        let format = get(KEY_FRAME_FORMAT).unwrap_or_default();
        if !format.is_empty() && format != FRAME_FORMAT {
            return Err(Error::Protocol(format!("不支持的帧格式: {format}")));
        }
        Ok(Self {
            codec: get(KEY_CODEC).unwrap_or_default(),
            samplerate: get_num(KEY_SAMPLERATE),
            channels: get_num(KEY_CHANNELS),
            bitrate: get_num(KEY_BITRATE),
            width: get_num(KEY_WIDTH),
            height: get_num(KEY_HEIGHT),
            fps: get_num(KEY_FPS),
            clock_rate: {
                let rate = get_num(KEY_CLOCK_RATE);
                if rate > 0 { rate } else { 90000 }
            },
        })
    }
}

/// 音视频帧（封装后的传输单元）
#[derive(Debug, Clone)]
pub struct AvFrame {
    /// 显示时间戳（clock-rate 刻度；如 90kHz 时钟下 1 秒 = 90000）
    pub pts: u64,
    /// 关键帧（视频 I 帧 / 音频首包；可独立解码）
    pub key_frame: bool,
    /// 编码后数据（如 H.264 NAL 单元 / Opus 包）
    pub data: Bytes,
}

impl AvFrame {
    /// 普通帧
    pub fn new(pts: u64, data: impl Into<Bytes>) -> Self {
        Self {
            pts,
            key_frame: false,
            data: data.into(),
        }
    }

    /// 关键帧（视频 I 帧 / 音频首包）
    pub fn key(pts: u64, data: impl Into<Bytes>) -> Self {
        Self {
            pts,
            key_frame: true,
            data: data.into(),
        }
    }
}

/// 帧载荷编解码：[flags:u8][pts:u64 LE][编码数据]
fn encode_payload(frame: &AvFrame) -> Bytes {
    let mut buf = Vec::with_capacity(9 + frame.data.len());
    buf.push(if frame.key_frame { FLAG_KEY_FRAME } else { 0 });
    buf.put_u64_le(frame.pts);
    buf.extend_from_slice(&frame.data);
    Bytes::from(buf)
}

fn decode_payload(data: &[u8]) -> Result<AvFrame> {
    if data.len() < 9 {
        return Err(Error::Protocol("AV 帧载荷过短".into()));
    }
    let flags = data[0];
    let pts = u64::from_le_bytes(data[1..9].try_into().unwrap());
    Ok(AvFrame {
        pts,
        key_frame: flags & FLAG_KEY_FRAME != 0,
        data: Bytes::copy_from_slice(&data[9..]),
    })
}

/// 推流端（record）：发送音视频帧
pub struct AvSender {
    stream: StreamSender,
    clock_rate: u64,
    frames: u64,
    last_pts: u64,
}

impl AvSender {
    /// 创建推流端（stream 需已携带 AV 参数 metadata，用 AvParams::metadata 创建；
    /// 推荐 Session::create_stream_with_metadata(name, params.metadata())）
    pub fn open(stream: StreamSender, params: &AvParams) -> Result<Self> {
        Ok(Self {
            stream,
            clock_rate: params.clock_rate.max(1),
            frames: 0,
            last_pts: 0,
        })
    }

    /// 发送一帧（自动封装 pts / 关键帧标志）
    pub async fn send_frame(&mut self, frame: &AvFrame) -> Result<()> {
        self.frames += 1;
        self.last_pts = frame.pts;
        self.stream.send_raw(encode_payload(frame)).await
    }

    /// 采样时钟率（pts 单位）
    pub fn clock_rate(&self) -> u64 {
        self.clock_rate
    }

    /// 已发送帧数
    pub fn frames_sent(&self) -> u64 {
        self.frames
    }

    /// 正常结束（trailers：帧数 / 末帧 pts）
    pub async fn finish(&mut self) -> Result<()> {
        self.finish_with(0, None).await
    }

    /// 结束（指定结束码与原因；trailers：帧数 / 末帧 pts）
    pub async fn finish_with(&mut self, code: u16, message: Option<String>) -> Result<()> {
        self.stream
            .finish_with(
                code,
                message,
                vec![
                    StreamMetaEntry::num("frames", self.frames),
                    StreamMetaEntry::num("last-pts", self.last_pts),
                ],
            )
            .await
    }
}

/// 播放端（play）：接收并解析音视频帧
pub struct AvReceiver {
    stream: StreamReceiver,
    params: AvParams,
    frames: u64,
    key_frames: u64,
}

impl AvReceiver {
    /// 创建播放端（解析 StreamOpen 协商的 AV 参数；流名/元数据来自 open）
    pub fn open(stream: StreamReceiver) -> Result<Self> {
        let params = AvParams::from_metadata(stream.metadata())?;
        Ok(Self {
            stream,
            params,
            frames: 0,
            key_frames: 0,
        })
    }

    /// 接收下一帧；流结束返回 Ok(None)
    pub async fn recv_frame(&mut self) -> Result<Option<AvFrame>> {
        let Some(frame) = self.stream.recv_frame().await? else {
            return Ok(None);
        };
        let av = decode_payload(&frame.data)?;
        self.frames += 1;
        if av.key_frame {
            self.key_frames += 1;
        }
        Ok(Some(av))
    }

    /// 协商的音视频参数（codec / 采样率 / 分辨率 / 时钟率）
    pub fn params(&self) -> &AvParams {
        &self.params
    }

    /// 已接收帧数
    pub fn frames_received(&self) -> u64 {
        self.frames
    }

    /// 已接收关键帧数
    pub fn key_frames(&self) -> u64 {
        self.key_frames
    }

    /// 结束码（流结束后有效）
    pub fn end_code(&self) -> u16 {
        self.stream.end_code()
    }

    /// 结束原因（流结束后有效）
    pub fn end_message(&self) -> Option<&str> {
        self.stream.end_message()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_payload_roundtrip() {
        let frame = AvFrame::key(90000, b"nal-unit".to_vec());
        let decoded = decode_payload(&encode_payload(&frame)).unwrap();
        assert_eq!(decoded.pts, 90000);
        assert!(decoded.key_frame);
        assert_eq!(decoded.data.as_ref(), b"nal-unit");

        let frame2 = AvFrame::new(180000, b"p-frame".to_vec());
        let decoded2 = decode_payload(&encode_payload(&frame2)).unwrap();
        assert_eq!(decoded2.pts, 180000);
        assert!(!decoded2.key_frame);
        assert_eq!(decoded2.data.as_ref(), b"p-frame");
    }

    #[test]
    fn params_metadata_roundtrip() {
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
        let meta = params.metadata();
        let parsed = AvParams::from_metadata(&meta).unwrap();
        assert_eq!(parsed.codec, "h264");
        assert_eq!(parsed.width, 1920);
        assert_eq!(parsed.height, 1080);
        assert_eq!(parsed.fps, 30);
        assert_eq!(parsed.clock_rate, 90000);
        assert_eq!(parsed.bitrate, 2_000_000);
    }

    #[test]
    fn audio_params_roundtrip() {
        let params = AvParams {
            codec: "opus".into(),
            samplerate: 48000,
            channels: 2,
            ..Default::default()
        };
        let parsed = AvParams::from_metadata(&params.metadata()).unwrap();
        assert_eq!(parsed.samplerate, 48000);
        assert_eq!(parsed.channels, 2);
        assert_eq!(parsed.clock_rate, 90000, "默认时钟率");
    }

    #[test]
    fn rejects_unknown_frame_format() {
        let meta = vec![StreamMetaEntry::str(KEY_FRAME_FORMAT, "other/1")];
        assert!(AvParams::from_metadata(&meta).is_err());
    }

    #[test]
    fn rejects_short_payload() {
        assert!(decode_payload(&[0u8; 8]).is_err());
    }
}
