# echostream-file

EchoStream 文件流传输扩展 —— 基于流三帧协议的分块文件传输（上传 / 下载 / 大文件同步）。

## 能力

- **元数据协商**：StreamOpen 携带 filename / size / mime
- **分块发送**：默认 64KiB 分块（可配置），QUIC 流窗口友好
- **完整性校验**：StreamEnd trailers 携带 sha256 校验和 / 帧数 / 字节数，接收端自动验证
- **双接收模式**：写入本地文件（recv_file）或收集内存（recv_to_memory）

## 用法

```rust
use echostream_file::{FileStreamExt, recv_file};

// 发送端（一行创建：自动携带 filename/size/mime 元数据）
let sender = client.create_file_stream("upload", "/tmp/a.bin", Some("application/octet-stream".into())).await?;
let summary = sender.send_all().await?;
println!("{} bytes / {} 帧 / sha256 {}", summary.size, summary.frames, summary.checksum);

// 接收端（#[stream("upload")] 处理器内）
#[stream("upload")]
async fn on_upload(stream: StreamReceiver) -> Result<()> {
    let summary = recv_file(stream, "/tmp/out.bin").await?; // 自动校验大小与校验和
    Ok(())
}
```

## 元数据约定

| 键 | 位置 | 说明 |
|----|------|------|
| filename | open | 文件名 |
| size | open | 文件大小（字节） |
| mime | open | MIME 类型（可选） |
| checksum | trailers | sha256 校验和（hex） |
| frames / bytes | trailers | 数据帧数 / 实际字节数 |

## 测试

```bash
cargo test -p echostream-file
cargo run -p echostream --example file_transfer   # 端到端：1MiB 文件往返 + 校验和一致
```
