# RustKV Lab 前端设计规范

> 现状基线：2026-09-01。本文档根据 `prototype/app/page.tsx`、`prototype/app/globals.css`、`prototype/app/layout.tsx`、`prototype/components/ui/*`、`prototype/components/design_lab/*` 与 `prototype/components.json` 的当前实现整理。它描述的是 RustKV Lab 现有原型的真实设计系统，不把尚未实现的页面或后端能力当作既定规范。

## 1. 范围与设计定位

RustKV Lab 是 Rust 网络 KV 存储的可视化实验与测试平台。当前单页应用提供六个实验工作区：系统总览、键值操作、并发实验、崩溃恢复、性能测试、发布订阅。它是前端本地状态原型，页面明确使用“前端本地模拟 · 扩展预留”与“不连接后端”等提示；新增界面不能暗示已经存在真实网络服务或持久化后端。

### 1.1 设计原则

1. **先让系统状态可读，再装饰。** 在线/离线、实验进度、WAL、吞吐、延迟和校验结论必须有清晰的文字、数值和状态样式；颜色和动效只做辅助。
2. **实验过程可复验。** 控件、步骤、日志、数据前后对比和结论放在同一工作区内，用户应能看见“准备 → 执行 → 结果/校验”的因果链。
3. **技术控制台的紧凑密度。** 使用深色表面、细边框、等宽数字和小型辅助标签，优先展示数据与日志，不使用消费产品式的大面积留白或插画。
4. **中文优先，协议名保留。** 说明、按钮、状态和错误提示用中文；`SET`、`GET`、`DELETE`、`KEYS`、`WAL`、`TTL`、`BTreeMap` 等协议/实现名保留原文，并在必要处配中文解释。
5. **状态必须有语义，不靠颜色猜。** 成功、异常、运行中、离线等状态同时使用文字、图标或结构变化表达。
6. **复用现有基础设施。** 组件基于 `components/ui` 的 shadcn `base-nova` 风格、CSS 变量和 Lucide 图标；新增模式优先组合已有组件，不复制一份近似实现。

### 1.2 视觉气质

关键词是“深色系统实验室、实时监控、可验证、克制的荧光”。背景接近黑蓝色，面板为略亮的蓝灰渐变；绿色代表正常/成功/实时，青色代表读取与数据，紫色代表系统消息与发布订阅，橙色代表 WAL/警示，红色代表离线、失败和删除。网格背景、1px 线、细微辉光和等宽数据共同形成实验台/终端感。不要加入渐变霓虹背景、玻璃拟态大圆角、插画风或高饱和多色装饰。

## 2. 颜色系统

应用通过 `:root` 与 `.dark` 使用同一套暗色变量；`layout.tsx` 固定 `<html lang="zh-CN" className="dark">`，当前没有浅色主题分支。以下是 `globals.css` 中的准确基础 token：

| Token | Hex | 语义与当前用途 |
| --- | --- | --- |
| `--background` | `#080b10` | 页面底色、深色实验室背景 |
| `--foreground` | `#f4f7fb` | 默认高对比正文/前景色 |
| `--card` | `#111720` | 卡片基础色（面板另叠加渐变） |
| `--card-foreground` | `#f4f7fb` | 卡片文字 |
| `--popover` | `#151c26` | 弹层/浮层基础色 |
| `--popover-foreground` | `#f4f7fb` | 弹层文字 |
| `--primary` | `#24d98f` | 主操作、选中、成功、进度和实时状态 |
| `--primary-foreground` | `#03130c` | 绿色主按钮上的深色文字 |
| `--secondary` | `#18212d` | 次级按钮、次级表面 |
| `--secondary-foreground` | `#e8edf4` | 次级表面上的文字 |
| `--muted` | `#171e28` | 弱化表面、hover 背景 |
| `--muted-foreground` | `#8c9aaa` | 通用弱化文字 |
| `--accent` | `#152a29` | 暗绿色强调表面 |
| `--accent-foreground` | `#65e6bb` | 暗绿色强调文字 |
| `--destructive` | `#f36565` | 危险操作基础色 |
| `--border` | `#202a36` | 基础边框 token |
| `--input` | `#263240` | 输入控件边框/禁用表面 |
| `--ring` | `#32dca1` | 键盘 focus ring |

