//! ClientCore 状态机单元测试：RPC 匹配 / 事件路由 / 主动调用 / 流序号

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use echostream_core::ClientCore;
use echostream_proto::{
    EventMsg, Message, RequestMsg, ResponseMsg, StatusCode, StreamMetaEntry, StreamMsg, Timestamp,
};

#[test]
fn rpc_response_matches_by_id() {
    let mut core = ClientCore::new();
    let got = Arc::new(AtomicU64::new(0));

    // 发起两个请求，响应乱序到达也应正确匹配
    let (id1, req1) = core.build_request("add", Bytes::from(vec![1]), {
        let got = got.clone();
        move |data: Bytes, _err: Option<String>| {
            assert_eq!(data.as_ref(), b"r1");
            got.fetch_add(1, Ordering::Relaxed);
        }
    });
    let (id2, req2) = core.build_request("add", Bytes::from(vec![2]), {
        let got = got.clone();
        move |data: Bytes, _err: Option<String>| {
            assert_eq!(data.as_ref(), b"r2");
            got.fetch_add(10, Ordering::Relaxed);
        }
    });
    assert_ne!(id1, id2);
    match (&req1, &req2) {
        (Message::Request(a), Message::Request(b)) => {
            assert_eq!(a.id, 1);
            assert_eq!(b.id, 2);
        }
        _ => panic!("期望 Request"),
    }

    // 乱序响应
    let resp2 = Message::Response(ResponseMsg {
        id: id2,
        code: StatusCode::SUCCESS,
        message: None,
        data: Bytes::from_static(b"r2"),
    });
    assert!(core.handle_inbound(resp2).is_none());
    let resp1 = Message::Response(ResponseMsg {
        id: id1,
        code: StatusCode::SUCCESS,
        message: None,
        data: Bytes::from_static(b"r1"),
    });
    assert!(core.handle_inbound(resp1).is_none());
    assert_eq!(got.load(Ordering::Relaxed), 11);
}

#[test]
fn unknown_response_id_ignored() {
    let mut core = ClientCore::new();
    let resp = Message::Response(ResponseMsg {
        id: 999,
        code: StatusCode::SUCCESS,
        message: None,
        data: Bytes::new(),
    });
    assert!(core.handle_inbound(resp).is_none()); // 不应 panic
}

#[test]
fn error_response_calls_back_with_empty() {
    let mut core = ClientCore::new();
    let called = Arc::new(AtomicU64::new(0));
    let (id, _) = core.build_request("x", Bytes::new(), {
        let called = called.clone();
        move |data: Bytes, _err: Option<String>| {
            assert!(data.is_empty(), "错误响应应回调空数据");
            called.fetch_add(1, Ordering::Relaxed);
        }
    });
    let resp = Message::Response(ResponseMsg {
        id,
        code: StatusCode::FORBIDDEN,
        message: Some("被拦截".into()),
        data: Bytes::new(),
    });
    core.handle_inbound(resp);
    assert_eq!(called.load(Ordering::Relaxed), 1);
}

