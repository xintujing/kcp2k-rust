use crate::kcp2k::Kcp2KMode;
use crate::kcp2k_common::{generate_cookie, Callback, CallbackFuncType, CallbackType, Kcp2KChannel, Kcp2KConnectionStates, Kcp2KError, Kcp2KReliableHeader, Kcp2KUnreliableHeader};
use crate::kcp2k_config::Kcp2KConfig;
use kcp::Kcp;
use revel_cell::arc::Arc;
use socket2::{SockAddr, Socket};
use std::io;
use std::io::Write;
use std::time::{Duration, Instant};

#[allow(unused)]
pub struct Kcp2kConnection {
    id: u64,
    config: Arc<Kcp2KConfig>,
    kcp2k_mode: Arc<Kcp2KMode>,
    callback_func: CallbackFuncType,
    cookie: Arc<u32>,
    pub(crate) state: Arc<Kcp2KConnectionStates>,
    socket: Arc<Socket>,
    client_sock_addr: Arc<SockAddr>,
    kcp: Arc<Kcp<UdpOutput>>,
    watch: Instant,
    last_send_ping_time: Arc<Duration>,
    last_recv_time: Arc<Duration>,
}

#[derive(Debug)]
pub struct UdpOutput {
    kcp2k_mode: Arc<Kcp2KMode>,      // kcp2k_mode
    cookie: Arc<u32>,                // cookie
    socket: Arc<Socket>,             // socket
    client_sock_addr: Arc<SockAddr>, // client_sock_addr
}
impl UdpOutput {
    // 创建一个新的 Writer，用于将数据包写入 UdpSocket
    fn new(kcp2k_mode: Arc<Kcp2KMode>, cookie: Arc<u32>, socket: Arc<Socket>, client_sock_addr: Arc<SockAddr>) -> UdpOutput {
        UdpOutput { kcp2k_mode, cookie, socket, client_sock_addr }
    }
}
impl Write for UdpOutput {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // 创建一个缓冲区，用于存储消息内容
        let mut buffer = Vec::new();

        // 写入通道头部
        buffer.push(Kcp2KChannel::Reliable.into());

        // 写入握手 cookie 以防止 UDP 欺骗
        buffer.extend_from_slice(&self.cookie.to_le_bytes());

        // 写入 data
        buffer.extend_from_slice(buf);

