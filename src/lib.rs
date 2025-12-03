pub mod kcp2k;
pub mod kcp2k_client;
pub mod kcp2k_common;
pub mod kcp2k_config;
pub mod kcp2k_connection;
pub mod kcp2k_server;

pub use revel_cell;

#[cfg(test)]
mod tests {
    use crate::kcp2k_client::Kcp2KClient;
    use crate::kcp2k_common::{Callback, CallbackType, Kcp2KChannel};
    use crate::kcp2k_config::Kcp2KConfig;
    use crate::kcp2k_connection::Kcp2kConnection;
    use crate::kcp2k_server::Kcp2KServer;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn call_back(conn: &Kcp2kConnection, cb: Callback) {
        println!("{}", cb);

        let now = SystemTime::now();
        let seconds_since_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards").as_millis();
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

    #[test]
    pub fn test_server() {
        env_logger::init();
        let config = Kcp2KConfig::default();

        let server = Kcp2KServer::new("0.0.0.0:3100".to_string(), config, call_back);

        loop {
            server.tick();
        }
    }

    #[test]
    pub fn test_client() {
        env_logger::init();
        let config = Kcp2KConfig::default();

        let client = Kcp2KClient::new(config, call_back);

        client.connect("127.0.0.1:3100".to_string());

        loop {
            client.tick();
        }
    }
}
