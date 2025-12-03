pub mod kcp2k_tests {
    use kcp2k_rs::kcp2k_client::Kcp2KClient;
    use kcp2k_rs::kcp2k_common::{Callback, CallbackType, Kcp2KChannel};
    use kcp2k_rs::kcp2k_config::Kcp2KConfig;
    use kcp2k_rs::kcp2k_connection::Kcp2kConnection;
    use kcp2k_rs::kcp2k_server::Kcp2KServer;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn call_back(conn: &Kcp2kConnection, cb: Callback) {
        println!("{}", cb);

        let now = SystemTime::now();
        let seconds_since_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards").as_millis(); // as_secs() 返回 u64
        // 2. 将 u64 时间戳转换为小端字节序的字节数组
        let time = seconds_since_epoch.to_le_bytes();

        match cb.r#type {
            CallbackType::OnConnected => {
                let _ = conn.send_data(time.as_slice(), Kcp2KChannel::Unreliable);
            }
            CallbackType::OnData => {
                let _ = conn.send_data(time.as_slice(), Kcp2KChannel::Reliable);
            }
            CallbackType::OnError => {}
            CallbackType::OnDisconnected => {}
        }
    }

    pub fn test_server() {
        let config = Kcp2KConfig::default();

        let server = Kcp2KServer::new("0.0.0.0:3100".to_string(), config, call_back);

        loop {
            server.tick();
        }
    }

    pub fn test_client() {
        let config = Kcp2KConfig::default();

        let client = Kcp2KClient::new(config, call_back);

        client.connect("10.119.120.22:3100".to_string());

        loop {
            client.tick();
        }
    }
}

fn main() {
    kcp2k_tests::test_server();
    kcp2k_tests::test_client();
}
