# B日志压缩四项对比实验

## 1. 实验目标

实验只测B存储模块，不经过网络层。统一比较以下四项：

1. 压缩前后持久化文件大小。
2. 基础版WAL、创新版压缩前WAL和Snapshot的启动恢复时间。
3. `compact()`造成的暂停时间。
4. 基础版与创新版的正常同步写入时间。

## 2. 测试输入

正式测评默认使用：

| 参数 | 数值 | 说明 |
| --- | ---: | --- |
| `Operations` | 2000 | 连续执行2000次同步SET |
| `LiveKeys` | 100 | 循环覆盖100个键，最终保留100组数据 |
| `Runs` | 5 | 完整实验执行5轮，取中位数 |
| 恢复重复 | 7 | 每轮分别打开存储7次，取中位数 |
| 编译模式 | release | 排除debug构建开销 |
| 工具链 | Windows GNU | 两版使用相同工具链 |

键固定为`key:0000`到`key:0099`，值固定为递增的`value:XXXXXXXX`。循环覆盖会产生大量已经失效的历史版本，适合验证日志压缩。每轮使用新的临时目录，并在写入后和每次恢复后校验全部最终键值。

## 3. 对比组

| 组别 | 内容 | 主要用途 |
| --- | --- | --- |
| A基础版 | 旧格式WAL，不支持压缩 | 原始基准 |
| B创新版压缩前 | 带版本号和序号的新WAL | 测量新格式成本 |
| C创新版压缩后 | Snapshot + 空WAL | 测量压缩收益 |

比较关系：

```text
A vs B：version、seq和更大CRC32范围的成本
B vs C：Snapshot日志压缩本身的收益
A vs C：从基础版升级后的总体结果
```

## 4. 一键测评指令

在创新版目录执行：

```powershell
cd D:\Rust_KV
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\run_b_compaction_benchmark.ps1
```

自定义输入：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass `
  -File .\scripts\run_b_compaction_benchmark.ps1 `
  -Operations 5000 `
  -LiveKeys 200 `
  -Runs 7
```

如果基础版工作树不在默认目录，可以增加：

```powershell
-BasicRepo "其他基础版目录"
```

## 5. 输出文件

脚本自动生成：

| 文件 | 用途 |
| --- | --- |
| `docs/results/b_compaction_comparison.png` | 可直接放入Word或PPT的彩色柱状图 |
| `docs/results/b_compaction_comparison.svg` | 无损缩放的原始图表 |
| `docs/results/b_compaction_result.md` | 自动生成的结果表和结论 |
| `docs/results/b_compaction_metrics.json` | 完整输入、版本、中位数和原始样本 |
| `docs/results/b_compaction_samples.csv` | 每轮数据，可用Excel继续绘图 |

## 6. 公平性措施

- 两版都在`PersistentStore::open()`完成后才开始统计写入时间。
- 奇偶轮次交换基础版和创新版的执行顺序，降低缓存、温度和后台任务造成的固定偏差。
- 每一轮都创建全新的WAL和Snapshot目录。
- 文件大小读取真实元数据，不使用估算值。
- 所有耗时采用多轮中位数，不使用最好一次结果。
- 基础版和创新版使用同一个Rust版本、release模式和`sync_data()`策略。

正常写入耗时容易受到磁盘同步波动影响。当两版差异接近0时，应写成“没有观察到明显退化”，不能根据一次正负变化宣称写入被优化。
