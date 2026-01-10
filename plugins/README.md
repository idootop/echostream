# EchoStream 插件

这里存放 EchoStream 的核心插件集合和第三方插件示例。

## 核心插件规划

- **discovery**：局域网服务发现
- **auth** - Token 认证插件
- **logging** - 结构化日志
- **transform** - 传输数据编解码
- **metrics** - 请求统计、性能指标收集
- **tracing** - 链路追踪，性能分析

## 使用说明

每个插件都是一个独立的 Rust 项目，可以：

1. 直接集成到你的项目中
2. 作为参考实现开发自定义插件
3. 发布为独立的 crate 供他人使用

## 开发指南

创建新插件时，建议遵循以下结构：

```
plugin-name/
├── Cargo.toml
├── src/
│   └── lib.rs
├── examples/
│   └── basic.rs
└── README.md
```

插件应该：

- 接口简洁，开箱即用
- 零依赖或最小化依赖
- 提供清晰的使用示例
- 性能优先，避免不必要的开销
