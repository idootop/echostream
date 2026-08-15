//! EchoStream 文件流传输扩展
//!
//! 基于流三帧协议（StreamOpen 协商 → 数据帧分块 → StreamEnd trailers 校验）：
//! - **发送**：FileSender 分块读取文件（默认 64KiB），open 携带 filename/size/mime 元数据，
//!   结束 trailers 携带 sha256 校验和 / 帧数 / 实际字节数
//! - **接收**：recv_file 写入本地文件 / recv_to_memory 收集内存，自动验证校验和与大小
//!
//! 适用于：文件上传下载、大文件传输、备份同步等场景。
//!
//! ```text
//! // 发送端
//! let stream = session.create_stream_with_metadata("file", FileSender::meta(&path, None)).await?;
//! FileSender::open(stream, &path, None).await?.send_all().await?;
//!
//! // 接收端（#[stream("file")] handler 内）
//! let summary = recv_file(stream, "/tmp/out.bin").await?;
//! ```

use std::path::Path;

use bytes::Bytes;
use echostream_core::{StreamReceiver, StreamSender};
use echostream_proto::{Error, Result, StreamMetaEntry};
use sha2::{Digest, Sha256};

/// open metadata：文件名
pub const KEY_FILENAME: &str = "filename";
/// open metadata：文件大小（字节）
pub const KEY_SIZE: &str = "size";
/// open metadata：MIME 类型（可选）
pub const KEY_MIME: &str = "mime";
/// trailers：sha256 校验和（hex）
pub const KEY_CHECKSUM: &str = "checksum";
/// trailers：数据帧数
pub const KEY_FRAMES: &str = "frames";
/// trailers：实际发送字节数
pub const KEY_BYTES: &str = "bytes";

/// 默认分块大小（64KiB，QUIC 流窗口友好）
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;

/// 文件传输摘要（发送完成 / 接收完成后获得）
#[derive(Debug, Clone)]
pub struct FileSummary {
    /// 文件名
    pub filename: String,
    /// 文件大小（字节）
    pub size: u64,
    /// 数据帧数
    pub frames: u64,
    /// sha256 校验和（hex 小写）
    pub checksum: String,
}

/// 发送端：分块读取本地文件，经流发送到对端
pub struct FileSender {
    stream: StreamSender,
    file: Option<tokio::fs::File>,
    chunk_size: usize,
    sent: u64,
    frames: u64,
    hasher: Sha256,
}

impl FileSender {
    /// 构造 open metadata（filename/size/mime；供 create_stream_with_metadata 使用）
    pub async fn meta(
        path: impl AsRef<Path>,
        mime: Option<String>,
    ) -> Result<Vec<StreamMetaEntry>> {
        let path = path.as_ref();
        let size = tokio::fs::metadata(path)
            .await
            .map_err(|e| Error::Io(e.to_string()))?
            .len();
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let mut meta = vec![
            StreamMetaEntry::str(KEY_FILENAME, &filename),
            StreamMetaEntry::num(KEY_SIZE, size),
        ];
        if let Some(mime) = mime {
            meta.push(StreamMetaEntry::str(KEY_MIME, mime));
        }
        Ok(meta)
    }

    /// 打开本地文件并准备发送
    ///
    /// stream 需已携带文件元数据（filename/size/mime）—— 推荐用 FileStreamExt::create_file_stream
    /// 一步创建；首帧 StreamOpen 在首次 send 时自动发出。
    pub async fn open(stream: StreamSender, path: impl AsRef<Path>) -> Result<Self> {
        let file = tokio::fs::File::open(path.as_ref())
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
        Ok(Self {
            stream,
            file: Some(file),
            chunk_size: DEFAULT_CHUNK_SIZE,
            sent: 0,
            frames: 0,
            hasher: Sha256::new(),
        })
    }

