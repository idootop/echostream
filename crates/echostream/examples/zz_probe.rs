//! 临时探针：验证 postcard 对 i64 的编码（确认是否为 ZigZag varint）——用完即删
use echostream::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
    }
    println!("i64 10          = {}", hex(&echostream::codec::encode(&10i64)?));
    println!("i64 20          = {}", hex(&echostream::codec::encode(&20i64)?));
    println!("(i64,i64) (10,20)= {}", hex(&echostream::codec::encode(&(10i64, 20i64))?));
    println!("(u64,u64) (10,20)= {}", hex(&echostream::codec::encode(&(10u64, 20u64))?));
    println!("i64 15          = {}", hex(&echostream::codec::encode(&15i64)?));
    println!("i64 30          = {}", hex(&echostream::codec::encode(&30i64)?));
    println!("i64 60          = {}", hex(&echostream::codec::encode(&60i64)?));
    Ok(())
}
