# Rust 多客户端键值存储

这是一个从零实现的 Rust 键值存储课程项目。

## 系统结构

```text
答辩网页          prototype/
        ↓ HTTP /api/*
本地实验控制器    src/bin/controller.rs → src/controller.rs
        ↓ TCP JSON Lines / 子进程管理
网络服务层        src/bin/server.rs → src/server.rs
        ↓
协议层            src/protocol.rs
存储层            src/storage.rs
持久化正式入口      src/persistence.rs → src/persistence_advanced.rs
```

`src/client.rs` 还提供命令行客户端。`src/benchmark.rs` 负责真实 TCP 基准测试和可追溯证据包。控制器只监听本机，用来启动、强杀和重启 `kv-server`，并承接浏览器无法直接完成的并发、恢复和性能实验。

## 当前状态

项目已完成内存键值存储和“Snapshot + 增量 WAL”持久化：支持新增、覆盖、查询、删除、有序键列表和数量统计，并统一检查键值长度、空白字符和控制字符。新版 WAL 使用格式版本、连续序号和 CRC32；写入操作会先追加并同步 WAL，再更新内存。服务器启动时先校验并加载 Snapshot，再重放序号更新的 WAL。

当前发布版只保留高级 `persistence::PersistentStore`，支持安全 `compact()`：临时快照写入并复核、原子发布、备份恢复，最后才清理已被快照覆盖的 WAL。基础版与统一对比程序保存在独立分支 `experiment/b-storage-comparison-unified`，不进入最终项目目录和服务器链路。

TCP 服务完整支持半包、粘包、CRLF、非法 UTF-8/JSON、超长帧和连接错误隔离。服务端真实提供四种实验组合：

- `sync + mutex`
- `sync + rwlock`
- `async(Tokio) + mutex`
- `async(Tokio) + rwlock`

四种组合共用相同协议、数据集和 `WAL → flush → sync_data → Memory` 持久化语义。异步模式把阻塞存储操作放入 `spawn_blocking`；`RwLock` 的 GET/KEYS/STATUS/STORAGE_STATUS 使用读锁，SET/DELETE/COMPACT 使用写锁。

答辩网页通过真实 HTTP 控制器接入。答辩模式的 CRUD、并发计数、Kill/Restart、Snapshot + WAL 恢复、实时存储状态、真实 Compact 和 QPS/P50/P95/P99 都来自后端；后端不可达时返回 `BACKEND_UNREACHABLE`，不会自动降级为模拟数据。Performance Lab 提供“快速演示”和“完整实验”两档：快速档使用 `1 / 128 Clients`、每点固定采样 3 秒，完整档保留 `1 / 10 / 50 / 100 Clients`、每轮 10,000 请求、预热和 5 轮正式采样。两档均运行真实 Rust 服务，A/B 切换前必须等待控制器确认实验环境已重置。

Performance Lab 还展示 B 模块基础 WAL 与 Snapshot 压缩版的历史实测柱状图，原始数据保存在 `docs/b_compaction_metrics.json`。历史完整数据与本次实时数据分别标注，不混用、不补造。

协议层已经实现了JSON Lines协议同时定义了客户端-服务端请求/响应数据结构，能够对frame进行流式解析，能够自动处理过大帧，完美区分空帧以及不完全的帧。协议层实现了严格的协议校验，能够拒绝非法的数据流，防止注入或者协议污染。
服务端和客户端实现了异步化的功能，客户端通过tokio实现异步连接服务端，异步打印提示信息，服务端通过tokio实现异步处理客户端的请求，为每一个客户端连接生成了一个独立的异步任务，实现了真正的并发。

## 构建与测试

Windows GNU 工具链下，在仓库根目录执行：

```powershell
cargo +stable-x86_64-pc-windows-gnu fmt --all -- --check
cargo +stable-x86_64-pc-windows-gnu clippy --all-targets --all-features --offline -- -D warnings
cargo +stable-x86_64-pc-windows-gnu test --all-targets --all-features --offline
```

## 启动答辩系统

打开两个 PowerShell 终端。

终端一启动控制器和由它托管的 KV 子进程：

```powershell
cd D:\Rust_KV
powershell -ExecutionPolicy Bypass -File .\scripts\start_backend.ps1
```

终端二启动本地网页：

```powershell
cd D:\Rust_KV
powershell -ExecutionPolicy Bypass -File .\scripts\start_frontend.ps1
```

浏览器打开 `http://127.0.0.1:3000`。页面默认使用深色主题并直接进入“答辩模式 · 后端实测”；空数据文件启动时显示 `0` 个后端键，后端未启动时会真实显示离线，不会先展示模拟数据。控制器为 `127.0.0.1:7879`，主演示 TCP 服务为 `127.0.0.1:7878`。完整操作顺序见[答辩真实后端指南](docs/P-06真实后端验收.md)。

## 设计文档

- [架构设计](docs/架构设计.md)：五层职责、请求数据流、并发与锁范围。
- [协议与持久化设计](docs/协议与持久化设计.md)：JSON Lines 协议、错误码、WAL 格式与恢复规则。
- [B模块存储与持久化说明](docs/B模块存储与持久化说明.md)：内存存储、WAL、CRC32、恢复流程和交接说明。
- [B模块Snapshot与WAL压缩对比](docs/B模块Snapshot与WAL压缩对比.md)：基础/创新代码、实验输入、原始数据和结果解释。
- [网络层设计文档](docs/网络层设计文档.md)：异步客户端和异步服务端核心组件以及功能概述
- [答辩真实后端指南](docs/P-06真实后端验收.md)：启动、接口、四段演示、真实性边界和故障处理。