`@theme inline` 将这些变量映射为 `background`、`foreground`、`primary`、`secondary`、`muted`、`accent`、`destructive`、`border`、`input`、`ring` 等 Tailwind/shadcn 颜色名。新增样式应优先使用这些 token；当前 CSS 中已有的专项 hex 是实现证据，不能直接复制成新的散落颜色。

### 2.1 现有语义色（专项色）

这些颜色在页面组件中有明确含义；透明背景/边框使用同色 `rgba` 叠加，例如正常状态常见 `rgba(36,217,143,.05)` 表面、`rgba(36,217,143,.2)` 边框、`rgba(36,217,143,.08)` focus/图标表面。

| 语义 | 当前准确颜色 | 使用位置 |
| --- | --- | --- |
| 实时/成功绿 | `#24d98f`、`#2add9d`、`#43dda4`、`#48dfa8` | 主按钮、选中 tab、进度指示器、通过结论、吞吐曲线 |
| 读取/数据青 | `#36cfe2`、`#28cbe1`、`#35cadf` | GET、读取操作标签、键卡片图标、客户端读取 |
| 发布订阅/系统紫 | `#a78bfa`、`#9a84ff`、`#a89af1`、`#b1a0f2` | Pub/Sub broker、系统操作、订阅者状态、系统指标 |
| 品牌/WAL 橙 | `#f88c3f`、`#ef9555`、`#f08f4f` | RustKV 品牌标记、WAL 状态、TCP JSON 提示 |
| 警示/进行中琥珀 | `#f3a052`、`#f1a15d`、`#f0a158` | 运行中徽章、TTL 进度、启动/恢复状态 |
| 错误/离线红 | `#f36565`、`#f26767`、`#f07171`、`#f27979` | 服务离线、失败结论、删除与错误日志 |
| 累计请求蓝 | `#5b9cf5` | 顶部“累计请求”指标条 |
| 中性结构线 | `#202a36`、`#202b37`、`#293440` | 面板/网格/控件边框和分隔线 |

专项色的透明变体必须保留低对比、低亮度的实验室气质。状态色不要同时承担不相关的含义：例如橙色用于 WAL/警示，紫色用于 Pub/Sub/系统，红色用于危险/失败。

## 3. 字体与字号

- `layout.tsx` 通过 `next/font/google` 加载 `Geist` 与 `Geist_Mono`，变量名分别为 `--font-geist-sans` 和 `--font-geist-mono`。
- `body` 使用 `var(--font-geist-sans), "Microsoft YaHei", sans-serif`；`button`、`input`、`textarea`、`select` 继承字体。
- 等宽体 `var(--font-geist-mono)` 用于协议名、Key/Value、时间、日志、指标数字、英文副标题和状态徽章，保证数字列对齐；普通解释文案使用 Geist Sans，并以“Microsoft YaHei”作为中文回退。
- `components.json` 的 `--font-sans`、`--font-mono`、`--font-heading` 分别指向这些变量；不要另行引入字体。

当前实际字号是紧凑而有层级的，不存在统一的“所有正文 14px”规则：

| 层级/元素 | 当前字号 |
| --- | --- |
| 品牌名 | `15px`；品牌副标题等宽 `9px` |
| 页面标题 `h1` | `18px`；答辩模式 `23px` |
| 面板标题 `h2` | `14px`；compact 标题 `13px`；答辩模式 `16px` |
| 服务状态主数值 | `25px` 等宽 |
| 实时吞吐主数值 | `24px` 等宽 |
| 顶部指标数值 | `17px` 等宽；答辩模式 `20px` |
| 面板 kicker/表单标签 | 约 `8–9px`，等宽 kicker 带字距 |
| 数据、日志、辅助说明 | 约 `7–10px`；正文说明通常 `8–11px` |
| 进度环百分比 | `28px` 等宽；恢复计数 `25px` |

