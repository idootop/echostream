# EchoStream 基准测试

> 本机回环环境（macOS arm64，release 构建），数据用于发布参考与优化跟踪。
> 运行：`cargo run -p echostream --example bench --release`

## 结果（2026-08 基线，事件通道复用 + datagram + 负载矩阵）

### 负载矩阵（短 / 中 / 长）

| 指标 | 64B | 4KiB | 256KiB |
|------|-----|------|--------|
| 顺序 RPC 延迟 | ~55 µs/次 | ~116 µs/次 | ~3.5 ms/次 |
| 并发 RPC 吞吐 | ~53k req/s | ~19k req/s | - |
| 可靠事件吞吐 | ~110k evt/s | ~26k evt/s | - |
| 不可靠事件吞吐（数据报） | ~490k evt/s | ~354k evt/s（1KiB，受 datagram 4096 上限约束） | - |
| 流吞吐 | - | - | ~150 MiB/s（1MiB 帧） |

说明：不可靠事件载荷上限 = 对端通告的 `datagram_receive_buffer_size`（4096）。

## 架构性开销说明

- 每条消息使用独立 QUIC 流（协议设计）：带来隔离与背压，代价是每次 RPC/事件
  的流打开与关闭开销 —— 延迟与吞吐的主要构成
- RPC 每请求：1 次 TLS 会话复用 + 1 条双向流 + 2 次 postcard 编解码
- 事件每事件：1 条单向流 + 1 次编解码（可靠传输）

## 优化方向（未实施，保持轻量优先）

1. **事件走 datagram**（不可靠模式，可选开关）：适合高频可丢事件，吞吐可数量级提升
2. **流多路并行**：单流受拥塞窗口限制，多流可提升大文件传输吞吐
3. **RPC 连接池**：多连接分摊流控窗口，提升并发上限
4. **0-RTT 重连**：已具备基础（TLS 会话恢复由 rustls 处理），可验证
5. 零拷贝路径：帧编解码当前有若干次拷贝，可引入 `postcard` 零拷贝读取

## 复现

```bash
cargo run -p echostream --example bench --release
```
