use kcp2k_rust::kcp2k_common::{Callback, CallbackType, Kcp2KChannel};
use kcp2k_rust::kcp2k_config::Kcp2KConfig;
use kcp2k_rust::kcp2k_connection::Kcp2kConnection;
use kcp2k_rust::kcp2k_server::Kcp2KServer;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn call_back(conn: &Kcp2kConnection, cb: Callback) {
    println!("server - {}", cb);

    let now = SystemTime::now();
    let seconds_since_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards").as_millis();
    // 2. 将 u64 时间戳转换为小端字节序的字节数组
    let time = seconds_since_epoch.to_le_bytes();

    if let CallbackType::OnConnected = cb.r#type {
        let _ = conn.send_data(time.as_slice(), Kcp2KChannel::Unreliable);
    } else if let CallbackType::OnData = cb.r#type {
        let _ = conn.send_data(time.as_slice(), Kcp2KChannel::Reliable);
    } else if let CallbackType::OnError = cb.r#type {}
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    let config = Kcp2KConfig::default();

    let server = Kcp2KServer::new("0.0.0.0:3100".to_string(), config, call_back);

    loop {
        server.tick();
    }
}
