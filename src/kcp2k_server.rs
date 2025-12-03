use crate::kcp2k::{Kcp2K, Kcp2KMode};
use crate::kcp2k_common::{connection_hash, CallbackFuncType, Kcp2KChannel, Kcp2KConnectionStates, Kcp2KError};
use crate::kcp2k_config::Kcp2KConfig;
use crate::kcp2k_connection::Kcp2kConnection;
use log::{error, info};
use revel_cell::arc::Arc;
use socket2::SockAddr;
use std::collections::BTreeMap;
use std::io::Error;
use std::net::SocketAddr;

pub struct Kcp2KServer {
    kcp2k: Kcp2K,
    connections: Arc<BTreeMap<u64, Arc<Kcp2kConnection>>>,
}

impl Kcp2KServer {
    fn handle_data(&self, sock_addr: &SockAddr, data: &[u8]) {
        // 生成连接 ID
        let conn_id = connection_hash(sock_addr);
        // 如果连接存在，则处理数据
        match self.connections.get(&conn_id) {
            None => {
                let conn_id = connection_hash(&sock_addr);
                let kcp_server_connection = Kcp2kConnection::new(conn_id, self.kcp2k.config.clone(), Arc::new(Kcp2KMode::Server), self.kcp2k.socket.clone(), Arc::new(sock_addr.clone()), self.kcp2k.callback_func);
                self.connections.value_mut().insert(conn_id, Arc::new(kcp_server_connection));
            }
            Some(conn) => {
                if let Err(e) = conn.value_mut().raw_input(data) {
                    error!("Error reading from data: {}", e);
                }
            }
        }
    }
}

impl Kcp2KServer {
    pub fn new(addr: String, config: Kcp2KConfig, callback: CallbackFuncType) -> Self {
        let kcp2k = Kcp2K::new(config, callback);
        let socket_addr = match addr.parse::<SocketAddr>() {
            Ok(addr) => addr,
            Err(e) => panic!("{}", Kcp2KError::Unexpected(e.to_string())),
        };
        if let Err(e) = kcp2k.socket.bind(&socket_addr.into()) {
            panic!("{}", Kcp2KError::Unexpected(e.to_string()))
        }
        if let Ok(local_addr) = kcp2k.socket.local_addr()
            && let Some(socket_addr) = local_addr.as_socket()
        {
            info!("[KCP2K] Server bind on: {:?}", socket_addr);
        }
        Kcp2KServer { connections: Arc::new(BTreeMap::new()), kcp2k }
    }

    pub fn tick(&self) {
        self.tick_incoming();
        self.tick_outgoing();
    }

    pub fn tick_incoming(&self) {
        // 移除断开连接的连接
        self.connections.value_mut().retain(|_, conn| *conn.state != Kcp2KConnectionStates::Disconnected);

        while let Some((sock_addr, data)) = self.kcp2k.raw_receive_from() {
            self.handle_data(&sock_addr, &data);
        }

        for connection in self.connections.values() {
            connection.tick_incoming();
        }
    }

    pub fn tick_outgoing(&self) {
        for connection in self.connections.values() {
            connection.tick_outgoing();
        }
    }

    pub fn connections(&self) -> &Arc<BTreeMap<u64, Arc<Kcp2kConnection>>> {
        &self.connections
    }

    pub fn send(&self, conn_id: u64, data: &[u8], channel: Kcp2KChannel) -> Result<(), Kcp2KError> {
        if let Some(conn) = self.connections.get(&conn_id) {
            return conn.send_data(data, channel);
        }
        Err(Kcp2KError::ConnectionNotFound("Connection not found".to_string()))
    }

    pub fn stop(&self) -> Result<(), Error> {
        self.kcp2k.socket.shutdown(std::net::Shutdown::Both)
    }
}