`letter-spacing` 是风格的一部分：顶部副标题约 `0.08em`、页面路径约 `0.13em`、kicker 约 `0.1em`、指标标签约 `0.11em`。保持短标签和大数值的对比，不要把小字号说明扩成大段密集正文。

## 4. 间距、圆角、边框与阴影

### 4.1 几何基准

- 根圆角 token：`--radius: 0.72rem`（约 `11.52px`）；派生 token 为 `--radius-sm = 0.65 × radius`、`--radius-md = 0.82 × radius`、`--radius-lg = radius`、`--radius-xl = 1.28 × radius`。
- 常见实际圆角：面板 `10px`；内嵌卡片/键单元/控制组 `7–9px`；分隔进度条 `5–10px`；状态徽章与连接 pill `999px`；步骤圆点和 broker 使用 `50%`。
- 页面主网格统一 `gap: 12px`。内部常见间距为 `4/5/6/7/8/9/10/11/12/13/14/15/16/17/18/19px`，按层级逐步收紧；不要凭感觉引入 24px 以上的大间隔。
- 桌面 `.content-area` 为 `padding: 16px 24px 24px`；面板多为 `17px` 内边距，服务/WAL 卡 `19px`，图表/恢复面板 `16px`，吞吐卡 `18px 18px 12px`。移动端内容区改为 `14px 12px 24px`。
- 顶栏高度 `62px`，tab 导航 `49px`，顶部指标条最小高度 `62px`，页面标题行 `45px`。

### 4.2 面板与表面

通用 `.panel`：

```css
border: 1px solid #202b37;
border-radius: 10px;
background: linear-gradient(145deg, rgba(18,25,34,.96), rgba(13,18,26,.98));
box-shadow: 0 16px 36px rgba(0,0,0,.11), inset 0 1px rgba(255,255,255,.018);
```

面板顶部通过 `::before` 加一条从透明到 `rgba(92,111,134,.4)` 再到透明的 1px 发光细线。深色日志/输入内嵌面通常使用 `#080c11` 或 `rgba(6,10,15,.48–.62)`，不能用纯白卡片承载实验内容。

现有代表性阴影：品牌标记内阴影 `inset 0 0 18px rgba(248,140,63,.08)`；键卡片 hover `0 8px 24px rgba(0,0,0,.16)`；进度、状态点和 broker 的辉光使用小范围 `0 0 8–40px` 同色阴影。阴影应服务于层级或实时焦点，不为每个文本节点加光晕。

## 5. 应用布局与响应式

### 5.1 外壳

`.lab-shell` 至少占满视口高度，背景声明由四层组成：顶部绿色径向光晕、低透明水平网格、低透明垂直网格和 `#080b10` 底色；两层网格共同形成 `28px × 28px` 的实验网格。

桌面顶栏是三列 grid：左侧品牌，中间连接状态 pill，右侧操作。连接 pill 显示状态点、中文状态和 `127.0.0.1:7878`；右侧提供“答辩模式”和“重置实验室”。下方 tab 导航固定六等分，当前项用绿色文字、浅绿背景和底部 2px 发光线标识。指标条固定六列，指标左侧使用 4px × 22px 的彩色竖条。

内容区先显示当前路径（`RUSTKV SYSTEMS LAB / ...`）、中文页面标题及最后更新时间，再渲染当前工作区。实验页统一使用 `.lab-page { height: calc(100vh - 258px); min-height: 620px; }`；短视口在桌面端允许内容区滚动。

### 5.2 六个工作区的桌面网格