    /// 自定义分块大小（默认 64KiB）
    pub fn chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size.max(1);
        self
    }

    /// 发送下一块；返回 false 表示文件已全部发送（未调用结束帧）
    pub async fn send_next(&mut self) -> Result<bool> {
        use tokio::io::AsyncReadExt;
        let Some(file) = self.file.as_mut() else {
            return Ok(false);
        };
        let mut buf = vec![0u8; self.chunk_size];
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
        if n == 0 {
            self.file = None;
            return Ok(false);
        }
        buf.truncate(n);
        self.hasher.update(&buf);
        self.sent += n as u64;
        self.frames += 1;
        self.stream.send_raw(Bytes::from(buf)).await?;
        Ok(true)
    }

    /// 发送全部并正常结束（trailers：sha256 校验和 / 帧数 / 字节数）
    pub async fn send_all(mut self) -> Result<FileSummary> {
        while self.send_next().await? {}
        let checksum = hex(&self.hasher.clone().finalize());
        let summary = FileSummary {
            filename: self.stream.name().to_string(),
            size: self.sent,
            frames: self.frames,
            checksum: checksum.clone(),
        };
        self.stream
            .finish_with(
                0,
                None,
                vec![
                    StreamMetaEntry::str(KEY_CHECKSUM, checksum),
                    StreamMetaEntry::num(KEY_FRAMES, self.frames),
                    StreamMetaEntry::num(KEY_BYTES, self.sent),
                ],
            )
            .await?;
        Ok(summary)
    }
}

/// 接收端：从流接收文件并写入本地路径（自动校验大小与 sha256）
pub async fn recv_file(stream: StreamReceiver, dest: impl AsRef<Path>) -> Result<FileSummary> {
    let dest = dest.as_ref();
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| Error::Io(e.to_string()))?;
    let mut summary = recv_into(&mut file, stream).await?;
    if let Some(name) = summary.filename.rsplit('/').next() {
        summary.filename = name.to_string();
    }
    Ok(summary)
}

/// 接收端：从流接收文件到内存（返回字节与摘要）
pub async fn recv_to_memory(stream: StreamReceiver) -> Result<(Vec<u8>, FileSummary)> {
    let mut buf: Vec<u8> = Vec::new();
    let summary = recv_into(&mut buf, stream).await?;
    Ok((buf, summary))
}

