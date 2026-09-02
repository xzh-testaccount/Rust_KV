# B模块基础版与创新版对比

## 1. 版本组织

本次提高项没有直接混入团队稳定版本，而是使用Git标签、分支和独立工作树分开管理。

| 用途 | Git位置 | 本机目录 |
| --- | --- | --- |
| 团队稳定版 | `main`，标签 `b-storage-basic-v1` | `D:\rustwork\Rust_KV_basic_bench`（只用于复测） |
| B创新版 | `feature/b-snapshot-compaction` | `D:\Rust_KV` |

基础标签固定了“内存存储 + CRC32 WAL + 启动恢复”和公共基准程序。创新分支增加WAL版本号、操作序号、Snapshot和安全压缩。两个版本没有远程推送，团队可以先评审创新版，再决定是否合并。

创新分支保留 `src/persistence.rs` 作为基础实现对照，`src/lib.rs` 使用 `#[path = "persistence_advanced.rs"]` 把公开的 `persistence` 模块指向 `src/persistence_advanced.rs`。因此C的导入路径和调用方式不变。

## 2. 不影响C的接口约定

C仍然只需要：

```rust
use rust_kv_store::persistence::PersistentStore;

let store = PersistentStore::open("data/kv.wal")?;
```

以下已有方法的名称、参数和返回值保持不变：

```text
open / set / get / delete / keys / len / is_empty / stats / wal_path
```

创新版只增加了三个可选入口：

```text
compact          手动生成快照并清空已被快照覆盖的WAL
snapshot_path    查看快照路径
last_sequence    查看最后一条成功修改的序号
```

C可以完全不调用这些新增入口，原来的 `Arc<Mutex<PersistentStore>>` 集成代码仍能工作。课程演示时可由B的测试或基准程序单独调用 `compact()`，不要求A增加协议命令，也不要求C修改网络层。

## 3. 创新版做了什么

### 3.1 WAL版本号和连续序号

基础记录只校验操作内容：

```json
{"record":{"op":"set","key":"course","value":"Rust"},"crc32":"B033579D"}
```

创新记录把版本、序号和操作一起纳入CRC32：

```json
{"payload":{"version":1,"seq":1,"record":{"op":"set","key":"course","value":"Rust"}},"crc32":"CA96C85E"}
```

- `version`让以后升级文件格式时能够明确拒绝不支持的版本。
- `seq`必须从正确起点开始并严格递增，可发现中间记录被删除、重复或调换顺序。
- CRC32覆盖整个 `payload`，修改版本号、序号、键、值或操作都会导致校验失败。
- 旧版带CRC32的WAL仍可读取；继续写入时会从正确的下一个序号开始，不会偷偷改写旧文件。

### 3.2 Snapshot快照

快照只保存某个序号对应的最终状态，不再保存已经被覆盖的所有历史过程：

```text
2000条SET历史记录
        ↓ compact()
100条最终键值 + last_seq=2000
```

快照中的键来自 `BTreeMap`，天然按字典序输出。快照也带版本号和CRC32，启动时还会检查键是否严格有序、是否重复，以及键值是否合法。

### 3.3 安全压缩顺序

`compact()`使用以下顺序：

```text
关闭并同步当前WAL写入器
→ 写入 kv.snapshot.tmp
→ flush + sync_data
→ 重新读取临时快照并验证CRC32和最终数据
→ 原快照改名为 kv.snapshot.bak
→ 临时快照改名为正式快照
→ 截断并同步WAL
→ 重新打开WAL追加写入器
```

关键点是先发布完整快照，再清空WAL。如果进程在“快照已发布、WAL未截断”之间退出，重启会根据快照的 `last_seq` 跳过WAL中已经包含的旧操作，避免重复删除等错误。若正式快照缺失但 `.bak` 存在，启动会先恢复备份；没有发布的 `.tmp` 不会被当成正式数据。

### 3.4 严格损坏处理

创新版继续采用基础版的严格策略：

