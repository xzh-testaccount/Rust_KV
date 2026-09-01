# Rust 多客户端键值存储

根目录是从零重建的、可编译的 Rust 键值存储框架；完整的旧版实现及其测试归档在 [`mvp/`](mvp/)。两套项目相互独立，请按各自的命令构建和测试。

## 五层结构

```text
命令行客户端层  src/bin/client.rs → src/client.rs
协议层          src/protocol.rs
网络服务层      src/bin/server.rs → src/server.rs
存储层          src/storage.rs
持久化层        src/persistence.rs
```

`src/error.rs` 提供跨层共用的错误类型。客户端负责命令行交互，协议层负责 JSON Lines 契约，网络服务层负责 TCP 连接生命周期，存储层负责键值接口，持久化层负责 WAL 记录与恢复。

## 当前状态

根项目目前只完成模块边界、数据契约、配置和入口骨架，尚未实现 TCP 网络服务、帧读取与编解码、WAL 追加/同步/重放等运行能力；`kv-server` 和 `kv-client` 的入口会明确报告未实现状态。因此，根项目是后续开发的框架，不是可用的服务端发布版。

## 构建与测试

在仓库根目录执行：

```powershell
cargo build
cargo test
```

旧版 `mvp` 项目单独执行：

```powershell
cargo build --manifest-path mvp/Cargo.toml
cargo test --manifest-path mvp/Cargo.toml
```

## 设计文档

- [架构设计](docs/架构设计.md)：五层职责、请求数据流、并发与锁范围。
- [协议与持久化设计](docs/协议与持久化设计.md)：JSON Lines 协议、错误码、WAL 格式与恢复规则。
- [旧版实现说明](mvp/README.md)：完整实现的功能、运行和验收记录。
