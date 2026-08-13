//! EchoStream Node.js binding（napi-rs）
//!
//! 载荷约定：所有 RPC/Event/Stream 载荷均为 postcard 编码字节（Buffer），
//! 与 Rust 侧线缆格式一致，Node 端可复用 `sdk/web/postcard.js` 编解码。

use std::sync::Arc;

use bytes::Bytes;
use echostream::prelude::*;
use napi::bindgen_prelude::*;
use napi::threadsafe_function::ThreadsafeFunction;
use napi_derive::napi;

fn to_napi_err(e: echostream::Error) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

/// 连接服务端（QUIC）
#[napi]
pub async fn connect(url: String) -> napi::Result<JsClient> {
    let client = ClientBuilder::new()
        .connect(&url)
        .await
        .map_err(to_napi_err)?;
    Ok(JsClient {
        client: Arc::new(client),
    })
}

/// EchoStream 客户端
#[napi]
pub struct JsClient {
    client: Arc<Client>,
}

#[napi]
impl JsClient {
    /// 发起 RPC 请求，返回响应载荷（postcard 字节）
    #[napi]
    pub async fn request(&self, name: String, payload: Vec<u8>) -> napi::Result<Vec<u8>> {
        let data = Bytes::from(payload);
        let resp: Bytes = self
            .client
            .request_raw(&name, data)
            .await
            .map_err(to_napi_err)?;
        Ok(resp.to_vec())
    }

    /// 发送单向事件
    #[napi]
    pub async fn emit(&self, name: String, payload: Vec<u8>) -> napi::Result<()> {
        let data = Bytes::from(payload);
        self.client.emit_raw(&name, data).await.map_err(to_napi_err)
    }

    /// 创建流（推送连续数据）
    #[napi]
    pub async fn create_stream(&self, name: String) -> napi::Result<JsStream> {
        let stream = self
            .client
            .create_stream(&name)
            .await
            .map_err(to_napi_err)?;
        Ok(JsStream {
            inner: tokio::sync::Mutex::new(stream),
        })
    }

    /// 注册事件监听（回调收到事件载荷 Buffer）
    #[napi]
    pub fn on_event(&self, name: String, callback: ThreadsafeFunction<Buffer>) {
        let handler = JsEventCallback {
            name: name.clone(),
            callback,
        };
        // 客户端事件处理器注册：通过构建器重建成本高，直接注册到内部 router
        // 使用 add_event 需要 ClientBuilder —— Client 内部 router 不可变，这里
        // 采用 Runtime 注册方式：core Client 提供 on_event 注册。
        self.client.add_event_handler(handler);
    }

    /// 关闭连接
    #[napi]
    pub fn close(&self) {
        self.client.close();
    }
}

/// 事件回调适配（ThreadsafeFunction → EventHandler）
struct JsEventCallback {
    name: String,
    callback: ThreadsafeFunction<Buffer>,
}

#[async_trait::async_trait]
impl EventHandler for JsEventCallback {
    type Data = Bytes;

    fn name(&self) -> &str {
        &self.name
    }

    async fn handle(&self, _session: &Session, data: Bytes) -> echostream::Result<()> {
        let buf = Buffer::from(data.as_ref());
        self.callback
            .call_async::<()>(Ok(buf))
            .await
            .map_err(|e| echostream::Error::Io(e.to_string()))?;
        Ok(())
    }
}

/// 流发送器（Node 侧句柄）
#[napi]
pub struct JsStream {
    inner: tokio::sync::Mutex<StreamSender>,
}

#[napi]
impl JsStream {
    /// 发送一帧
    #[napi]
    pub async fn send(&self, payload: Vec<u8>) -> napi::Result<()> {
        let mut stream = self.inner.lock().await;
        stream.send(Bytes::from(payload)).await.map_err(to_napi_err)
    }

    /// 关闭流
    #[napi]
    pub async fn finish(&self) -> napi::Result<()> {
        let mut stream = self.inner.lock().await;
        stream.finish().await.map_err(to_napi_err)
    }
}
