use crate::kcp2k::{Kcp2K, Kcp2KMode};
use crate::kcp2k_common::{connection_hash, CallbackFuncType, Kcp2KChannel, Kcp2KConnectionStates, Kcp2KError};
use crate::kcp2k_config::Kcp2KConfig;
use crate::kcp2k_connection::Kcp2kConnection;
use log::{error, info};
use revel_cell::arc::Arc;
use socket2::SockAddr;
use std::io::Error;
use std::net::SocketAddr;

pub struct Kcp2KClient {
    kcp2k: Kcp2K,
    connection: Arc<Option<Kcp2kConnection>>,
}

impl Kcp2KClient {
    fn create_connection(&self, sock_addr: SockAddr) {
        let conn_id = connection_hash(&sock_addr);
        self.connection.set_value(Some(Kcp2kConnection::new(conn_id, self.kcp2k.config.clone(), Arc::new(Kcp2KMode::Client), self.kcp2k.socket.clone(), Arc::new(sock_addr), self.kcp2k.callback_func)));
    }

    fn handle_data(&self, sock_addr: &SockAddr, data: &[u8]) {
        // 如果连接存在，则处理数据
        match self.connection.value_mut() {
            None => {
                error!("[KCP2K] No connection found for incoming data from {:?}", sock_addr);
            }
            Some(conn) => {
                if let Err(e) = conn.raw_input(data) {
                    error!("Error reading from data: {}", e);
                }
            }
        }
    }
}

impl Kcp2KClient {
    pub fn new(config: Kcp2KConfig, callback: CallbackFuncType) -> Self {
        let kcp2k = Kcp2K::new(config, callback);
        let client = Kcp2KClient { kcp2k, connection: Default::default() };
        client
    }

    pub fn connect(&self, addr: String) {
        let socket_addr = match addr.parse::<SocketAddr>() {
            Ok(addr) => addr,
            Err(e) => panic!("{}", Kcp2KError::Unexpected(e.to_string())),
        };
        if let Err(e) = self.kcp2k.socket.connect(&socket_addr.into()) {
            panic!("{}", Kcp2KError::Unexpected(e.to_string()))
        }
        if let Ok(local_addr) = self.kcp2k.socket.local_addr()
            && let Some(socket_addr) = local_addr.as_socket()
        {
            self.create_connection(socket_addr.into());
            info!("[KCP2K] Client connecting to: {}", addr);
        }

        if let Some(connection) = self.connection.value_mut() {
            connection.send_hello();
        }
    }

    pub fn tick(&self) {
        self.tick_incoming();
        self.tick_outgoing();
    }

    pub fn tick_incoming(&self) {
        if let Some(conn) = self.connection.value_mut()
            && *conn.state == Kcp2KConnectionStates::Disconnected
        {
            self.connection.set_value(None);
        }

        while let Some((sock_addr, data)) = self.kcp2k.raw_receive_from() {
            self.handle_data(&sock_addr, &data);
        }

        if let Some(conn) = self.connection.value_mut() {
            conn.tick_incoming();
        }
    }

    pub fn tick_outgoing(&self) {
        if let Some(conn) = self.connection.value_mut() {
            conn.tick_outgoing();
        }
    }

    pub fn connection(&self) -> &Arc<Option<Kcp2kConnection>> {
        &self.connection
    }

    pub fn send(&self, data: &[u8], channel: Kcp2KChannel) -> Result<(), Kcp2KError> {
        if let Some(conn) = self.connection.value_mut() {
            return conn.send_data(data, channel);
        }
        Err(Kcp2KError::ConnectionClosed("Connection is closed".to_string()))
    }

    pub fn stop(&self) -> Result<(), Error> {
        self.kcp2k.socket.shutdown(std::net::Shutdown::Both)
    }
}