        // 发送数据
        match match *self.kcp2k_mode {
            // 客户端
            Kcp2KMode::Client => self.socket.send(&buffer),
            // 服务器
            Kcp2KMode::Server => self.socket.send_to(&buffer, &self.client_sock_addr),
        } {
            // 发送成功
            Ok(_) => Ok(buf.len()),
            // 发送失败
            Err(err) => Err(err),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Kcp2kConnection {
    pub(crate) fn new(id: u64, config: Arc<Kcp2KConfig>, kcp2k_mode: Arc<Kcp2KMode>, socket: Arc<Socket>, client_sock_addr: Arc<SockAddr>, callback_func: CallbackFuncType) -> Self {
        // generate cookie
        let cookie = match *kcp2k_mode {
            Kcp2KMode::Client => Arc::new(0),
            Kcp2KMode::Server => Arc::new(generate_cookie()),
        };

        // set up kcp over a reliable channel (that's what kcp is for)
        let udp_output = UdpOutput::new(kcp2k_mode.clone(), cookie.clone(), socket.clone(), client_sock_addr.clone());

        // kcp
        let mut kcp = Kcp::new(0, udp_output);
        // set nodelay.
        // note that kcp uses 'nocwnd' internally so we negate the parameter
        kcp.set_nodelay(if config.no_delay { true } else { false }, config.interval, config.fast_resend, !config.congestion_window);
        kcp.set_wndsize(config.send_window_size, config.receive_window_size);

        // IMPORTANT: high level needs to add 1 channel byte to each raw
        // message. so while Kcp.MTU_DEF is perfect, we actually need to
        // tell kcp to use MTU-1 so we can still put the header into the
        // message afterward.
        let _ = kcp.set_mtu(config.mtu - Kcp2KConfig::METADATA_SIZE_RELIABLE);

        // set maximum retransmits (aka dead_link)
        kcp.set_maximum_resend_times(config.max_retransmits);

        let connection = Kcp2kConnection {
            id,
            config,
            kcp2k_mode,
            callback_func,
            cookie,
            state: Arc::new(Kcp2KConnectionStates::Connected),
            socket,
            client_sock_addr,
            kcp: Arc::new(kcp),
            watch: Instant::now(),
            last_send_ping_time: Default::default(),
            last_recv_time: Default::default(),
        };

        connection
    }

    // 发送 Hello 消息
    pub(crate) fn send_hello(&self) {
        let _ = self.send_reliable(Kcp2KReliableHeader::Hello, Default::default());
    }

    pub(crate) fn raw_input(&mut self, segment: &[u8]) -> Result<(), Kcp2KError> {
        if segment.len() <= 5 {
            let err = Kcp2KError::InvalidReceive(format!("{}: Received invalid message with length={}. Disconnecting the connection.", std::any::type_name::<Self>(), segment.len()));
            self.on_error(err.clone());
            return Err(err);
        }

        // cookie
        let message_cookie = u32::from_le_bytes([segment[1], segment[2], segment[3], segment[4]]);

        if *self.cookie == 0 {
            self.cookie.set_value(message_cookie);
        } else if *self.state == Kcp2KConnectionStates::Authenticated && *self.cookie.value() != message_cookie {
            // 如果连接已经通过验证，但是收到了带有不同 cookie 的消息，那么这可能是由于客户端的 Hello 消息被多次传输，或者攻击者尝试进行 UDP 欺骗。
            let err = Kcp2KError::InvalidReceive(format!(
                "{}: Dropped message with invalid cookie: {:?} from {:?} expected: {:?} state: {:?}. This can happen if the client's Hello message was transmitted multiple times, or if an attacker attempted UDP spoofing.",
                std::any::type_name::<Self>(),
                message_cookie,
                self.client_sock_addr.clone(),
                self.cookie,
                self.state
            ));
            self.on_error(err.clone());
            self.send_disconnect();
            return Err(err);
        }

        // 消息
        let kcp_data = &segment[5..];

        // 更新最后接收时间
        self.last_recv_time.set_value(self.watch.elapsed());

        // 根据通道类型处理消息
        match Kcp2KChannel::from(segment[0]) {
            Kcp2KChannel::Reliable => self.raw_input_reliable(kcp_data),
            Kcp2KChannel::Unreliable => self.raw_input_unreliable(kcp_data),
            _ => {
                let err = Kcp2KError::Unexpected(format!("{}: Received message with unexpected channel. Disconnecting the connection.", std::any::type_name::<Self>()));
                self.on_error(err.clone());
                Err(err)
            }
        }
    }

    pub(crate) fn tick_incoming(&self) {
        // 获取经过的时间
        let elapsed_time = self.watch.elapsed();
        // 根据状态处理不同的逻辑
        match self.state.value() {
            Kcp2KConnectionStates::Connected => self.tick_incoming_connected(elapsed_time),
            Kcp2KConnectionStates::Authenticated => self.tick_incoming_authenticated(elapsed_time),
            _ => {}
        }
    }

    pub(crate) fn tick_outgoing(&self) {
        match self.state.value() {
            Kcp2KConnectionStates::Connected | Kcp2KConnectionStates::Authenticated => {
                let _ = self.kcp.value_mut().update(self.watch.elapsed().as_millis() as u32);
            }
            _ => {}
        }
    }
}

#[allow(unused)]
impl Kcp2kConnection {
    pub fn send_data(&self, data: &[u8], channel: Kcp2KChannel) -> Result<(), Kcp2KError> {
        // 如果数据为空，则返回错误
        if data.is_empty() {
            let err = Kcp2KError::InvalidSend("send_data: tried sending empty message. This should never happen. Disconnecting.".to_string());
            self.on_error(err.clone());
            return Err(err);
        }
        // 根据通道类型发送数据
        match channel {
            Kcp2KChannel::Reliable => self.send_reliable(Kcp2KReliableHeader::Data, data),
            Kcp2KChannel::Unreliable => self.send_unreliable(Kcp2KUnreliableHeader::Data, data),
            _ => {
                let err = Kcp2KError::InvalidSend("send_data: channel disconnected.".to_string());
                self.on_error(err.clone());
                Err(err)
            }
        }
    }

    // 获取连接 ID
    pub fn connection_id(&self) -> u64 {
        self.id
    }

    // 获取本地地址
    pub fn local_address(&self) -> String {
        match self.kcp2k_mode.value() {
            Kcp2KMode::Client => match self.client_sock_addr.as_socket() {
                None => "".to_string(),
                Some(socket) => socket.to_string(),
            },
            Kcp2KMode::Server => match self.socket.local_addr() {
                Ok(addr) => match addr.as_socket() {
                    None => "".to_string(),
                    Some(socket) => socket.to_string(),
                },
                Err(_) => "".to_string(),
            },
        }
    }

    // 获取远程地址
    pub fn remote_address(&self) -> String {
        match self.kcp2k_mode.value() {
            Kcp2KMode::Client => match self.socket.peer_addr() {
                Ok(addr) => match addr.as_socket() {
                    None => "".to_string(),
                    Some(socket) => socket.to_string(),
                },
                Err(_) => "".to_string(),
            },
            Kcp2KMode::Server => match self.client_sock_addr.as_socket() {
                None => "".to_string(),
                Some(socket) => socket.to_string(),
            },
        }
    }
}

impl Kcp2kConnection {
    fn on_authenticated(&self) {
        if *self.kcp2k_mode == Kcp2KMode::Server {
            self.send_hello();
        }
        self.state.set_value(Kcp2KConnectionStates::Authenticated);
        self.on_connected();
    }

    fn on_connected(&self) {
        (self.callback_func)(
            self,
            Callback {
                r#type: CallbackType::OnConnected,
                conn_id: self.id,
                ..Default::default()
            },
        );
    }

    fn on_data(&self, data: &[u8], kcp2k_channel: Kcp2KChannel) {
        (self.callback_func)(
            self,
            Callback {
                r#type: CallbackType::OnData,
                data: data.to_vec(),
                channel: kcp2k_channel,
                conn_id: self.id,
                ..Default::default()
            },
        );
    }

    fn on_error(&self, error: Kcp2KError) {
        (self.callback_func)(
            self,
            Callback {
                r#type: CallbackType::OnError,
                conn_id: self.id,
                error,
                ..Default::default()
            },
        );
    }

    fn on_disconnected(&self) {
        // 如果连接已经断开，则不执行任何操作
        if *self.state == Kcp2KConnectionStates::Disconnected {
            return;
        }
        // 发送断开连接通知
        self.send_disconnect();
        // 设置状态为断开
        self.state.set_value(Kcp2KConnectionStates::Disconnected);
        // 回调
        (self.callback_func)(
            self,
            Callback {
                r#type: CallbackType::OnDisconnected,
                conn_id: self.id,
                ..Default::default()
            },
        );
    }

    // 发送 ping
    fn send_ping(&self) {
        match self.config.is_reliable_ping {
            true => {
                let _ = self.send_reliable(Kcp2KReliableHeader::Ping, Default::default());
            }
            false => {
                let _ = self.send_unreliable(Kcp2KUnreliableHeader::Ping, Default::default());
            }
        }
    }

    // 发送断开连接通知
    fn send_disconnect(&self) {
        for _ in 0..5 {
            let _ = self.send_unreliable(Kcp2KUnreliableHeader::Disconnect, Default::default());
        }
    }

    fn send_reliable(&self, kcp2k_header_reliable: Kcp2KReliableHeader, data: &[u8]) -> Result<(), Kcp2KError> {
        // 创建一个缓冲区，用于存储消息内容
        let mut buffer = vec![];

        // 写入通道头部
        buffer.push(kcp2k_header_reliable.into());

        // 写入数据
        if !data.is_empty() {
            buffer.extend_from_slice(&data);
        }

        // 通过 KCP 发送处理
        match self.kcp.value_mut().send(&buffer) {
            Ok(_) => Ok(()),
            Err(e) => {
                let err = Kcp2KError::InvalidSend(format!("{}: 发送失败，错误码={}，内容长度={}", "send_reliable", e, data.len()));
                self.on_error(err.clone());
                Err(err)
            }
        }
    }

    fn raw_send(&self, data: &[u8]) -> Result<(), Kcp2KError> {
        match self.kcp2k_mode.value() {
            Kcp2KMode::Client => match self.socket.send(&data) {
                Ok(_) => Ok(()),
                Err(e) => Err(Kcp2KError::SendError(e.to_string())),
            },
            Kcp2KMode::Server => match self.socket.send_to(&data, &self.client_sock_addr) {
                Ok(_) => Ok(()),
                Err(e) => Err(Kcp2KError::SendError(e.to_string())),
            },
        }
    }

    fn send_unreliable(&self, kcp2k_header_unreliable: Kcp2KUnreliableHeader, data: &[u8]) -> Result<(), Kcp2KError> {
        // 创建一个缓冲区，用于存储消息内容
        let mut buffer = vec![];

        // 写入通道头部
        buffer.push(Kcp2KChannel::Unreliable.into());

        // 写入握手 cookie 以防止 UDP 欺骗
        buffer.extend_from_slice(&self.cookie.to_le_bytes());

        // 写入 kcp 头部
        buffer.push(kcp2k_header_unreliable.into());

        // 写入数据
        if !data.is_empty() {
            buffer.extend_from_slice(&data);
        }

        //  send it raw
        self.raw_send(&buffer)
    }

    // 处理 ping
    fn handle_ping(&self, elapsed_time: Duration) {
        if elapsed_time >= *self.last_send_ping_time + Duration::from_millis(Kcp2KConfig::PING_INTERVAL) {
            self.last_send_ping_time.set_value(elapsed_time);
            self.send_ping();
        }
    }

    // 处理超时
    fn handle_timeout(&self, elapsed_time: Duration) {
        if elapsed_time > *self.last_recv_time + Duration::from_millis(self.config.timeout) {
            self.on_error(Kcp2KError::Timeout("timeout to disconnected.".to_string()));
            self.on_disconnected();
        }
    }

    // 处理 dead_link
    fn handle_dead_link(&self) {
        if self.kcp.is_dead_link() {
            self.on_error(Kcp2KError::Timeout("dead link to disconnecting.".to_string()));
            self.on_disconnected();
        }
    }

    // 处理可靠消息
    fn raw_input_reliable(&self, data: &[u8]) -> Result<(), Kcp2KError> {
        if let Err(e) = self.kcp.value_mut().input(&data) {
            let err = Kcp2KError::InvalidReceive(format!("[KCP2K] {}: Input failed with error={:?} for buffer with length={}", std::any::type_name::<Self>(), e, data.len() - 1));
            self.on_error(err.clone());
            return Err(err);
        }
        Ok(())
    }

    // 处理不可靠消息
    fn raw_input_unreliable(&self, data: &[u8]) -> Result<(), Kcp2KError> {
        // 至少需要一个字节用于 header
        if data.len() < 1 {
            return Err(Kcp2KError::InvalidReceive(format!("{}: Received unreliable message with invalid length={}. Disconnecting the connection.", std::any::type_name::<Self>(), data.len())));
        }
        // 安全地提取标头。攻击者可能会发送超出枚举范围的值。
        let header = data[0];

        // 判断 header 类型
        let header = Kcp2KUnreliableHeader::from(header);

        // 提取数据
        let data = &data[1..];

        // 根据头部类型处理消息
        match header {
            Kcp2KUnreliableHeader::Data => match self.state.value() {
                Kcp2KConnectionStates::Authenticated => {
                    self.on_data(data, Kcp2KChannel::Unreliable);
                    Ok(())
                }
                _ => {
                    let err = Kcp2KError::InvalidReceive(format!("{}: Received Data message while not Authenticated. Disconnecting the connection.", std::any::type_name::<Self>()));
                    self.on_error(err.clone());
                    Err(err)
                }
            },
            Kcp2KUnreliableHeader::Disconnect => {
                self.on_disconnected();
                Ok(())
            }
            Kcp2KUnreliableHeader::Ping => Ok(()),
        }
    }

    // 接收下一个可靠消息
    fn receive_next_reliable(&self) -> Option<(Kcp2KReliableHeader, Vec<u8>)> {
        // 用于存储接收到的数据
        let mut buffer = Vec::new();
        // 初始化 buffer 大小
        match self.kcp.peeksize() {
            Ok(size) => {
                buffer.resize(size, 0);
            }
            Err(_) => {
                return None;
            }
        }
        // 从 KCP 接收数据
        match self.kcp.value_mut().recv(&mut buffer) {
            Ok(size) => {
                if size == 0 {
                    self.on_error(Kcp2KError::InvalidReceive(format!("{}: Receive failed with error={}. closing connection.", std::any::type_name::<Self>(), size)));
                    self.send_disconnect();
                    return None;
                }
                // 解析头部
                let header_byte = buffer[0];

                // 从 buffer 中提取消息
                Some((Kcp2KReliableHeader::from(header_byte), buffer[1..size].to_vec()))
            }
            Err(error) => {
                self.on_error(Kcp2KError::InvalidReceive(format!("[KCP-2K] connection - {}: Receive failed with error={}. closing connection.", std::any::type_name::<Self>(), error)));
                self.send_disconnect();
                None
            }
        }
    }

    // 处理连接
    fn tick_incoming_connected(&self, elapsed_time: Duration) {
        self.handle_timeout(elapsed_time);
        self.handle_dead_link();
        self.handle_ping(elapsed_time);

        if let Some((header, _)) = self.receive_next_reliable() {
            match header {
                Kcp2KReliableHeader::Hello => {
                    self.on_authenticated();
                }
                Kcp2KReliableHeader::Data => {
                    self.on_error(Kcp2KError::InvalidReceive("Received invalid header while Connected. Disconnecting the connection.".to_string()));
                    self.on_disconnected();
                }
                _ => {}
            }
        }
    }

    // 处理认证过的连接
    fn tick_incoming_authenticated(&self, elapsed_time: Duration) {
        self.handle_timeout(elapsed_time);
        self.handle_dead_link();
        self.handle_ping(elapsed_time);

        if let Some((header, data)) = self.receive_next_reliable() {
            match header {
                Kcp2KReliableHeader::Hello => {
                    self.on_error(Kcp2KError::InvalidReceive("Received invalid header while Authenticated. Disconnecting the connection.".to_string()));
                    self.on_disconnected();
                }
                Kcp2KReliableHeader::Data => {
                    if data.is_empty() {
                        self.on_error(Kcp2KError::InvalidReceive("Received empty Data message while Authenticated. Disconnecting the connection.".to_string()));
                        self.on_disconnected();
                    } else {
                        self.on_data(&data, Kcp2KChannel::Reliable);
                    }
                }
                _ => {}
            }
        }
    }
}
