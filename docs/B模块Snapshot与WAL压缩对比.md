# B模块 Snapshot 与 WAL 压缩对比

## 1. 版本与用途

当前发布版只保留高级持久化实现：WAL 版本号、连续序号、Snapshot CRC32 和安全 `compact()`。基础实现与统一 benchmark 不进入最终项目目录，统一保存在独立实验分支。

历史版本也通过 Git 固定：

| 用途 | Git 位置 |
| --- | --- |
| 基础持久化 | 标签 `b-storage-basic-v1` |
| Snapshot 创新版 | 标签 `b-storage-snapshot-v1` |
| 创新代码与原始结果 | 分支 `feature/b-snapshot-compaction` |
| 基础版 + 提高版统一对比 | 分支 `experiment/b-storage-comparison-unified` |

## 2. 固定实验输入

```text
同步 SET：2,000 次
最终有效键：100 个
键选择：循环覆盖 key:0000 ～ key:0099
完整运行：5 轮
每轮恢复测量：7 次并取中位数
构建：release
工具链：stable-x86_64-pc-windows-gnu
```

基础版和创新版执行相同的键、值和写入顺序。每轮使用新的临时目录，写入和恢复后都校验 100 个最终键值。

如需重新运行统一实验，请进入独立对比工作树或切换到 `experiment/b-storage-comparison-unified` 后执行：

```powershell
cd D:\rustwork\Rust_KV_b_compare
cargo +stable-x86_64-pc-windows-gnu run --release --example storage_benchmark -- 2000 100
```

前两个参数分别是写入次数和循环覆盖后的有效键数。实验分支中的程序会用同一输入依次运行基础版与提高版，并打印压缩前后文件大小、写入耗时、恢复耗时和 `compact()` 耗时。当前正式目录不提供该 example。

## 3. 实测中位数

测试时间：2026-09-02。原始数据见 [b_compaction_metrics.json](b_compaction_metrics.json) 和 [b_compaction_samples.csv](b_compaction_samples.csv)。

| 指标 | 基础版 | 创新版压缩前 | 创新版压缩后 |
| --- | ---: | ---: | ---: |
| 持久化文件大小 | 166.0 KiB | 233.3 KiB | 4.4 KiB |
| 启动恢复 | 1.8 ms | 3.3 ms | 1.1 ms |
| `compact()` 暂停 | — | — | 12.0 ms |
| 2,000 次正常写入 | 4,847.9 ms | 4,631.3 ms | 与压缩前相同 |

## 4. 如何解释

- 新版 WAL 增加 `version + seq`，所以未压缩时比基础记录大约多 40.5%。
- `compact()` 把 2,000 条修改历史收敛为 100 个最终键值，创新版自身磁盘占用减少 98.1%。
- 压缩后的 Snapshot 相对基础 WAL 少 97.4%，本机恢复时间从 1.8 ms 降至 1.1 ms。
- 单次压缩会产生约 12.0 ms 暂停，因此当前采用手动触发，不在普通请求中自动执行。
- 写入时间的 -4.5% 属于本机波动，不能表述为写入优化；主要收益是空间和恢复工作量。

前端图表使用这些已经落盘的历史实测数据，并明确标记为“历史实测”，不会把它伪装成当前运行实时生成的指标。当前服务器旁边另行展示真实 WAL、Snapshot、序号和本次 `compact()` 结果。

## 5. 安全压缩流程

```text
同步并关闭当前 WAL 写入器
→ 写 kv.snapshot.tmp
→ flush + sync_data
→ 重新读取临时快照并校验 CRC32 与最终数据
→ 旧快照改名为 .bak
→ .tmp 发布为正式 .snapshot
→ 截断并同步已被快照覆盖的 WAL
→ 重新打开 WAL 追加写入器
```

服务器重启时先加载 Snapshot，再只重放 `seq > last_seq` 的 WAL。正式快照缺失而 `.bak` 存在时会恢复备份；未发布的 `.tmp` 不参与恢复。