| 工作区 | 当前列/行定义 | 结构重点 |
| --- | --- | --- |
| 系统总览 | `0.9fr 1.55fr 0.92fr`；两行 `minmax(250px,.95fr)` / `minmax(270px,1fr)` | 服务卡、吞吐卡、WAL 卡；实验卡横跨两列；操作流单独一列 |
| 键值操作 | `310px minmax(430px,1fr) 300px` | 命令面板、存储视图、历史记录三列 |
| 并发实验 | `minmax(0,1fr) 350px`；首行 `112px` | 参数横跨整行；下方为客户端活动矩阵 + 实时结果 |
| 崩溃恢复 | `330px minmax(430px,1fr) 320px`；`50px / minmax(0,1fr) / 174px` | 步骤横跨整行；控制、核心证明、WAL 重放；底部 WAL 压缩横跨整行 |
| 性能测试 | `minmax(430px,1.25fr) minmax(380px,1fr) 290px`；`105px / 87px / minmax(0,1fr)` | 参数、KPI 横跨整行；吞吐图、延迟图、结果解读三列 |
| 发布订阅 | `minmax(0,1fr) 340px` | 左侧消息舞台，右侧事件日志 |

### 5.3 断点行为

- `≤1260px`：KV 从三列收窄为 `285px minmax(390px,1fr) 260px`，键卡片改为两列并隐藏第 9 个之后的卡片；恢复/性能列宽同步收窄。
- `≤1080px`：实验页整体改为两列、自动行；大多数面板最小高度 `420px`。KV 存储、恢复压缩、发布订阅舞台等关键面板横跨两列；参数区和 KPI 横跨整行。顶栏改为两列并隐藏连接 pill；tab 隐藏英文副标题。
- `≤760px`：顶栏允许换行（最小高度 `62px`，左右 `14px`），隐藏品牌副标题和“答辩模式”按钮；tab 横向滚动，隐藏图标旁文字；指标条变为三列；隐藏最后更新时间；总览变为单列，服务/吞吐/WAL 卡最小高度 `260px`。
- `≤700px`：KV、并发、恢复、性能、发布订阅全部单列；实验/基准参数单列；KPI 两列；恢复步骤横向滚动且每步至少 `118px`；消息舞台单列并隐藏连接线；投递摘要缩小间距；键卡片保持两列。
- `≤780px` 高且宽度 `>1080px`：实验页高度改为自动、最小高度 `620px`，`.content-area` 滚动，避免内容被视口裁切。

新增面板应遵循“宽屏三列/两列 → 1080 两列 → 700 单列”的退化路径，并为日志、步骤、数据网格指定最小宽度或横向滚动，避免文字和数值互相覆盖。

## 6. 组件模式与状态

### 6.1 基础组件契约

当前 `components.json`：`style: base-nova`、`baseColor: neutral`、`cssVariables: true`、`iconLibrary: lucide`。优先使用 `components/ui` 的 `Button`、`Input`、`Progress`、`Switch`、`AlertDialog`、`ChartContainer` 及其配套组件。

`Button` 是 Base UI primitive + CVA：默认高度 `32px`；`xs` `24px`；`sm` `28px`；`lg` `36px`；icon `32px`、`icon-xs` `24px`、`icon-sm` `28px`、`icon-lg` `36px`。已有页面会按语义覆写为操作按钮 `36px`、发布按钮 `38px`、恢复按钮 `35px`。现有 variant：`default`、`outline`、`secondary`、`ghost`、`destructive`、`link`；危险/停止/强制终止使用 destructive 或其红色专项样式，次级导航使用 ghost/outline。

`Input` 基础高度 `32px`，实验表单统一覆写为 `36px`；背景为 `rgba(6,10,15,.55)`，边框 `#293644`，键盘聚焦为绿色边框与 `0 0 0 3px rgba(36,217,143,.08)`。`Progress` 基础 track 高度 `4px`，实验和 WAL 重放覆写为 `5px`；`Switch` 默认约 `32px × 18.4px`。

图标统一使用 Lucide，图标通常 `12–17px`，不使用带文字的 emoji 代替状态图标。`LabPanelHeader` 是页面现有的共享标题模式：kicker（图标 + 等宽小标签）→ `h2`，右侧可放徽章、数值或操作按钮。

### 6.2 通用模式

