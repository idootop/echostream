//! EchoStream Python binding（PyO3）
//!
//! 完整暴露 client + server 能力（与 Rust 核心同一份实现）：
//! - 客户端：`connect` / `Client`（request / emit / create_stream / on_event / close）
//! - 服务端：`ServerBuilder` / `Server`（run / shutdown / broadcast / sessions）
//! - 会话：`Session`（服务端主动调用客户端）
//!
//! 载荷约定：所有 RPC / Event / Stream 载荷为 postcard 编码字节（`bytes`），
//! 与 Rust 侧线缆格式一致。
//!
//! 同步 API：内部使用 tokio runtime（`block_on`）；事件回调为同步 Python 函数。

use std::sync::{Arc, OnceLock};

use bytes::Bytes;
use echostream::prelude::*;
use pyo3::conversion::IntoPyObjectExt;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

// ======================== 运行时 ========================

/// 全局 tokio runtime（同步 API 内部使用）
fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("创建 tokio runtime 失败")
    })
}

fn to_py_err(e: Error) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

fn block_on<F: std::future::Future>(f: F) -> Result<F::Output> {
    Ok(runtime().block_on(f))
}

// ======================== 客户端 ========================

/// 连接服务端（QUIC）
#[pyfunction]
pub fn connect(py: Python<'_>, url: &str) -> PyResult<Client> {
    let client = py
        .detach(|| block_on(ClientBuilder::new().connect(url)))
        .map_err(to_py_err)?
        .map_err(to_py_err)?;
    Ok(Client {
        client: Arc::new(client),
    })
}

/// EchoStream 客户端
#[pyclass]
pub struct Client {
    client: Arc<::echostream::Client>,
}

#[pymethods]
impl Client {
    /// 发起 RPC 请求，返回响应载荷（postcard 字节）
    fn request(&self, py: Python<'_>, name: &str, payload: &[u8]) -> PyResult<Py<PyAny>> {
        let data = Bytes::copy_from_slice(payload);
        let resp: Bytes = py
            .detach(|| block_on(self.client.request_raw(name, data)))
            .map_err(to_py_err)?
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &resp).into())
    }

    /// 发送单向事件
    fn emit(&self, py: Python<'_>, name: &str, payload: &[u8]) -> PyResult<()> {
        let data = Bytes::copy_from_slice(payload);
        py.detach(|| block_on(self.client.emit_raw(name, data)))
            .map_err(to_py_err)?
            .map_err(to_py_err)
    }

    /// 发送不可靠事件（数据报通道；连接不支持时返回错误）
    fn emit_unreliable(&self, py: Python<'_>, name: &str, payload: &[u8]) -> PyResult<()> {
        let data = Bytes::copy_from_slice(payload);
        py.detach(|| block_on(self.client.emit_unreliable_raw(name, data)))
            .map_err(to_py_err)?
            .map_err(to_py_err)
    }

    /// 创建流（推送连续数据）
    fn create_stream(&self, py: Python<'_>, name: &str) -> PyResult<Stream> {
        let stream = py
            .detach(|| block_on(self.client.create_stream(name)))
            .map_err(to_py_err)?
            .map_err(to_py_err)?;
        Ok(Stream {
            inner: tokio::sync::Mutex::new(stream),
        })
    }

    /// 创建流并携带元数据协商（dict：字符串 / 数字 / 布尔值）
    fn create_stream_with_metadata(
        &self,
        py: Python<'_>,
        name: &str,
        metadata: &Bound<'_, PyDict>,
    ) -> PyResult<Stream> {
        use echostream::StreamMetaEntry;
        let mut meta = Vec::with_capacity(metadata.len());
        for (k, v) in metadata.iter() {
            let key = k.extract::<String>()?;
            if let Ok(s) = v.extract::<String>() {
                meta.push(StreamMetaEntry::str(key, s));
            } else if let Ok(n) = v.extract::<i64>() {
                meta.push(StreamMetaEntry::num(key, n as u64));
            } else if let Ok(b) = v.extract::<bool>() {
                meta.push(StreamMetaEntry::bool(key, b));
            } else {
                return Err(to_py_err(echostream::Error::InvalidParameter(format!(
                    "metadata 值 {key} 类型不支持（仅字符串/数字/布尔）"
                ))));
            }
        }
        let stream = py
            .detach(|| block_on(self.client.create_stream_with_metadata(name, meta)))
            .map_err(to_py_err)?
            .map_err(to_py_err)?;
        Ok(Stream {
            inner: tokio::sync::Mutex::new(stream),
        })
    }

    /// 注册事件监听（回调签名：`handler(data: bytes) -> None`），返回注册 token（off_event 取消注册）
    fn on_event(&self, name: &str, callback: Py<PyAny>) -> u64 {
        self.client
            .add_event_handler(PyEventHandler::new(name, callback))
    }

    /// 取消注册事件监听（按 on_event 返回的 token）
    fn off_event(&self, token: u64) -> bool {
        self.client.remove_event_handler(token)
    }

    /// 注册 RPC 处理器（处理服务端主动调用，回调签名：`handler(data: bytes) -> bytes`），
    /// 返回注册 token（off_rpc 取消注册）
    fn add_rpc(&self, name: &str, callback: Py<PyAny>) -> u64 {
        self.client
            .add_rpc_handler(PyRpcHandler::new(name, callback))
    }

    /// 取消注册 RPC 处理器（按 add_rpc 返回的 token）
    fn off_rpc(&self, token: u64) -> bool {
        self.client.remove_rpc_handler(token)
    }

    /// 注册流处理器（服务端推送，回调签名：`handler(receiver)`，receiver.recv() 拉帧），
    /// 返回注册 token（off_stream 取消注册）
    fn add_stream(&self, name: &str, callback: Py<PyAny>) -> u64 {
        self.client
            .add_stream_handler(PyStreamHandler::new(name, callback))
    }

    /// 取消注册流处理器（按 add_stream 返回的 token）
    fn off_stream(&self, token: u64) -> bool {
        self.client.remove_stream_handler(token)
    }

    /// 关闭连接
    fn close(&self) {
        self.client.close();
    }
}