#[test]
fn event_routes_to_listeners() {
    let mut core = ClientCore::new();
    let count = Arc::new(AtomicU64::new(0));
    core.on_event("hello", {
        let count = count.clone();
        move |_name: &str, data: Bytes| {
            assert_eq!(data.as_ref(), b"world");
            count.fetch_add(1, Ordering::Relaxed);
        }
    });
    let evt = Message::Event(EventMsg {
        id: 1,
        name: "hello".into(),
        data: Bytes::from_static(b"world"),
    });
    core.handle_inbound(evt);
    assert_eq!(count.load(Ordering::Relaxed), 1);

    // 未注册的事件忽略
    let evt2 = Message::Event(EventMsg {
        id: 2,
        name: "nope".into(),
        data: Bytes::new(),
    });
    core.handle_inbound(evt2);
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[test]
fn server_initiated_rpc_gets_response() {
    let mut core = ClientCore::new();
    core.on_rpc("add", |_id: u64, data: Bytes| {
        let (a, b): (i64, i64) = postcard::from_bytes(&data).unwrap();
        Some(postcard::to_allocvec(&(a + b)).unwrap().into())
    });

    let req = Message::Request(RequestMsg {
        id: 7,
        name: "add".into(),
        data: postcard::to_allocvec(&(10i64, 20i64)).unwrap().into(),
    });
    let resp = core.handle_inbound(req).expect("应有响应");
    match resp {
        Message::Response(r) => {
            assert_eq!(r.id, 7);
            assert!(r.code.is_success());
            let sum: i64 = postcard::from_bytes(&r.data).unwrap();
            assert_eq!(sum, 30);
        }
        other => panic!("期望 Response，得到 {other:?}"),
    }
}

#[test]
fn unknown_rpc_gets_error_response() {
    let mut core = ClientCore::new();
    let req = Message::Request(RequestMsg {
        id: 3,
        name: "missing".into(),
        data: Bytes::new(),
    });
    let resp = core.handle_inbound(req).expect("应有错误响应");
    match resp {
        Message::Response(r) => {
            assert_eq!(r.id, 3);
            assert!(!r.code.is_success());
        }
        other => panic!("期望 Response，得到 {other:?}"),
    }
}

#[test]
fn async_rpc_handler_defers_response() {
    let mut core = ClientCore::new();
    core.on_rpc("slow", |_id: u64, _: Bytes| None); // 异步：稍后补响应

    let req = Message::Request(RequestMsg {
        id: 5,
        name: "slow".into(),
        data: Bytes::new(),
    });
    assert!(core.handle_inbound(req).is_none(), "异步处理不应立即响应");

    // 调用方稍后补响应
    let resp = core.build_response(5, Bytes::from_static(b"done"));
    match resp {
        Message::Response(r) => assert_eq!(r.id, 5),
        other => panic!("期望 Response，得到 {other:?}"),
    }
}

#[test]
fn stream_seq_increments_per_stream() {
    let mut core = ClientCore::new();
    let id = core.open_stream("chat");
    let f1 = core.build_stream_frame(id, Bytes::from(vec![1]), 0, 0, 0);
    let f2 = core.build_stream_frame(id, Bytes::from(vec![2]), 0, 0, 0);
    match (f1, f2) {
        (Message::Stream(a), Message::Stream(b)) => {
            assert_eq!(a.seq, 0);
            assert_eq!(b.seq, 1);
        }
        _ => panic!("期望 Stream 帧"),
    }
    // 另一条流序号独立
    let id2 = core.open_stream("chat");
    let f3 = core.build_stream_frame(id2, Bytes::from(vec![3]), 0, 0, 0);
    match f3 {
        Message::Stream(s) => assert_eq!(s.seq, 0),
        _ => panic!("期望 Stream 帧"),
    }
}

#[test]
fn stream_open_carries_metadata_and_routes_by_id() {
    let mut core = ClientCore::new();
    let got = Arc::new(std::sync::Mutex::new(Vec::new()));
    core.on_stream("video", {
        let got = got.clone();
        move |frame: Option<StreamMsg>| match frame {
            Some(f) => got.lock().unwrap().push(format!("frame:{}", f.seq)),
            None => got.lock().unwrap().push("end".to_string()),
        }
    });

    // 流开始帧：名称 + 元数据
    let id = core.open_stream("video");
    let open = core.build_stream_open(
        id,
        "video",
        vec![
            StreamMetaEntry::str("codec", "h264"),
            StreamMetaEntry::num("width", 1920),
            StreamMetaEntry::num("height", 1080),
        ],
    );
    match &open {
        Message::StreamOpen(o) => {
            assert_eq!(o.name, "video");
            assert_eq!(o.metadata.len(), 3);
        }
        other => panic!("期望 StreamOpen，得到 {other:?}"),
    }
    core.handle_inbound(open);

    // 数据帧（无 name）按 id 路由
    let f1 = core.build_stream_frame(id, Bytes::from(vec![1]), 0, 0, 44100);
    match &f1 {
        Message::Stream(s) => assert_eq!(s.rtp_ts, 44100),
        other => panic!("期望 Stream，得到 {other:?}"),
    }
    core.handle_inbound(f1);

    // 元数据查询
    let meta = core.stream_metadata(id).unwrap();
    assert_eq!(meta[0].key, "codec");
    assert_eq!(core.stream_metadata(id).unwrap()[1].key, "width");

    // 结束帧：记录结束信息
    core.handle_inbound(core.build_stream_end(id, 7, Some("cancelled".to_string())));
    let end = core.stream_end(id).unwrap();
    assert_eq!(end.code, 7);
    assert_eq!(end.message.as_deref(), Some("cancelled"));

    let frames = got.lock().unwrap().clone();
    assert_eq!(frames, vec!["frame:0".to_string(), "end".to_string()]);

    // 清理状态
    core.remove_stream_state(id);
    assert!(core.stream_metadata(id).is_none());
}

#[test]
fn datagram_event_is_self_describing() {
    let mut core = ClientCore::new();
    let raw = core.build_datagram_event("pos", Bytes::from_static(&[9, 9]));
    // 数据报载荷为裸 postcard Message，可独立解码
    let msg: Message = postcard::from_bytes(&raw).unwrap();
    match msg {
        Message::Event(e) => {
            assert_eq!(e.name, "pos");
            assert_eq!(e.data.as_ref(), &[9, 9]);
        }
        other => panic!("期望 Event，得到 {other:?}"),
    }
}

#[test]
fn listeners_can_be_unregistered() {
    let mut core = ClientCore::new();
    let count = Arc::new(AtomicU64::new(0));

    // 事件：同名多监听器，按 id 精确移除
    let id1 = core.on_event("ev", {
        let count = count.clone();
        move |_n: &str, _d: Bytes| {
            count.fetch_add(1, Ordering::Relaxed);
        }
    });
    let _id2 = core.on_event("ev", {
        let count = count.clone();
        move |_n: &str, _d: Bytes| {
            count.fetch_add(10, Ordering::Relaxed);
        }
    });
    core.handle_inbound(Message::Event(EventMsg {
        id: 1,
        name: "ev".into(),
        data: Bytes::new(),
    }));
    assert_eq!(count.load(Ordering::Relaxed), 11);
    assert!(core.off_event(id1), "移除已注册监听应成功");
    assert!(!core.off_event(id1), "重复移除应失败");
    core.handle_inbound(Message::Event(EventMsg {
        id: 2,
        name: "ev".into(),
        data: Bytes::new(),
    }));
    assert_eq!(count.load(Ordering::Relaxed), 21, "仅剩余 id2 监听器");

    // RPC / 流：按 id 移除
    let rpc_id = core.on_rpc("ping", |_id, _d| None);
    assert!(core.off_rpc(rpc_id));
    let stream_id = core.on_stream("chat", |_f| {});
    assert!(core.off_stream(stream_id));
}

#[test]
fn stream_end_and_timestamp_helpers() {
    let core = ClientCore::new();
    let end = core.build_stream_end(42, 0, None);
    match end {
        Message::StreamEnd(e) => {
            assert_eq!(e.id, 42);
            assert_eq!(e.code, 0);
        }
        other => panic!("期望 StreamEnd，得到 {other:?}"),
    }
    let end_err = core.build_stream_end(43, 7, Some("boom".to_string()));
    match end_err {
        Message::StreamEnd(e) => {
            assert_eq!(e.code, 7);
            assert_eq!(e.message.as_deref(), Some("boom"));
        }
        other => panic!("期望 StreamEnd，得到 {other:?}"),
    }
    let ts = Timestamp::now();
    assert!(ts.as_millis() > 0);
    let _ = StreamMsg {
        id: 0,
        seq: 0,
        flags: 0,
        sender_ts: ts,
        rtp_ts: 0,
        data: Bytes::new(),
    };
}