| 模式 | 当前实现 | 使用规则 |
| --- | --- | --- |
| 状态 pill/badge | `.connection-pill`、`.tiny-status`、`.experiment-badge`、`.proof-badge`、`.prototype-label`、`.sweet-spot`、`.latency-unit`、`.history-count` | 胶囊形、1px 边框、8px 等宽文字；用中文状态 + 颜色/图标，避免只显示色块 |
| 面板 | `.panel` + `LabPanelHeader` | 1px 结构线、10px 圆角、深色渐变；标题明确说明数据/实验目的 |
| 命令结果 | `.command-result` | `success/info/error` 三种结果；通过 `aria-live="polite"` 让读屏器读出最新反馈 |
| 数据卡片 | `.key-cell`、`.experiment-list > div`、`.result-kpis > div` | 低透明内嵌面、短标签 + 等宽数值；hover 仅轻微抬升 |
| 日志 | `.ops-list`、`.history-list`、`.replay-log`、`.event-log` | 时间/序号、操作或日志正文、结果；等宽、截断长 Key，不撑破网格 |
| 选择控件 | `.preset-buttons`、`.segmented`、`.view-toggle` | active 使用绿色边框/浅色表面；实验运行时禁用参数；视图切换按钮提供 `aria-label` |
| 空状态 | `.empty-state` | 图标 + 简短标题 + 下一步提示；例如“没有匹配的键”“修改搜索词，或用 SET 创建第一个键。” |

### 6.3 状态矩阵

服务状态类型为 `ONLINE`、`OFFLINE`、`STARTING`、`RECOVERING`、`ERROR`：

| 状态 | 当前文字 | 当前视觉/交互 |
| --- | --- | --- |
| `ONLINE` | 服务在线；网络与存储正常 | 绿色状态点呼吸，连接 pill 绿色，操作和实验可用，吞吐实时更新 |
| `OFFLINE` | 服务离线；进程已被强制终止 | 红色状态点，连接断开/曲线冻结，面板红色边框，KV/实验入口禁用；恢复页出现断电氛围边框 |
| `STARTING` | 正在启动；初始化存储引擎 | 橙色连接/服务状态，按恢复中处理 |
| `RECOVERING` | 正在恢复；重放 WAL 日志 | 橙色状态，恢复进度和日志更新，部分恢复控件禁用 |
| `ERROR` | 服务异常；需要人工检查 | 状态字典已有文字；当前 CSS 没有单独的 `.state-error` 规则，新增错误样式前应先补语义 token，不要误套 ONLINE 的绿色 |

实验状态类型为 `IDLE`、`RUNNING`、`COMPLETED`、`STOPPED`、`INTERRUPTED`。页面显示“准备就绪/运行中/已完成/已停止”，性能页对 `INTERRUPTED` 显示“已保留完成点”；运行中导航项加橙色小圆点，控制参数和开始按钮按上下文禁用。

恢复阶段为 `IDLE → PREPARED → CRASHED → RECOVERING → VERIFIED`。步骤条用 `done`、`active`、待执行三种结构表达；校验结果为等待、通过、失败，失败明确显示“发现不一致”及丢失数量，不用绿色掩盖故障注入。

KV 结果为 `success/info/error`；TTL 小于等于 10 秒的键卡片加 `.ttl-warning` 橙色边框和底部剩余比例条。发布订阅中订阅者有 `listening/disconnected`，broker 有 `online/offline`，发布中显示 `.flying-message`；离线时发布按钮和订阅状态必须同时体现不可用。

## 7. 图表与数据可视化

### 7.1 顶部指标与格式

指标条固定六项：`键总数`（青）、`活跃客户端`（紫）、`实时吞吐`（绿，单位请求/秒）、`累计请求`（蓝）、`成功率`（绿）、`WAL 大小`（橙）。数字使用 `Intl.NumberFormat('zh-CN')`；字节按 MB/KB/B 格式化。实时数据变化应更新对应数字和状态文案，而不是只移动装饰线。

### 7.2 总览吞吐 Sparkline

