use crate::kcp2k_common::{configure_socket_buffers, CallbackFuncType, Kcp2KError};
use crate::kcp2k_config::Kcp2KConfig;
use revel_cell::arc::Arc;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use std::mem::MaybeUninit;

#[derive(Debug, PartialEq, Copy, Clone)]
#[repr(u8)]
#[allow(unused)]
pub enum Kcp2KMode {
    Client,
    Server,
}

#[allow(unused)]
pub struct Kcp2K {
    pub(crate) config: Arc<Kcp2KConfig>,
    pub(crate) socket: Arc<Socket>,
    pub(crate) callback_func: CallbackFuncType,
}

impl Kcp2K {
    pub(crate) fn raw_receive_from(&self) -> Option<(SockAddr, Vec<u8>)> {
        // 1. 申请接收缓冲区（MTU）
        let mut buf: Vec<MaybeUninit<u8>> = Vec::with_capacity(self.config.mtu);

        unsafe {
            buf.set_len(self.config.mtu); // 必须
        }

        // 2. 调用 socket2 recv_from（官方签名）
        let (size, addr) = match self.socket.recv_from(&mut buf) {
            Ok(x) => x,
            Err(_) => return None,
        };

        // 3. 将 MaybeUninit 转成 &[u8]（官方安全惯用法）
        let data = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, size) };

        // 4. 转成 Bytes（必须 copy，但只复制一次）
        Some((addr, data.to_vec()))
    }
}

#[allow(unused)]
impl Kcp2K {
    pub fn new(config: Kcp2KConfig, callback: CallbackFuncType) -> Self {
        let domain = match config.dual_mode {
            true => Domain::IPV6,
            false => Domain::IPV4,
        };
        let socket = match Socket::new(domain, Type::DGRAM, Some(Protocol::UDP)) {
            Ok(v) => v,
            Err(e) => panic!("{}", Kcp2KError::Unexpected(e.to_string())),
        };
        if let Err(e) = configure_socket_buffers(&socket, &config) {
            panic!("{}", Kcp2KError::Unexpected(e.to_string()));
        }
        if let Err(e) = socket.set_nonblocking(true) {
            panic!("{}", Kcp2KError::Unexpected(e.to_string()));
        }

        let kcp2k = Self {
            config: Arc::new(config),
            socket: Arc::new(socket),
            callback_func: callback,
        };

        kcp2k
    }

    pub fn stop(&self) -> Result<(), Kcp2KError> {
        match self.socket.shutdown(std::net::Shutdown::Both) {
            Ok(_) => Ok(()),
            Err(e) => Err(Kcp2KError::Unexpected(e.to_string())),
        }
    }
}