/// 流发送器
#[pyclass]
pub struct Stream {
    inner: tokio::sync::Mutex<StreamSender>,
}

#[pymethods]
impl Stream {
    /// 发送一帧
    fn send(&self, py: Python<'_>, payload: &[u8]) -> PyResult<()> {
        let mut stream = py.detach(|| runtime().block_on(self.inner.lock()));
        py.detach(|| block_on(stream.send_raw(Bytes::copy_from_slice(payload))))
            .map_err(to_py_err)?
            .map_err(to_py_err)
    }

    /// 关闭流
    fn finish(&self, py: Python<'_>) -> PyResult<()> {
        let mut stream = py.detach(|| runtime().block_on(self.inner.lock()));
        py.detach(|| block_on(stream.finish()))
            .map_err(to_py_err)?
            .map_err(to_py_err)
    }
}

// ======================== 事件回调 ========================

/// Python 函数 → EventHandler 适配
struct PyEventHandler {
    name: String,
    callback: Py<PyAny>,
}

impl PyEventHandler {
    fn new(name: &str, callback: Py<PyAny>) -> Self {
        Self {
            name: name.to_string(),
            callback,
        }
    }
}

#[async_trait::async_trait]
impl DynEventHandler for PyEventHandler {
    fn name(&self) -> &str {
        &self.name
    }

    async fn handle_encoded(&self, _session: &::echostream::Session, data: Bytes) -> Result<()> {
        Python::attach(|py| {
            let args = (PyBytes::new(py, &data),);
            self.callback
                .call1(py, args)
                .map_err(|e| Error::Io(e.to_string()))?;
            Ok(())
        })
    }
}

// ======================== 服务端 ========================

/// Python 侧流处理器（回调：receiver 句柄 → Python 侧 recv() 拉帧）
struct PyStreamHandler {
    name: String,
    callback: Py<PyAny>,
}

impl PyStreamHandler {
    fn new(name: &str, callback: Py<PyAny>) -> Self {
        Self {
            name: name.to_string(),
            callback,
        }
    }
}

#[async_trait::async_trait]
impl StreamHandler for PyStreamHandler {
    fn name(&self) -> &str {
        &self.name
    }

    async fn handle(&self, _session: &::echostream::Session, stream: StreamReceiver) -> Result<()> {
        Python::attach(|py| {
            let receiver = PyStreamReceiver {
                inner: tokio::sync::Mutex::new(Some(stream)),
            };
            self.callback
                .call1(py, (receiver,))
                .map_err(|e| Error::Io(e.to_string()))?;
            Ok(())
        })
    }
}