`Sparkline` 是 `viewBox="0 0 680 180"` 的内联 SVG，带四条水平虚线网格（`#202b36`，`3 5`）、绿色渐变面积填充、绿色曲线和末端点。在线时曲线使用 `#2add9d`/`#64f1bd` 与轻微 `green-glow`；离线通过 `.is-frozen` 切换为 `#647184`/`#657287`，去除辉光并保持最后状态，标题显示“服务离线，曲线已冻结”。SVG 当前有 `aria-label="最近 60 秒吞吐量曲线"`。

### 7.3 性能图表

性能页使用 `ChartContainer` + Recharts，并通过 `ChartConfig` 暴露准确系列 token：

| 系列 | 标签 | 颜色 |
| --- | --- | --- |
| `qps` | `吞吐量（req/s）` | `#2add9d` |
| `p50` | `P50 延迟` | `#36cfe2` |
| `p95` | `P95 延迟` | `#a78bfa` |
| `p99` | `P99 延迟` | `#f09a58` |

吞吐图是客户端数量对每秒请求数的 BarChart：顶部圆角 `5px`，隐藏坐标轴线和 tickLine，网格水平虚线 `#202b36`，hover cursor 为低透明绿色。延迟图是 P50/P95/P99 的 monotone LineChart：线宽 `2px`、点半径 `3px`，`x=50` 有绿色虚线参考线表示并发甜点候选。图表卡必须保留标题、单位和 tooltip；没有数据时使用空状态（“正在生成吞吐量数据点”“等待延迟样本”）而不是空白区域。

### 7.4 其他数据图形

- 并发结果环使用 `conic-gradient(#2add9d var(--progress), #1d2833 0)`，内圈 `#0f151d`，中间同时展示百分比和“已完成 / 总数”。
- 并发/恢复/压缩 Progress indicator 使用 `linear-gradient(90deg,#22c892,#50e3b1)`；轨道 `#202b36`。
- 客户端矩阵使用文字图例“读取/写入/删除/等待”和彩色节点；每个节点也有 `title="客户端 N"`，颜色不是唯一信息。
- WAL 压缩和 workload insight 使用比例条，比例旁必须有数量、单位或结论文字；不要用面积、角度或颜色编码没有说明的指标。

扩展 Recharts 图表时继续使用 `ChartContainer` 的 config 和 tooltip。当前 Recharts 图表没有额外的显式 `aria-label`，新增图表应补充可读标题/摘要或 `aria-describedby`；不要把坐标轴数值只留给视觉 tooltip。

## 8. 动效与实时反馈

动效只表示状态变化或消息流向：

- `breathe`：状态点 `2.4s` 循环；订阅监听图标 `2s`；导航运行点 `1.2s`。关键帧为透明度 `.75 → 1`、缩放 `.94 → 1.08`。
- `clientPulse`：并发运行时客户端矩阵的部分节点交替亮起，`.72s` 或 `.9s`（后者延迟 `.15s`），从 `brightness(.78)/scale(.98)` 到 `brightness(1.35)/scale(1.02)`。
- `messageFlight`：发布消息从右侧向 broker/订阅者方向飞行，时长 `.7s`，包含淡入、居中和淡出阶段。
- tab、按钮、键卡片和客户端节点分别使用约 `160ms`、`150ms`、`180ms` 的轻量过渡；键卡片 hover 只抬升 `1px`。
- `AlertDialog` 使用 Base UI 的淡入/淡出和轻微缩放；遮罩为 `rgba(3,6,10,.7)` + `8px` 背景模糊。

必须保留 `@media (prefers-reduced-motion: reduce)`：将滚动行为恢复为 auto，把动画迭代限制为一次，并把动画/过渡缩短至 `.01ms`。新增循环动画应确保关闭动效后状态仍可读；不要用闪烁、快速位移或持续大范围辉光传达错误。

“答辩模式”是现有密度/可读性开关：隐藏 `.secondary-detail`，页面字号放大到 `112%`，扩大页面标题、面板标题、关键 verdict/badge，并将内容横向内边距改为 `18px`。不要把它解释成另一套品牌主题或颜色主题。

## 9. 中文文案与术语

### 9.1 固定导航和品牌

