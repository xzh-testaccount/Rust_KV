# Rust 多客户端键值存储系统

这是一个使用 Rust 实现的、支持多客户端 TCP 访问和文件持久化的键值存储课程项目。服务器维护一份共享的键值数据，命令行客户端负责把用户命令发送到服务器并显示结果。

当前文档冻结了实现和验收口径。Windows 发布版真实验收已于 2026-08-31 完成，结果见[测试、环境与演示](../docs/测试环境与演示.md)；Linux 仍需在目标环境单独复核。

## 功能范围

- 写入或覆盖键值：`set`
- 查询：`get`
- 删除：`delete`
- 按字典序列出全部键：`keys`
- 查看键数量：`status`
- 检查连接：`ping`
- 正常退出当前客户端连接：`quit`
- TCP 多客户端并发访问
- 追加式 WAL 文件持久化，服务器重启后恢复数据

## 环境

必要工具是 Rust 工具链和 Cargo。Windows 工作区已实测可执行 `rustc 1.98.0` 和 `cargo 1.98.0`；Linux 运行边界尚未实测，最终验收时需要在目标 Linux 环境重新执行构建和测试。CLion、RustRover、VS Code 等仅为可选开发工具。

检查工具链：

```powershell
rustc --version
cargo --version
```

需要安装或切换稳定工具链时：

```powershell
rustup toolchain install stable
rustup default stable
```

## 构建与测试

以下命令用于复核发布版；自动化测试记录和 Windows 真实验收结果见[测试、环境与演示](../docs/测试环境与演示.md)：

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build
```

截至 2026-08-31，Windows 发布版自动化验收为 **41 passed、0 failed、0 ignored**；分项为 library 1、client 6、network 15、persistence 6、protocol_storage 13。

## 启动方式

服务器默认监听 `127.0.0.1:7878`，默认数据文件是相对于当前工作目录的 `data/kv.wal`：

```powershell
cargo run --bin kv-server -- --bind 127.0.0.1:7878 --data data/kv.wal
```

客户端默认连接同一地址：

```powershell
cargo run --bin kv-client -- --server 127.0.0.1:7878
```

服务器使用 `--bind HOST:PORT` 覆盖监听地址，客户端使用 `--server HOST:PORT` 覆盖服务器地址；服务器用 `--data PATH` 覆盖数据文件。启动前应确保数据文件的父目录可创建或已经存在。

## 客户端命令

客户端交互命令如下。`KEY` 不得包含空白字符或控制字符；`VALUE` 是一个非空文本参数，可以包含空格，但含空格时必须使用 JSON 字符串引号。

```text
set KEY VALUE
get KEY
delete KEY
keys
status
ping
quit
```

例如：

```text
set user:1 Alice
get user:1
set greeting "hello rust"
keys
status
delete user:1
quit
```

协议层使用 JSON Lines，客户端会把上述交互命令转换为协议请求；协议字段和错误码以[协议与持久化设计](../docs/协议与持久化设计.md)为准。

## 固定限制

| 项目 | 规定 |
| --- | --- |
| 默认地址 | `127.0.0.1:7878` |
| 默认数据文件 | `data/kv.wal` |
| 单帧大小 | 64 KiB（不含末尾 LF） |
| 键最大长度 | 256 字节（UTF-8 编码后） |
| 值最大长度 | 16 KiB（UTF-8 编码后） |
| 键排序 | `BTreeMap` 字典序 |
| 并发模型 | 每连接一个线程，共享 `Arc<Mutex<PersistentStore>>` |

## 文档

- [需求规格与验收](../docs/需求规格与验收.md)：七阶段目标、验收矩阵和提交检查。
- [架构设计](../docs/架构设计.md)：模块边界、数据流、线程和锁范围。
- [协议与持久化设计](../docs/协议与持久化设计.md)：请求响应格式、错误码、WAL 和恢复规则。
- [测试、环境与演示](../docs/测试环境与演示.md)：测试案例、运行环境、双客户端和重启恢复演示。

## 提交前清理

提交内容不应包含 `target/`、运行时生成的 `data/kv.wal`、临时日志或编辑器缓存。演示产生的数据应在提交前移出或删除；如果数据文件需要作为测试样例提交，应放在明确命名的测试资源目录并在文档中说明用途。