/// 通用接收逻辑：逐帧写入目标（文件或内存），结束时校验 trailers
async fn recv_into<W: tokio::io::AsyncWriteExt + Unpin>(
    writer: &mut W,
    mut stream: StreamReceiver,
) -> Result<FileSummary> {
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut frames = 0u64;
    while let Some(frame) = stream.recv_frame().await? {
        hasher.update(&frame.data);
        received += frame.data.len() as u64;
        frames += 1;
        writer
            .write_all(&frame.data)
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
    }
    // 结束信息（trailers）
    let filename = stream.get_metadata(KEY_FILENAME).unwrap_or_default();
    let expected_size = stream
        .get_metadata(KEY_SIZE)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(received);
    let expected_checksum = stream
        .end_metadata()
        .iter()
        .find(|m| m.key == KEY_CHECKSUM)
        .map(|m| String::from_utf8_lossy(&m.value).to_string());
    let checksum = hex(&hasher.finalize());
    // 校验
    if received != expected_size {
        return Err(Error::Protocol(format!(
            "文件大小不符: 期望 {expected_size}，实际 {received}"
        )));
    }
    if let Some(expected) = expected_checksum
        && checksum != expected
    {
        return Err(Error::Protocol(format!(
            "文件校验和不符: 期望 {expected}，实际 {checksum}"
        )));
    }
    Ok(FileSummary {
        filename,
        size: received,
        frames,
        checksum,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 文件流便捷扩展：一行创建带文件元数据的流
///
/// ```text
/// use echostream_file::FileStreamExt;
/// let sender = session.create_file_stream("upload", "/tmp/a.bin", None).await?;
/// sender.send_all().await?;
/// ```
pub trait FileStreamExt {
    /// 创建文件传输流（自动携带 filename/size/mime 元数据协商）
    fn create_file_stream(
        &self,
        name: &str,
        path: impl AsRef<Path> + Send,
        mime: Option<String>,
    ) -> impl std::future::Future<Output = Result<FileSender>> + Send;
}

impl FileStreamExt for echostream_core::Session {
    async fn create_file_stream(
        &self,
        name: &str,
        path: impl AsRef<Path> + Send,
        mime: Option<String>,
    ) -> Result<FileSender> {
        let meta = FileSender::meta(path.as_ref(), mime).await?;
        let stream = self.create_stream_with_metadata(name, meta).await?;
        FileSender::open(stream, path.as_ref()).await
    }
}

impl FileStreamExt for echostream_core::Client {
    async fn create_file_stream(
        &self,
        name: &str,
        path: impl AsRef<Path> + Send,
        mime: Option<String>,
    ) -> Result<FileSender> {
        let meta = FileSender::meta(path.as_ref(), mime).await?;
        let stream = self.create_stream_with_metadata(name, meta).await?;
        FileSender::open(stream, path.as_ref()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use echostream_core::FrameIo;
    use echostream_proto::{Message, StreamMsg, StreamOpenMsg, Timestamp};

    /// 内存流：按序重放预置帧（发送端帧序列回放）
    struct FakeIo {
        frames: std::vec::IntoIter<Message>,
    }

    #[async_trait]
    impl FrameIo for FakeIo {
        async fn write_message(&mut self, _msg: &Message) -> Result<()> {
            Err(Error::Protocol("只读".into()))
        }
        async fn read_message(&mut self) -> Result<Option<Message>> {
            Ok(self.frames.next())
        }
        async fn finish(&mut self) -> Result<()> {
            Ok(())
        }
    }

    /// 构造接收端：first 为首个数据帧；FakeIo 提供 StreamOpen + 后续帧 + 结束 trailers
    fn receiver_with(
        first_data: &[u8],
        open_meta: Vec<StreamMetaEntry>,
        following: Vec<Message>,
    ) -> StreamReceiver {
        let first = StreamMsg {
            id: 7,
            seq: 0,
            sender_ts: Timestamp(0),
            data: Bytes::copy_from_slice(first_data),
        };
        let mut all = vec![Message::StreamOpen(StreamOpenMsg {
            id: 7,
            name: "file".into(),
            metadata: open_meta.clone(),
        })];
        all.extend(following);
        let io = Box::new(FakeIo {
            frames: all.into_iter(),
        });
        // 模拟 dispatch 行为：open metadata 注入 receiver
        StreamReceiver::new(io, first, open_meta)
    }

    fn checksum_meta(data: &[u8]) -> Vec<StreamMetaEntry> {
        let mut h = Sha256::new();
        h.update(data);
        vec![
            StreamMetaEntry::str(KEY_CHECKSUM, hex(&h.finalize())),
            StreamMetaEntry::num(KEY_FRAMES, 1),
            StreamMetaEntry::num(KEY_BYTES, data.len() as u64),
        ]
    }

    fn end_with(id: u64, meta: Vec<StreamMetaEntry>) -> Message {
        Message::StreamEnd(echostream_proto::StreamEndMsg {
            id,
            code: 0,
            message: None,
            metadata: meta,
        })
    }

    #[tokio::test]
    async fn recv_to_memory_verifies_checksum_and_size() {
        let payload = b"hello file stream".to_vec();
        let recv = receiver_with(
            &payload,
            vec![
                StreamMetaEntry::str(KEY_FILENAME, "a.bin"),
                StreamMetaEntry::num(KEY_SIZE, payload.len() as u64),
            ],
            vec![end_with(7, checksum_meta(&payload))],
        );
        let (buf, summary) = recv_to_memory(recv).await.unwrap();
        assert_eq!(buf, payload);
        assert_eq!(summary.filename, "a.bin");
        assert_eq!(summary.size, payload.len() as u64);
        assert_eq!(summary.frames, 1);
    }

    #[tokio::test]
    async fn recv_rejects_tampered_checksum() {
        let payload = b"original bytes".to_vec();
        // 篡改：校验和与实际内容不符
        let meta = checksum_meta(b"tampered!");
        let recv = receiver_with(
            &payload,
            vec![StreamMetaEntry::num(KEY_SIZE, payload.len() as u64)],
            vec![end_with(7, meta)],
        );
        let err = recv_to_memory(recv).await.unwrap_err();
        assert!(err.to_string().contains("校验和不符"), "错误: {err}");
    }

    #[tokio::test]
    async fn recv_rejects_size_mismatch() {
        let payload = b"short".to_vec();
        let recv = receiver_with(
            &payload,
            vec![StreamMetaEntry::num(KEY_SIZE, 999)],
            vec![end_with(7, checksum_meta(&payload))],
        );
        let err = recv_to_memory(recv).await.unwrap_err();
        assert!(err.to_string().contains("大小不符"), "错误: {err}");
    }
}