- 品牌：`RustKV 实验室`
- 副标题：`Rust 网络 KV 存储 · 可视化实验与测试平台`
- 导航：`系统总览 / 键值操作 / 并发实验 / 崩溃恢复 / 性能测试 / 发布订阅`
- 英文 hint：`Overview / KV Operations / Concurrency / Crash Recovery / Performance / Pub / Sub`
- 页面路径前缀：`RUSTKV SYSTEMS LAB / ...`

### 9.2 操作和状态用语

沿用当前动词：`写入 SET`、`读取 GET`、`删除`、`查看全部键`、`开始并发实验`、`停止实验`、`运行基准测试`、`停止并保留结果`、`写入并记录快照`、`强制终止服务`、`重启并重放 WAL`、`运行 Compact`、`发布消息`、`订阅/取消`、`添加`、`重置实验室`。

错误和反馈应具体描述下一步，例如：`连接失败`并提示先在“崩溃恢复”页面重启服务，`键不能为空`，`值不能为空`，`未找到 NOT_FOUND`，`存储未发生变化`。成功反馈说明动作结果，如“已先写入 WAL，再更新内存”“删除记录已持久化到 WAL”。

单位与术语固定写法：`请求/秒`、`ms`、`MB/KB/B`、`客户端`、`键`、`数据指纹`、`抽样值`、`顺序重放`、`一对多推送`。技术流程可保留 `Build → Flush/Sync → Replace → Verify` 和 `Publisher → Broker → Subscribers`，但周围说明仍用中文。

避免使用“神奇”“黑盒”“绝对零丢失”等无法从原型状态证明的宣传语；当前是本地模拟，任何后端能力都要明确标注“原型/扩展预留”。

## 10. 无障碍与可操作性

当前实现的无障碍基线：

- 页面使用语义化 `main`、`header`、`nav`、`section`、`article`；`html` 的语言为 `zh-CN`。
- 主导航有 `aria-label="实验页面导航"`，总览和恢复步骤有区域标签；键值结果使用 `aria-live="polite"`。
- 输入标签通过 `htmlFor`/`id` 关联；搜索框有 `aria-label="搜索键或值"`；视图切换、分页和图标按钮有中文 `aria-label`。
- 答辩模式用 `aria-pressed`；不可执行状态使用原生 `disabled`，Base UI 会同时阻止点击并降低透明度。
- `Button`、`Input`、`Switch`、`AlertDialog` 复用 Base UI 的 `focus-visible` ring；自定义输入 focus 使用绿色 3px 外环。不要通过 `outline: none` 或仅用颜色取消焦点反馈。
- 状态点旁始终有文本；客户端图例有文字，图表有标题/单位，数据卡有数字和说明。新增状态不可只加一个红/绿点。

扩展时应保持键盘可达顺序、可见 focus、足够的点击区域（现有普通按钮约 32px，操作按钮 35–38px，移动端不要继续压缩），并给新的 icon-only 控件补中文 `aria-label`。为 Recharts 图表提供文本摘要或关联描述；为异步结果使用 polite live region，错误不要使用 assertive 连续打断读屏器。保留对 `prefers-reduced-motion` 的响应。

## 11. 使用清单与禁用清单

### 应该使用

- CSS 基础 token：`--background`、`--foreground`、`--primary`、`--secondary`、`--muted`、`--accent`、`--destructive`、`--border`、`--input`、`--ring`。
- `.panel` + `LabPanelHeader` 的标题结构，以及现有面板/日志/空状态/徽章模式。
- `components/ui` 的 Button、Input、Progress、Switch、AlertDialog、ChartContainer 和 Lucide 图标。
- 绿色/青色/紫色/橙色/红色的既有语义映射，并同时提供中文文字、数值或图标。
- 1px 边框、10px 面板圆角、12px 主网格间距、深色内嵌表面和克制的同色辉光。
- 中文优先的反馈句式、`SET/GET/DELETE/KEYS/WAL/TTL` 等保留术语、zh-CN 数字格式。
- 先处理 `ONLINE/OFFLINE/RUNNING/RECOVERING` 等状态，再决定按钮是否可用、图表是否冻结和文案如何更新。

