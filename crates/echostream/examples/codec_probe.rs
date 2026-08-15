//! 协议编解码探针：输出 postcard 字节（hex），用于与 JS SDK 交叉验证
//!
//! 运行：`cargo run -p echostream --example codec_probe`

use echostream::prelude::*;

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[tokio::main]
async fn main() -> Result<()> {
    // 载荷样例
    println!(
        "encode_string_hello      = {}",
        hex(&echostream::codec::encode(&"hello".to_string())?)
    );
    println!(
        "encode_tuple_10_20       = {}",
        hex(&echostream::codec::encode(&(10u64, 20u64))?)
    );
    println!(
        "encode_vec_1_2_3         = {}",
        hex(&echostream::codec::encode(&vec![1u32, 2, 3])?)
    );
    println!(
        "encode_u64_42            = {}",
        hex(&echostream::codec::encode(&42u64)?)
    );

    // 消息样例
    let req = Message::Request(RequestMsg {
        id: 1,
        name: "add".into(),
        data: echostream::codec::encode(&(10u64, 20u64))?,
    });
    println!(
        "message_request_payload  = {}",
        hex(&postcard::to_allocvec(&req).map_err(|e| Error::Serialization(e.to_string()))?)
    );

    let resp = Message::Response(ResponseMsg {
        id: 1,
        code: StatusCode::SUCCESS,
        message: None,
        data: echostream::codec::encode(&30u64)?,
    });
    println!(
        "message_response_payload = {}",
        hex(&postcard::to_allocvec(&resp).map_err(|e| Error::Serialization(e.to_string()))?)
    );

    let event = Message::Event(EventMsg {
        id: 2,
        name: "hello".into(),
        data: echostream::codec::encode(&"world".to_string())?,
    });
    println!(
        "message_event_payload    = {}",
        hex(&postcard::to_allocvec(&event).map_err(|e| Error::Serialization(e.to_string()))?)
    );

    let stream = Message::Stream(StreamMsg {
        id: 3,
        seq: 0,
        sender_ts: Timestamp(123),
        data: echostream::codec::encode(&"hi".to_string())?,
    });
    println!(
        "message_stream_payload   = {}",
        hex(&postcard::to_allocvec(&stream).map_err(|e| Error::Serialization(e.to_string()))?)
    );
    Ok(())
}
