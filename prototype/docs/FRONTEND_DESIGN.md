# RustKV Lab 前端设计规范

> 现状基线：2026-09-02。本文依据 `prototype/app/page.tsx`、`prototype/app/globals.css`、`prototype/lib/rustkv-api.ts` 与 `prototype/components/design_lab/tokens.ts` 的当前实现整理。当前评审目标是大屏答辩演示；页面上的数据来源和实验结论必须始终标明为本地执行或后端实测。

## 1. 范围与执行模式

RustKV Lab 是 Rust 网络 KV 存储的可视化实验台，当前单页应用保留 5 个工作区：

| 工作区 | 英文标识 | 主要职责 |
| --- | --- | --- |
| 系统总览 | `Overview` | 服务状态、实验进度、最近操作 |
| 键值操作 | `KV Operations` | `SET`、`GET`、`DELETE`、`KEYS` |
| 并发实验 | `Concurrency` | 多客户端请求完成情况与正确性判定 |
| 崩溃恢复 | `Crash Recovery` | Seed、Kill、Snapshot + 增量 WAL 恢复与一致性校验 |
| 性能实验室 | `Performance Lab` | 并发对照、B 模块历史实测对比、实时 Compact 与动态结论 |

页面顶部固定展示品牌、执行模式、服务状态、实验导航和当前键总数。指标条只展示当前键总数，不虚构未有来源的系统指标。

### 1.1 两种执行模式

模式是数据来源和交互边界，视觉规范保持统一。

| 模式 | 页面语义 | 数据与请求规则 |
| --- | --- | --- |
| 退出答辩模式后的纯前端模式 | 纯前端 UI / 动画测试 | 使用浏览器内存和前端状态机；所有实验数值均为演示流程数据，非实测；不请求后端 |
| 答辩模式 | 通过适配层连接后端实测 | 浏览器请求 `/api/*`，Vite 转发到 `127.0.0.1:7879`；结果只采用控制器响应 |

纯前端模式的标题、状态 pill 和结果文案必须包含“纯前端”“UI/动画测试”或“非实测”等提示；答辩模式必须明确“后端实测”。答辩模式请求失败时进入离线、错误或中断状态，不允许降级到本地模拟，也不允许补造业务结果、恢复进度或性能点。

Performance Lab 在答辩模式下，首组准备、`A → B/C` 切换和每次 Retry 前都必须先获得后端 `POST /api/benchmark/reset` 确认，再开始下一组或重试；纯前端模式对应过程仅播放本地动画，不发送该请求。

切换模式或重置实验室会清理前端实验进度、图表和日志。答辩模式下的重置只清理前端状态，不删除后端数据或修改 WAL。

## 2. 视觉与组件基线

### 2.1 色彩与字体

页面保持暗色实验室外壳：`#080b10` 背景、28px 网格、半透明蓝黑面板、1px 边框和克制辉光。新增样式优先使用 CSS token，不在 JSX 中散落新颜色。

| Token | 值 | 语义 |
| --- | --- | --- |
| `--background` / `--foreground` | `#080b10` / `#f4f7fb` | 页面底色 / 主文字 |
| `--card` | `#111720` | 面板基础表面 |
| `--primary` / `--ring` | `#24d98f` / `#32dca1` | 主操作、通过、进度、focus |
| `--secondary` / `--muted` | `#18212d` / `#171e28` | 次级表面、弱化表面 |
| `--destructive` | `#f36565` | 删除、失败、离线 |
| `--border` / `--input` | `#202a36` / `#263240` | 结构线、输入边框 |
| `--benchmark-series-a/b/c` | `#2add9d` / `#36cfe2` / `#f09a58` | 性能图表系列 A/B/C |

绿色表示正常、通过和实时；青色表示读操作与数据；橙色表示 WAL、警示和执行中；红色表示危险、失败和离线；紫色用于系统/校验强调。颜色不能作为唯一状态信息。

普通文字使用 `var(--font-geist-sans)` 并以 `Microsoft YaHei` 回退；Key、Value、时间、日志、协议名、状态和指标数字使用 `var(--font-geist-mono)`。不另行引入字体。

### 2.2 几何与共享组件

