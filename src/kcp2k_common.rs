#![allow(unused)]

use crate::kcp2k::Kcp2KMode;
use crate::kcp2k_config::Kcp2KConfig;
use crate::kcp2k_connection::Kcp2kConnection;
use log::info;
use revel_cell::arc::Arc;
use socket2::{SockAddr, Socket};
use std::fmt::{Display, Formatter};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Error;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(u8)]
pub(crate) enum Kcp2KConnectionStates {
    None = 0,
    Authenticated = 1,
    Connected = 2,
    Disconnected = 3,
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(u8)]
pub(crate) enum Kcp2KReliableHeader {
    None = 0,
    Hello = 1,
    Ping = 2,
    Data = 3,
}
impl Into<u8> for Kcp2KReliableHeader {
    fn into(self) -> u8 {
        self as u8
    }
}
impl From<u8> for Kcp2KReliableHeader {
    fn from(value: u8) -> Self {
        match value {
            1 => Kcp2KReliableHeader::Hello,
            2 => Kcp2KReliableHeader::Ping,
            3 => Kcp2KReliableHeader::Data,
            _ => Kcp2KReliableHeader::None,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(u8)]
pub(crate) enum Kcp2KUnreliableHeader {
    Data = 4,
    Disconnect = 5,
    Ping = 6,
}
impl Into<u8> for Kcp2KUnreliableHeader {
    fn into(self) -> u8 {
        self as u8
    }
}
impl From<u8> for Kcp2KUnreliableHeader {
    fn from(value: u8) -> Self {
        match value {
            4 => Kcp2KUnreliableHeader::Data,
            5 => Kcp2KUnreliableHeader::Disconnect,
            6 => Kcp2KUnreliableHeader::Ping,
            _ => Kcp2KUnreliableHeader::Disconnect,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(u8)]
pub enum Kcp2KChannel {
    None = 0,
    Reliable = 1,
    Unreliable = 2,
}

impl Into<u8> for Kcp2KChannel {
    fn into(self) -> u8 {
        self as u8
    }
}

impl From<u8> for Kcp2KChannel {
    fn from(value: u8) -> Self {
        match value {
            1 => Kcp2KChannel::Reliable,
            2 => Kcp2KChannel::Unreliable,
            _ => Kcp2KChannel::Reliable,
        }
    }
}

// 定义一个枚举来封装不同的错误类型。
#[derive(Clone)]
pub enum Kcp2KError {
    None(String),               // 无错误
    DnsResolve(String),         // 无法解析主机名
    Timeout(String),            // ping 超时或失效链接
    Congestion(String),         // 超出传输/网络可以处理的消息数
    InvalidReceive(String),     // RECV 无效数据包（可能是故意攻击）
    InvalidSend(String),        // 用户尝试发送无效数据
    ConnectionClosed(String),   // 连接自愿关闭或非自愿丢失
    Unexpected(String),         // 意外错误/异常，需要修复。
    SendError(String),          // 发送数据失败
    ConnectionNotFound(String), // 未找到连接
}

impl Display for Kcp2KError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Kcp2KError::None(msg) => write!(f, "None: {}", msg),
            Kcp2KError::DnsResolve(msg) => write!(f, "DnsResolve: {}", msg),
            Kcp2KError::Timeout(msg) => write!(f, "Timeout: {}", msg),
            Kcp2KError::Congestion(msg) => write!(f, "Congestion: {}", msg),
            Kcp2KError::InvalidReceive(msg) => write!(f, "InvalidReceive: {}", msg),
            Kcp2KError::InvalidSend(msg) => write!(f, "InvalidSend: {}", msg),
            Kcp2KError::ConnectionClosed(msg) => write!(f, "ConnectionClosed: {}", msg),
            Kcp2KError::Unexpected(msg) => write!(f, "Unexpected: {}", msg),
            Kcp2KError::SendError(msg) => write!(f, "SendError: {}", msg),
            Kcp2KError::ConnectionNotFound(msg) => write!(f, "ConnectionNotFound: {}", msg),
        }
    }
}

impl Default for Kcp2KError {
    fn default() -> Self {
        Kcp2KError::None("None".to_string())
    }
}
pub type CallbackFuncType = fn(&Kcp2kConnection, Callback);

#[derive(Debug)]
pub enum CallbackType {
    OnConnected,
    OnData,
    OnError,
    OnDisconnected,
}
// Callback: 服务器回调
pub struct Callback {
    pub r#type: CallbackType,
    pub conn_id: u64,
    pub channel: Kcp2KChannel,
    pub data: Vec<u8>,
    pub error: Kcp2KError,
}

impl Display for Callback {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.r#type {
            CallbackType::OnConnected => {
                write!(f, "OnConnected: id {} ", self.conn_id)
            }
            CallbackType::OnData => {
                write!(f, "OnData: id {} {:?} {:?}", self.conn_id, self.channel, self.data.to_vec())
            }
            CallbackType::OnDisconnected => {
                write!(f, "OnDisconnected: id {}", self.conn_id)
            }
            CallbackType::OnError => {
                write!(f, "OnError: id {} - {}", self.conn_id, self.error)
            }
        }
    }
}

impl Default for Callback {
    fn default() -> Self {
        Self {
            r#type: CallbackType::OnError,
            conn_id: 0,
            channel: Kcp2KChannel::None,
            data: Vec::new(),
            error: Kcp2KError::default(),
        }
    }
}

pub(crate) fn configure_socket_buffers(socket: &Socket, config: &Kcp2KConfig) -> Result<(), Error> {
    // 记录初始大小以进行比较
    let initial_receive = socket.recv_buffer_size()?;
    let initial_send = socket.send_buffer_size()?;

    // 设置为配置的大小
    socket.set_recv_buffer_size(config.recv_buffer_size)?;
    socket.set_send_buffer_size(config.send_buffer_size)?;

    info!(
        "[KCP2K] RecvBuf = {}=>{} ({}x) SendBuf = {}=>{} ({}x)",
        initial_receive,
        socket.recv_buffer_size()?,
        socket.recv_buffer_size()? / initial_receive,
        initial_send,
        socket.send_buffer_size()?,
        socket.send_buffer_size()? / initial_send
    );
    Ok(())
}

// sock_addr hash
pub(crate) fn connection_hash(sock_addr: &SockAddr) -> u64 {
    // cookie 与 sock_addr 一起生成一个唯一的连接 ID
    let mut hasher = DefaultHasher::new();
    sock_addr.hash(&mut hasher);
    hasher.finish()
}

// 生成一个随机的 4 字节 cookie
pub(crate) fn generate_cookie() -> u32 {
    let start = SystemTime::now();
    let since_the_epoch = start.duration_since(UNIX_EPOCH).expect("Time went backwards");

    let nanos = since_the_epoch.as_nanos(); // 获取纳秒级时间戳 (u128)

    // 将 u128 纳秒时间戳转换为 u32，同时引入一些位操作增加“随机性”
    // 目标是让结果在 u32 的 0 到 u32::MAX 范围内尽可能分散
    // 这里的具体位操作可以根据需求调整，以最大化“混乱度”
    let cookie_val = (nanos as u32)
        ^ ((nanos >> 32) as u32) // 将 u128 的高位与低位异或
        ^ ((nanos >> 64) as u32)
        ^ ((nanos >> 96) as u32);

    cookie_val
}
