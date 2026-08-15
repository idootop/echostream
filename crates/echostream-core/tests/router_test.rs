//! Router 运行时管理测试：按 token 注册/移除、注册表查询、中间件链语义

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use bytes::Bytes;
use echostream_core::{Endpoint, Middleware, Next, Router, ServerContext, Session, StreamReceiver};
use echostream_proto::{Error, Message, Result};

// ======================== 桩实现 ========================

/// 无 I/O 端点桩（测试用）
struct NoopEndpoint;

#[async_trait]
impl Endpoint for NoopEndpoint {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    async fn open_bi(&self) -> Result<Box<dyn echostream_core::FrameIo>> {
        Err(Error::Protocol("stub".into()))
    }
    async fn open_uni(&self) -> Result<Box<dyn echostream_core::FrameIo>> {
        Err(Error::Protocol("stub".into()))
    }
    async fn accept_bi(&self) -> Result<Box<dyn echostream_core::FrameIo>> {
        Err(Error::Protocol("stub".into()))
    }
    async fn accept_uni(&self) -> Result<Box<dyn echostream_core::FrameIo>> {
        Err(Error::Protocol("stub".into()))
    }
    fn peer_addr(&self) -> SocketAddr {
        "127.0.0.1:1".parse().unwrap()
    }
    fn close(&self) {}
}

struct RpcStub(&'static str);

#[async_trait]
impl echostream_core::DynRpcHandler for RpcStub {
    fn name(&self) -> &str {
        self.0
    }
    async fn handle_encoded(&self, _s: &Session, _p: Bytes) -> Result<Bytes> {
        Ok(Bytes::from_static(b"ok"))
    }
}

struct EventStub(&'static str, Arc<AtomicUsize>);

#[async_trait]
impl echostream_core::DynEventHandler for EventStub {
    fn name(&self) -> &str {
        self.0
    }
    async fn handle_encoded(&self, _s: &Session, _p: Bytes) -> Result<()> {
        self.1.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct StreamStub(&'static str);

#[async_trait]
impl echostream_core::StreamHandler for StreamStub {
    fn name(&self) -> &str {
        self.0
    }
    async fn handle(&self, _s: &Session, mut stream: StreamReceiver) -> Result<()> {
        while stream.recv_frame().await?.is_some() {}
        Ok(())
    }
}

/// 记录链调用序的中间件（block 为 true 时拦截）
struct TraceMw(&'static str, Arc<AtomicUsize>, bool);

#[async_trait]
impl Middleware for TraceMw {
    fn name(&self) -> &str {
        self.0
    }

    async fn handle(&self, _s: &Session, msg: Message, next: Next) -> Result<Option<Message>> {
        self.1.fetch_add(1, Ordering::SeqCst);
        if self.2 {
            Ok(None) // 拦截
        } else {
            next.run(msg).await
        }
    }
}

/// 修改消息的中间件（改事件名，验证终端按修改后的名字查找）
struct RenameMw;

#[async_trait]
impl Middleware for RenameMw {
    fn name(&self) -> &str {
        "rename"
    }

    async fn handle(&self, _s: &Session, msg: Message, next: Next) -> Result<Option<Message>> {
        match msg {
            Message::Event(mut e) => {
                e.name = "renamed".into();
                next.run(Message::Event(e)).await
            }
            other => next.run(other).await,
        }
    }
}

fn session() -> Session {
    Session::new(1, Arc::new(NoopEndpoint), Arc::new(ServerContext::new()))
}

fn event(name: &str) -> echostream_proto::EventMsg {
    echostream_proto::EventMsg {
        id: 1,
        name: name.into(),
        data: Bytes::new(),
    }
}

// ======================== 测试 ========================

#[test]
fn token_register_and_remove() {
    let router = Router::default();
    let t1 = router.add_rpc(RpcStub("a"));
    let t2 = router.add_rpc(RpcStub("b"));
    let mut names = router.rpc_names();
    names.sort();
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
    assert!(router.remove_rpc(t1), "移除已注册的 token 应成功");
    assert!(!router.remove_rpc(t1), "重复移除应失败");
    assert_eq!(router.rpc_names(), vec!["b".to_string()]);
    let _ = t2;

    // 事件：同名多监听器，按 token 精确移除
    let e1 = router.add_event(EventStub("ev", Arc::new(AtomicUsize::new(0))));
    let e2 = router.add_event(EventStub("ev", Arc::new(AtomicUsize::new(0))));
    assert_eq!(router.event_names(), vec!["ev".to_string()]);
    assert!(router.remove_event(e1));
    assert!(router.remove_event(e2));
    assert!(router.event_names().is_empty(), "全部移除后注册表应为空");

    // 流与中间件
    let s1 = router.add_stream(StreamStub("chat"));
    assert_eq!(router.stream_names(), vec!["chat".to_string()]);
    assert!(router.has_stream("chat"));
    assert!(router.remove_stream(s1));
    assert!(!router.has_stream("chat"));

    let m1 = router.add_middleware(TraceMw("log", Arc::new(AtomicUsize::new(0)), false));
    assert_eq!(router.middleware_names(), vec!["log".to_string()]);
    assert!(router.remove_middleware(m1));
    assert!(router.middleware_names().is_empty());
}

#[tokio::test]
async fn middleware_chain_orders_and_intercepts() {
    let router = Router::default();
    let order = Arc::new(AtomicUsize::new(0));
    let hits = Arc::new(AtomicUsize::new(0));
    // 第一层放行，第二层拦截，第三层不应执行
    router.add_middleware(TraceMw("first", order.clone(), false));
    router.add_middleware(TraceMw("second", order.clone(), true));
    router.add_middleware(TraceMw("third", order.clone(), false));
    router.add_event(EventStub("ev", hits.clone()));

    let s = session();
    router.dispatch_event(&s, event("ev")).await;
    assert_eq!(order.load(Ordering::SeqCst), 2, "拦截后第三层不应执行");
    assert_eq!(hits.load(Ordering::SeqCst), 0, "拦截后处理器不应执行");
}

#[tokio::test]
async fn middleware_can_modify_message_before_terminal() {
    let router = Router::default();
    let hits = Arc::new(AtomicUsize::new(0));
    router.add_middleware(RenameMw);
    router.add_event(EventStub("renamed", hits.clone()));

    let s = session();
    router.dispatch_event(&s, event("x")).await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "终端应按修改后的事件名查找处理器"
    );
}

#[tokio::test]
async fn middleware_connect_disconnect_hooks_fire() {
    struct LifecycleMw(Arc<AtomicUsize>);

    #[async_trait]
    impl Middleware for LifecycleMw {
        fn name(&self) -> &str {
            "lifecycle"
        }
        async fn handle(&self, _s: &Session, msg: Message, next: Next) -> Result<Option<Message>> {
            next.run(msg).await
        }
        async fn on_connect(&self, _s: &Session) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn on_disconnect(&self, _s: &Session) -> Result<()> {
            self.0.fetch_add(10, Ordering::SeqCst);
            Ok(())
        }
    }

    let router = Router::default();
    let counter = Arc::new(AtomicUsize::new(0));
    router.add_middleware(LifecycleMw(counter.clone()));

    let s = session();
    router.run_connect_hooks(&s).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    router.run_disconnect_hooks(&s).await;
    assert_eq!(counter.load(Ordering::SeqCst), 11);
}
