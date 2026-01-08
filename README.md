# EchoStream

> 这是一个实验性项目，是对【AI 为主 + 人工为辅】开发模式的公开实践，对话记录可在 `/history` 目录查看。

**EchoStream** is a high-performance, asynchronous bi-directional RPC and streaming framework for Rust. It is engineered for real-time applications that demand both low-latency control signaling and synchronized media transmission.

## Features

- **⚡ Bi-Directional Multi-Modal RPC**: Handle Requests, Responses, and Events over a single unified connection.
- **🎵 Synchronized Audio Streaming**: Built-in clock synchronization and jitter buffering to align audio frames across distributed nodes.
- **🏎 QUIC-Powered**: Built on `quinn`, leveraging multi-streaming to eliminate Head-of-Line (HoL) blocking between control data and audio streams.
- **🛰 Zero-Conf Discovery**: Instant peer-to-peer discovery via mDNS for local area networks.
- **🦀 Developer Friendly**: Procedural macros for effortless handler registration and minimal boilerplate.

## Quick Start

> **🚧 Active Development**: EchoStream is currently in its early stages. Documentation and crates will be available soon.

## Why EchoStream?

While traditional RPC frameworks are optimized for discrete Request/Response cycles, they often fall short in handling **Isochronous Data**—where timing is as critical as integrity.

EchoStream bridges this gap by treating control signals and audio streams as first-class citizens. By combining the transport benefits of **QUIC** with a custom **Time-Sync** Protocol, it ensures that audio frames remain synchronized across the network while maintaining low-latency command execution.

## License

MIT License © 2026-PRESENT [Del Wang](https://del.wang)