- 顶栏高 `62px`，工作区导航高 `49px`，内容区为 `16px 24px 24px`，主网格 `gap: 12px`。
- 面板使用约 `10px` 圆角、1px 边框和深色渐变；内嵌数据面使用低透明蓝黑表面。
- 表单输入约 `36px` 高，普通操作按钮约 `35–38px` 高；键盘 focus 使用绿色 ring。
- 优先组合 `LabPanel`、`LabPanelHeader`、`LabButton`、`LabField`、`LabProgress`、`LabStatusPill`、`LabBadge` 与 `LabMetricStrip`；系列颜色使用 `LAB_BENCHMARK_SERIES_COLORS`。
- 图标统一使用 Lucide；按钮、状态、空状态和错误均提供可读文字。保留 `prefers-reduced-motion: reduce` 规则。

## 3. 大屏布局基准

以下是当前大屏 CSS 网格，新增内容应在这些区域内组合，不改变本次演示的宽屏信息层级：

| 工作区 | 大屏结构 |
| --- | --- |
| 系统总览 | 左侧服务卡，右侧实验进度；最近操作记录横跨底部 |
| 键值操作 | `310px / minmax(430px, 1fr) / 300px`：命令、存储视图、历史 |
| 并发实验 | 参数横跨首行；左侧客户端活动矩阵，右侧完成情况与判定 |
| 崩溃恢复 | 步骤横跨首行；控制、核心校验、Storage Replay 三列 |
| 性能实验室 | 12 列网格：参数区、预设区、固定条件、B 模块对比、实时存储状态、执行序列、双图、摘要和结论 |

本次只维护大屏布局，其他视口不在答辩演示范围内。

## 4. 工作区交互契约

### 4.1 键值操作

纯前端模式只读写浏览器内存；答辩模式通过适配层请求后端。`SET`、`GET`、`DELETE`、`KEYS` 的结果使用 `success/info/error` 三态和 `aria-live="polite"` 反馈。Key 非空、不可含空白或控制字符，UTF-8 不超过 256 字节；Value 非空、不可含控制字符，UTF-8 不超过 16 KiB。离线时输入与命令按钮禁用，并说明恢复服务的下一步。

### 4.2 Concurrency：只证明正确性

参数为客户端数量 `1 / 10 / 50 / 100`、每客户端请求数（默认 100）和 `Read / Mixed / Write` 访问模式。活动矩阵、进度环和结果区展示总请求、完成数、Success、Failed。

本工作区只给出 `CONCURRENCY PASS / FAIL` 或“未判定”结论：后端实测以所有请求完成且无失败为通过，纯前端模式只验证进度、状态和结论动画。这里不评价吞吐、延迟或运行时性能，性能比较统一放在 Performance Lab。

### 4.3 Crash Recovery：恢复来源与校验来源分离

演示步骤为 `Seed Data + Before → Kill Server → Restart + Snapshot / WAL Replay → Verify`，状态依次经过 `IDLE → PREPARED → CRASHED → RECOVERING → VERIFIED`。

| 证据 | 语义 |
| --- | --- |
| `Memory Store` | 易失内存；Kill 后清空，Restart 时由恢复流程重建 |
| 持久化 `Snapshot + WAL` | 真实恢复来源；先加载 Snapshot 最终状态，再按序重放 `lastSequence` 之后的增量 WAL |
| `Backend Before Evidence` | 控制器在数据可靠写入后返回的 Before 基线，只用于 Before/After 校验，绝不作为恢复来源 |

答辩模式依次调用 `POST /api/recovery/prepare`、`POST /api/recovery/kill`、`POST /api/recovery/restart`，并轮询 `GET /api/recovery/state`。Before/After 数量、指纹、抽样值、丢失数、WAL Replay 数量、恢复耗时、日志和进度全部来自控制器；页面不批量发送 SET、不计算实测指纹、不自增恢复进度，也不自行宣告服务上下线。纯前端模式仍可使用浏览器状态机测试 UI，但必须持续标明“非实测”。

### 4.4 Performance Lab

#### 实验方式与研究变量

- **单次实验**：执行一个配置系列，结论只描述本轮已收集点。
- **对照实验**：选择一个研究变量：`并发模型`（Sync / Async）、`锁策略`（Mutex / RwLock）或 `工作负载`（Read Heavy / Mixed / Write Heavy）。选中的变量自动生成对照组与实验组，其余条件保持固定。

#### 三个答辩 Preset

| Preset | 对照内容 | 固定条件 |
| --- | --- | --- |
| A | `Sync vs Async` | `Mutex`、`Mixed`、全部 Clients 规模 |
| B | `Mutex vs RwLock` | `Async`、`Read Heavy`、全部 Clients 规模 |
| C | `Read Heavy / Mixed / Write Heavy` | `Async`、`RwLock`、全部 Clients 规模 |