/// 流接收器（Python 侧句柄：同步拉帧）
#[pyclass]
pub struct PyStreamReceiver {
    inner: tokio::sync::Mutex<Option<StreamReceiver>>,
}

#[pymethods]
impl PyStreamReceiver {
    /// 读取下一帧载荷；流结束返回 None
    ///
    /// 可能被 tokio worker 线程调用（Python 回调内拉帧），
    /// 使用 `block_in_place` + `Handle::block_on` 兼容两种上下文。
    fn recv(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let handle = runtime().handle().clone();
        let result = py.detach(|| {
            tokio::task::block_in_place(|| {
                let mut guard = handle.block_on(self.inner.lock());
                match guard.as_mut() {
                    Some(recv) => handle.block_on(recv.recv_frame()),
                    None => Ok(None),
                }
            })
        });
        let frame = result.map_err(to_py_err)?;
        match frame {
            Some(f) => Ok(Some(PyBytes::new(py, &f.data).into())),
            None => Ok(None),
        }
    }

    /// 流元数据（来自 StreamOpen 首帧：音视频参数 / 文件信息等）
    fn metadata(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let guard = py.detach(|| {
            let handle = runtime().handle().clone();
            tokio::task::block_in_place(|| handle.block_on(self.inner.lock()))
        });
        let dict = PyDict::new(py);
        if let Some(recv) = guard.as_ref() {
            for m in recv.metadata() {
                dict.set_item(
                    m.key.as_str(),
                    String::from_utf8_lossy(&m.value).to_string(),
                )?;
            }
        }
        Ok(dict.unbind())
    }

    /// 结束码（0 正常 / 非 0 异常；流结束后有效，未结束返回 0）
    fn end_code(&self, py: Python<'_>) -> PyResult<u32> {
        let guard = py.detach(|| {
            let handle = runtime().handle().clone();
            tokio::task::block_in_place(|| handle.block_on(self.inner.lock()))
        });
        Ok(guard.as_ref().map(|r| r.end_code() as u32).unwrap_or(0))
    }

    /// 结束原因（流结束后有效；未结束返回 None）
    fn end_message(&self, py: Python<'_>) -> PyResult<Option<String>> {
        let guard = py.detach(|| {
            let handle = runtime().handle().clone();
            tokio::task::block_in_place(|| handle.block_on(self.inner.lock()))
        });
        Ok(guard
            .as_ref()
            .and_then(|r| r.end_message().map(|s| s.to_string())))
    }
}

/// 服务端（Python 侧句柄）
#[pyclass]
pub struct Server {
    server: ::echostream::Server,
    ctx: Arc<ServerContext>,
}

#[pymethods]
impl Server {
    /// 运行服务（阻塞直到 `shutdown`；请在其他线程调用 shutdown）
    fn run(&self, py: Python<'_>) -> PyResult<()> {
        py.detach(|| block_on(self.server.run()))
            .map_err(to_py_err)?
            .map_err(to_py_err)
    }

    /// 优雅关闭
    fn shutdown(&self) {
        self.server.shutdown();
    }

    /// 本地监听地址
    fn addr(&self) -> Option<String> {
        self.server.endpoint_addr().map(|a| a.to_string())
    }

    /// 广播事件到所有连接客户端
    fn broadcast(&self, py: Python<'_>, name: &str, payload: &[u8]) -> PyResult<()> {
        let data = Bytes::copy_from_slice(payload);
        py.detach(|| block_on(self.ctx.broadcast_raw(name, data)))
            .map_err(to_py_err)?
            .map_err(to_py_err)
    }

    /// 所有在线会话（可主动调用客户端）
    fn sessions(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let sessions = self
            .ctx
            .sessions()
            .into_iter()
            .map(|s| Session { session: s }.into_py_any(py).unwrap())
            .collect::<Vec<_>>();
        Ok(PyList::new(py, sessions)?.into())
    }
}

/// 会话（服务端视角）
#[pyclass]
pub struct Session {
    session: ::echostream::Session,
}

#[pymethods]
impl Session {
    /// 会话 ID
    fn id(&self) -> u64 {
        self.session.id()
    }