### 不应该使用

- 不要为新页面引入浅色主题、白色卡片、与现有系统不一致的字体或全新的品牌色。
- 不要复制粘贴 `.panel`、按钮、日志、状态 pill 或图表 tooltip 做“差不多”的变体；先找 `components/ui` 或 `design_lab` 模式。
- 不要将任意 hex/渐变直接散落在 JSX 或新 CSS 中；当前 CSS 中的专项色必须先判断是否应提升为 token，再复用。
- 不要用颜色作为唯一状态信号，不要让红/绿动效持续闪烁，不要忽略 reduced-motion 或键盘 focus。
- 不要把实验数据画成没有单位/图例/摘要的装饰，不要删除空状态、错误状态、冻结状态或失败注入的明确反馈。
- 不要把“前端本地模拟”写成已连接真实后端；不要执行部署、保存 Sites 版本或创建远程服务。
- 不要在移动端依赖三列固定宽度；必须遵循既有断点、单列退化与必要的横向滚动。

## 12. `design_lab` 使用约定

此约定来自 `prototype/AGENTS.md`，是实现、文档和组件之间的协作契约：

1. 新增或修改页面/组件前，先查看 `prototype/components/design_lab`；优先复用其中已有的组件、布局模式和样式。
2. 设计变更必须同步维护本文件与组件库，让实现、文档和 `components/design_lab` 保持一致。
3. 必须使用已有设计 token；禁止绕过 token 硬编码样式，也禁止通过复制粘贴制造重复组件。
4. 界面文案与交互提示保持中文优先；本项目是无后端原型，只实现前端展示、交互和本地状态。

当前 `prototype/components/design_lab` 已提供一层面向 RustKV Lab 的可复用语义封装，当前导出包括：

- `LabPanel` / `LabPanelHeader`：复用 `.panel`、`.panel-heading`、`.panel-kicker`；
- `LabButton`：以 `success/info/danger/secondary` 映射现有 `op-button set/get/delete/keys` 与 Base UI Button variant；
- `LabField`：统一表单 label 与字段布局；
- `LabMetric` / `LabMetricStrip`：统一顶部指标与 `cyan/violet/green/blue/orange` accent class；
- `LabProgress`：对现有 Progress 的轻量封装；
- `LabStatusOrb` / `LabStatusPill` / `LabBadge`：统一在线、离线、警告、实验和校验状态；
- `LAB_ACCENT_CLASSES`、`LAB_STATUS_PILL_CLASSES`、`LAB_BADGE_VARIANT_CLASSES`：把语义映射到既有 CSS class，而不是新增颜色值。

`design_lab` 当前是页面共用模式的登记处，但颜色、圆角、间距和动效仍以 `globals.css` 的实际规则为准。新增模式应先在 `design_lab` 中找到或登记，再落到页面；若一个模式缺失，先确认它不是现有 `ui` 组件或页面模式的组合。确需新建时，同时更新三处：

- `components/design_lab`：可复用的示例/组件契约；
- `app/globals.css` 或组件样式：实际 token 与状态；
- `docs/FRONTEND_DESIGN.md`：语义、用法、状态、响应式和无障碍说明。

设计评审时以“复用 → token → 状态 → 响应式 → 无障碍 → 文案”的顺序检查，避免先做视觉变体、最后才补可操作性。任何无法从当前页面、组件库或本规范证明的风格，都视为待讨论项，不得默认加入产品。

## 13. 变更核对

修改前端设计后，至少确认：

- 页面仍保持暗色 `dark` 外壳，主网格和断点没有破坏；
- 所有新状态都有文字、图标/结构和可用性反馈；
- 新的颜色、字号、间距、圆角、动效有明确 token/语义或已同步提升；
- 表单标签、按钮名称、focus、disabled、live region 和图表摘要仍可用；
- 只在本地启动验证，不进行 Sites 部署；
- 运行 `npm run lint` 与 `npm run build`，如仅文档变更也如实记录验证结果。