- 快照或WAL损坏时拒绝启动。
- 错误会指出WAL准确行号或明确标记 `Snapshot损坏`。
- 不自动忽略坏记录，不用空数据覆盖原文件。
- 写入失败后当前存储实例进入只读状态。

## 4. 公平对比方法

两个版本都使用 `examples/storage_benchmark.rs`，负载固定为：

```text
总写入次数：2000
最终保留键：100
键选择方式：循环覆盖同一批100个键
恢复测量：重复打开7次，取中位数
编译方式：release
工具链：stable-x86_64-pc-windows-gnu
```

每个版本完整运行五次，奇偶轮次交换执行顺序；每轮恢复再重复七次。下表对耗时采用五轮结果的中位数。耗时会受电脑负载、磁盘缓存和硬件影响，文件大小是确定值。完整方法、原始样本和彩色图表见 [B日志压缩四项实验](B日志压缩四项实验.md) 和 [自动测评结果](results/b_compaction_result.md)。

## 5. 本机实测结果

测试日期：2026-09-02。基础版实验提交为 `4624d3c`，创新版实验提交为 `1da03a3`。

| 指标 | 基础版 | 创新版压缩前 | 创新版压缩后 |
| --- | ---: | ---: | ---: |
| 2000次同步写入 | 4847.9 ms | 4631.3 ms | 与压缩前相同 |
| 持久化文件大小 | 170000 B | 238893 B | 4473 B |
| 启动恢复中位数 | 1803 μs | 3318 μs | 1088 μs |
| `compact()`暂停中位数 | 无 | 无 | 12020 μs |

创新版自己的压缩前后对比为 `238893 B → 4473 B`，占用减少约98.1%；Snapshot恢复相对创新版压缩前减少约67.2%，相对基础版减少约39.7%。

结果说明：序号和版本字段让压缩前WAL增大约40.5%，也使创新版压缩前恢复慢于基础版；Snapshot压缩则显著降低磁盘占用和恢复工作量。五轮写入中位数相差约4.5%，但方向为创新版略快，这属于同步磁盘写入的运行波动，应解释为“没有观察到明显写入退化”，不能宣称日志压缩提升了正常写入性能。这种同时展示收益、成本和测量边界的结果更适合课程答辩。

## 6. 测试结果

创新版执行：

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

结果为37项测试通过、0项失败，其中新增8项Snapshot专项测试：

- 压缩后重启保持最终状态。
- 压缩后继续写入并保持连续序号。
- 快照已发布但WAL未截断时能够安全恢复。
- 快照内容被修改后CRC32校验失败且原文件不被覆盖。
- 未发布的临时快照不会参与恢复。
- 正式快照缺失时恢复 `.bak`。
- WAL序号断档能定位到准确行。
- 基础版WAL可以读取并继续写入新版记录。

## 7. 复现命令

基础版：

```powershell
cd D:\rustwork\Rust_KV_basic_bench
cargo +stable-x86_64-pc-windows-gnu test
cargo +stable-x86_64-pc-windows-gnu run --release --example storage_benchmark -- 2000 100
```

创新版：

```powershell
cd D:\Rust_KV
cargo test
cargo run --release --example storage_benchmark -- 2000 100
```

答辩时建议先展示两版测试都通过，再展示基准输出，最后打开压缩后的 `.snapshot` 和空 `.wal` 文件说明恢复流程。

## 8. 当前边界

- `compact()`目前是显式调用，没有后台线程和自动阈值，因此不会在C处理请求时突然执行较慢的压缩。
- 单次压缩需要独占 `&mut PersistentStore`；C若以后接入自动压缩，应在同一个 `Mutex` 临界区内调用。
- 当前只支持单服务器进程访问一组文件，没有实现跨进程文件锁。
- CRC32用于发现意外损坏，不是防篡改签名。
- Snapshot解决日志增长和恢复效率问题，TTL过期时间仍作为后续独立创新点，不在本分支修改协议语义。
