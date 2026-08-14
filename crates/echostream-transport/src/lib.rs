//! EchoStream 传输层
//!
//! 实现框架的传输抽象（proto 的 Endpoint / FrameIo / Listener）：
//! - QUIC（feature = "quic"，默认开启）：quinn 封装 + 自签证书 + 便捷 bind/connect
//! - WebSocket（feature = "ws"）：局域网 Web 端零证书服务端
//! - WebTransport（feature = "web"）：公网浏览器服务端
//!
//! 所有传输帧协议一致（长度前缀 + postcard Message），上层框架（echostream-core）
//! 完全传输无关，通过 listener()/from_endpoint() 注入。

#[cfg(feature = "quic")]
pub mod quic;

#[cfg(feature = "ws")]
pub mod ws;

#[cfg(feature = "web")]
pub mod web;

// ======================== QUIC 便捷 API ========================

#[cfg(feature = "quic")]
mod ext {
    use std::net::ToSocketAddrs;
    use std::sync::Arc;

    use echostream_core::Listener;
    use echostream_core::{ClientBuilder, ServerBuilder};
    use echostream_proto::Result;

    use crate::quic::{QuicEndpoint, connect};

    /// ServerBuilder 的 QUIC 便捷扩展：build/serve 时自动创建 QUIC 监听器
    pub trait ServerBuilderExt {
        /// 使用 QUIC 监听器（自动生成自签名证书）
        fn bind(self, addr: impl Into<String>) -> Self;
    }

    impl ServerBuilderExt for ServerBuilder {
        fn bind(self, addr: impl Into<String>) -> Self {
            let addr = addr.into();
            self.listener_factory(Arc::new(move || {
                let addr = addr.clone();
                Box::pin(async move {
                    Ok(Arc::new(QuicEndpoint::bind(&addr).await?) as Arc<dyn Listener>)
                })
            }))
        }
    }

    /// ClientBuilder 的 QUIC 便捷扩展
    #[async_trait::async_trait]
    pub trait ClientBuilderExt {
        /// 使用 QUIC 连接到服务端（开发模式：跳过证书验证；按连接池大小建立连接）
        async fn connect(
            self,
            addr: impl ToSocketAddrs + Send + 'async_trait,
        ) -> Result<echostream_core::Client>;
    }

    #[async_trait::async_trait]
    impl ClientBuilderExt for ClientBuilder {
        async fn connect(
            self,
            addr: impl ToSocketAddrs + Send + 'async_trait,
        ) -> Result<echostream_core::Client> {
            let addr = addr
                .to_socket_addrs()
                .map_err(|e| echostream_proto::Error::Io(e.to_string()))?
                .next()
                .ok_or_else(|| {
                    echostream_proto::Error::InvalidParameter("无法解析服务端地址".into())
                })?;
            let mut conns: Vec<Arc<dyn echostream_core::Endpoint>> =
                Vec::with_capacity(self.pool_size());
            for _ in 0..self.pool_size() {
                conns.push(Arc::new(connect(addr).await?));
            }
            Ok(self.from_endpoints(conns))
        }
    }
}

#[cfg(feature = "quic")]
pub use ext::{ClientBuilderExt, ServerBuilderExt};