    /// 对端地址
    fn peer_addr(&self) -> String {
        self.session.peer_addr().to_string()
    }

    /// 主动调用客户端 RPC
    fn request(&self, py: Python<'_>, name: &str, payload: &[u8]) -> PyResult<Py<PyAny>> {
        let data = Bytes::copy_from_slice(payload);
        let resp: Bytes = py
            .detach(|| block_on(self.session.request_raw(name, data)))
            .map_err(to_py_err)?
            .map_err(to_py_err)?;
        Ok(PyBytes::new(py, &resp).into())
    }

    /// 向客户端发送事件
    fn emit(&self, py: Python<'_>, name: &str, payload: &[u8]) -> PyResult<()> {
        let data = Bytes::copy_from_slice(payload);
        py.detach(|| block_on(self.session.emit_raw(name, data)))
            .map_err(to_py_err)?
            .map_err(to_py_err)
    }

    /// 关闭连接
    fn close(&self) {
        self.session.close();
    }
}

/// 服务端构建器
#[pyclass]
pub struct ServerBuilder {
    router: Arc<Router>,
    ctx: Arc<ServerContext>,
    addr: Option<String>,
}

#[pymethods]
impl ServerBuilder {
    #[new]
    fn new() -> Self {
        Self {
            router: Arc::new(Router::default()),
            ctx: Arc::new(ServerContext::new()),
            addr: None,
        }
    }

    /// 绑定监听地址
    fn bind(&mut self, addr: &str) {
        self.addr = Some(addr.to_string());
    }

    /// 注册 RPC 处理器（回调签名：`handler(data: bytes) -> bytes`）
    fn add_rpc(&self, name: &str, callback: Py<PyAny>) {
        self.router.add_rpc(PyRpcHandler::new(name, callback));
    }

    /// 注册事件处理器（回调签名：`handler(data: bytes) -> None`）
    fn add_event(&self, name: &str, callback: Py<PyAny>) {
        self.router.add_event(PyEventHandler::new(name, callback));
    }

    /// 注册流处理器（回调签名：`handler(receiver)`，Python 侧 `receiver.recv()` 拉帧）
    fn add_stream(&self, name: &str, callback: Py<PyAny>) {
        self.router.add_stream(PyStreamHandler::new(name, callback));
    }

    /// 构建服务端
    fn build(&self) -> PyResult<Server> {
        let addr = self
            .addr
            .clone()
            .ok_or_else(|| PyRuntimeError::new_err("未指定监听地址"))?;
        let server = block_on(
            ::echostream::ServerBuilder::new()
                .with_router(self.router.clone())
                .with_ctx(self.ctx.clone())
                .bind(addr)
                .build(),
        )
        .map_err(to_py_err)?
        .map_err(to_py_err)?;
        Ok(Server {
            server,
            ctx: self.ctx.clone(),
        })
    }
}

/// Python 函数 → RPC 处理器适配
struct PyRpcHandler {
    name: String,
    callback: Py<PyAny>,
}

impl PyRpcHandler {
    fn new(name: &str, callback: Py<PyAny>) -> Self {
        Self {
            name: name.to_string(),
            callback,
        }
    }
}

#[async_trait::async_trait]
impl DynRpcHandler for PyRpcHandler {
    fn name(&self) -> &str {
        &self.name
    }

    async fn handle_encoded(
        &self,
        _session: &::echostream::Session,
        payload: Bytes,
    ) -> Result<Bytes> {
        Python::attach(|py| {
            let args = (PyBytes::new(py, &payload),);
            let resp = self
                .callback
                .call1(py, args)
                .map_err(|e| Error::Io(e.to_string()))?;
            let bytes = resp
                .extract::<Vec<u8>>(py)
                .map_err(|e| Error::Io(e.to_string()))?;
            Ok(Bytes::from(bytes))
        })
    }
}

// ======================== 模块注册 ========================

#[pymodule]
#[pyo3(name = "_native")]
fn echostream_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(connect, m)?)?;
    m.add_class::<Client>()?;
    m.add_class::<Stream>()?;
    m.add_class::<Server>()?;
    m.add_class::<Session>()?;
    m.add_class::<ServerBuilder>()?;
    m.add_class::<PyStreamReceiver>()?;
    Ok(())
}