#### Fixed Conditions 与执行序列

固定条件面板必须展示：`10,000 Keys` 数据集、`128 B` 值大小、每个规模 `10,000 Requests`、`WAL + sync_data`、`JSON Lines`、`Localhost`。持久化条件不可由实验者切换。

Clients 规模可选 `1 → 10 → 50 → 100`，按升序逐点执行。多组答辩演示遵循 `A → 重置相同实验环境 → B/C`：切换对照系列前恢复同一数据集与持久化条件，并在执行序列中计数 `Environment Resets`。

#### B 模块提高项与实时 Compact

Performance Lab 单独保留“基础 WAL vs Snapshot + WAL 压缩”区域，不与并发模型的实时曲线混为同一实验：

- 四张柱状卡片展示持久化文件大小、启动恢复时间、Compact 一次性暂停和 2,000 次正常写入总耗时；
- 数据固定来自 `2026-09-02` 保存的同负载历史实测，每张卡使用独立数值尺度并显示单位，柱长只能在同一卡片内比较；
- 历史数据不会随当前后端状态变化，也不标记为本轮实时结果；
- 答辩模式通过 `GET /api/storage/state` 读取当前引擎、键数、WAL/Snapshot 字节数、WAL 记录数、连续序号和可写状态；
- 点击“执行真实 Compact”调用 `POST /api/storage/compact`，显示操作前后总大小、WAL 记录数和真实耗时，并再次读取存储状态；
- 纯前端模式不会构造实时存储统计或 Compact 结果；请求失败只显示错误，不降级为历史数据或模拟数据。

#### 双图、来源与动态结论

- **吞吐图**：Clients 对 `Throughput (req/s)`，标记 Peak、首个下降拐点和对照系列最大差异。
- **延迟图**：Clients 对 `P50 / P95 / P99 (ms)`，P99 为主线，保留尾延迟可读性。

图表、摘要和结论随已完成 Scale 动态更新：答辩模式显示“后端实测结果”，通过适配层响应计算 QPS 与延迟分位；纯前端模式显示“本地实验执行器 · 非实测”，只展示本地生成数据。单次结论描述最高吞吐；对照结论在同一 Clients 规模比较吞吐与 P99 方向，不把本地结果写成 RustKV 性能事实。

#### 停止、失败、保留与重试

- 点击停止后进入 `STOPPED`，已完成点、曲线和摘要保留；当前未完成点保持待执行，可继续。
- 后端错误、超时、无效结果或服务离线会将当前 Scale 标记为 `FAILED`，已完成点不清空，并显示可重试。
- `Retry Failed Scale` 会重跑失败点并继续剩余点；重试期间仍保持固定条件与系列顺序。

## 5. 状态、反馈与可操作性

- 服务状态为 `ONLINE / OFFLINE / STARTING / RECOVERING / ERROR`。状态点必须同时配文字；离线时冻结更新时间、禁用依赖服务的操作，并保留已完成结果。
- 实验状态为 `IDLE / RUNNING / COMPLETED / STOPPED / INTERRUPTED`。运行中禁用会改变条件的控件；停止或中断必须显示“未判定”而不是通过。
- 面板标题采用 kicker + 中文标题；协议名和实现名（如 `SET`、`GET`、`WAL`、`BTreeMap`）可保留英文，其余说明中文优先。
- 表单标签与输入通过 `label/htmlFor` 关联，图标按钮提供中文 `aria-label`，实验方式和研究变量使用 `aria-pressed`/原生 `disabled`。图表必须有标题、单位、图例和空状态；异步执行序列保留 polite live region。

新增页面或组件必须先复用 `components/design_lab` 和 `components/ui` 的现有模式，再补充 token、状态和可访问反馈；不得引入新的主题体系或改变大屏答辩的五工作区结构。

## 6. 修改核对

- 仍是暗色大屏实验台，顶部模式和数据来源清楚可见；
- Concurrency 只输出正确性结论；Recovery 明确 Memory Store、持久化 Snapshot + WAL 与 Before/After 校验证据的职责边界；
- Performance Lab 保留并发对照、B 模块历史实测、实时存储状态与 Compact、A/B/C Preset、Fixed Conditions、双图、来源标识、动态结论和失败重试；
- 所有异步、失败、离线和空状态都有文字与下一步提示；
- 只在本地验证，文档变更后按协作约定执行 `npm run lint` 与 `npm run build`，并如实记录结果。
