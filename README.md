# kcp2k-rust

一个使用 Rust 实现的 KCP 协议库，采用 `revel_cell` 进行线程安全的 cell 管理。

## 简介

`kcp2k-rust` 是一个高性能的 KCP（快速可靠协议）实现，提供了客户端和服务器功能。KCP 是一个快速可靠协议，可以比 TCP 浪费 10%-20% 的带宽的代价，换取平均延迟降低 30%-40%，且最大延迟降低三倍的传输效果。

## 特性

- ✅ **客户端和服务器支持** - 提供完整的客户端和服务器实现
- ✅ **双通道传输** - 支持可靠（Reliable）和不可靠（Unreliable）两种传输通道
- ✅ **线程安全** - 使用 `revel_cell` 实现线程安全的 cell 管理
- ✅ **高度可配置** - 丰富的配置选项，包括缓冲区大小、MTU、窗口大小等
- ✅ **事件回调** - 支持连接、数据接收、错误和断开连接等事件回调
- ✅ **低延迟优化** - 默认启用 NoDelay 模式，支持快速重传和自定义更新间隔

## 安装

在 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
kcp2k-rust = "1.0.0"
```

## 快速开始

### 服务器示例

```rust
use kcp2k_rust::kcp2k_server::Kcp2KServer;
use kcp2k_rust::kcp2k_config::Kcp2KConfig;
use kcp2k_rust::kcp2k_common::{Callback, CallbackType, Kcp2KChannel};
use kcp2k_rust::kcp2k_connection::Kcp2kConnection;

fn callback(conn: &Kcp2kConnection, cb: Callback) {
    match cb.r#type {
        CallbackType::OnConnected => {
            println!("客户端已连接: {}", cb.conn_id);
        }
        CallbackType::OnData => {
            println!("收到数据: {:?}", cb.data);
            // 回显数据
            let _ = conn.send_data(&cb.data, cb.channel);
        }
        CallbackType::OnError => {
            println!("错误: {}", cb.error);
        }
        CallbackType::OnDisconnected => {
            println!("客户端已断开: {}", cb.conn_id);
        }
    }
}

fn main() {
    let config = Kcp2KConfig::default();
    let server = Kcp2KServer::new("0.0.0.0:3100".to_string(), config, callback);
    
    loop {
        server.tick();
    }
}
```

### 客户端示例

```rust
use kcp2k_rust::kcp2k_client::Kcp2KClient;
use kcp2k_rust::kcp2k_config::Kcp2KConfig;
use kcp2k_rust::kcp2k_common::{Callback, CallbackType, Kcp2KChannel};
use kcp2k_rust::kcp2k_connection::Kcp2kConnection;

fn callback(conn: &Kcp2kConnection, cb: Callback) {
    match cb.r#type {
        CallbackType::OnConnected => {
            println!("已连接到服务器");
            // 发送测试数据
            let _ = conn.send_data(b"Hello, Server!".as_slice(), Kcp2KChannel::Reliable);
        }
        CallbackType::OnData => {
            println!("收到服务器数据: {:?}", cb.data);
        }
        CallbackType::OnError => {
            println!("错误: {}", cb.error);
        }
        CallbackType::OnDisconnected => {
            println!("与服务器断开连接");
        }
    }
}

fn main() {
    let config = Kcp2KConfig::default();
    let client = Kcp2KClient::new(config, callback);
    
    client.connect("127.0.0.1:3100".to_string());
    
    loop {
        client.tick();
    }
}
```

## 配置选项

`Kcp2KConfig` 提供了丰富的配置选项：

```rust
pub struct Kcp2KConfig {
    pub dual_mode: bool,              // 是否启用 IPv6/IPv4 双模式
    pub recv_buffer_size: usize,      // 接收缓冲区大小（默认: 7MB）
    pub send_buffer_size: usize,      // 发送缓冲区大小（默认: 7MB）
    pub mtu: usize,                   // 最大传输单元（默认: 1200）
    pub no_delay: bool,               // 是否启用 NoDelay（默认: true）
    pub interval: i32,                // KCP 内部更新间隔，单位毫秒（默认: 10ms）
    pub fast_resend: i32,             // 快速重传参数（默认: 0）
    pub congestion_window: bool,      // 是否启用拥塞窗口（默认: false）
    pub send_window_size: u16,        // 发送窗口大小（默认: 32）
    pub receive_window_size: u16,     // 接收窗口大小（默认: 128）
    pub timeout: u64,                 // 超时时间，单位毫秒（默认: 2000ms）
    pub max_retransmits: u32,         // 最大重传次数（默认: 20）
    pub is_reliable_ping: bool,       // 是否启用可靠 ping（默认: true）
}
```

### 自定义配置示例

```rust
let config = Kcp2KConfig {
    no_delay: true,
    interval: 10,
    fast_resend: 2,
    send_window_size: 64,
    receive_window_size: 256,
    ..Default::default()
};
```

## 传输通道

库支持两种传输通道：

- **Reliable（可靠通道）** - 保证数据按顺序、完整地送达
- **Unreliable（不可靠通道）** - 不保证顺序和完整性，但延迟更低

```rust
// 使用可靠通道发送
conn.send_data(data, Kcp2KChannel::Reliable)?;

// 使用不可靠通道发送
conn.send_data(data, Kcp2KChannel::Unreliable)?;
```

## 回调事件

回调函数会接收以下事件类型：

- `OnConnected` - 连接建立时触发
- `OnData` - 接收到数据时触发
- `OnError` - 发生错误时触发
- `OnDisconnected` - 连接断开时触发

每个回调包含：
- `conn_id` - 连接 ID
- `channel` - 数据通道类型
- `data` - 接收到的数据（OnData 事件）
- `error` - 错误信息（OnError 事件）

## 运行示例

项目包含一个完整的示例程序：

```bash
cargo run --example program
```

## 依赖

- `revel_cell` - 线程安全的 cell 管理
- `socket2` - 底层 socket 操作
- `kcp` - KCP 协议实现
- `log` - 日志记录

## 文档

更多详细信息请参考 [API 文档](https://docs.rs/kcp2k-rust)。

## 许可证

本项目采用许可证文件 `LICENSE` 中指定的许可证。

## 贡献

欢迎提交 Issue 和 Pull Request！

## 作者

- xintujing

## 仓库

- GitHub: https://github.com/xintujing/kcp2k-rust

