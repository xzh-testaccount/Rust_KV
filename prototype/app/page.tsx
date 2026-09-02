'use client';

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from 'react';
import {
  Activity,
  ArrowRight,
  Bell,
  Boxes,
  Check,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Clock3,
  Database,
  FileClock,
  Gauge,
  Grid3X3,
  HardDrive,
  KeyRound,
  LayoutDashboard,
  List,
  MessageSquare,
  Network,
  Pause,
  Play,
  Plus,
  Presentation,
  Radio,
  RefreshCcw,
  RotateCcw,
  Search,
  Send,
  Server,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Square,
  TerminalSquare,
  Trash2,
  Users,
  WifiOff,
  Zap,
} from 'lucide-react';
import {
  Bar,
  BarChart,
  CartesianGrid,
  Line,
  LineChart,
  ReferenceLine,
  XAxis,
  YAxis,
} from 'recharts';

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import {
  LabBadge,
  LabButton,
  LabField,
  LabMetricStrip,
  LabPanel,
  LabPanelHeader,
  LabProgress,
  LabStatusOrb,
  LabStatusPill,
  type LabMetricItem,
} from '@/components/design_lab';

type TabId =
  | 'overview'
  | 'kv'
  | 'concurrency'
  | 'recovery'
  | 'performance'
  | 'pubsub';

type ServerState = 'ONLINE' | 'OFFLINE' | 'STARTING' | 'RECOVERING' | 'ERROR';
type OperationTone = 'read' | 'write' | 'delete' | 'system' | 'error';
type ExperimentStatus = 'IDLE' | 'RUNNING' | 'COMPLETED' | 'STOPPED' | 'INTERRUPTED';
type RecoveryPhase = 'IDLE' | 'PREPARED' | 'CRASHED' | 'RECOVERING' | 'VERIFIED';
type Workload = '读取为主' | '写入为主' | '混合读写';

type KvEntry = {
  key: string;
  value: string;
};

type OperationLog = {
  id: number;
  time: string;
  op: string;
  detail: string;
  latency: string;
  tone: OperationTone;
  result: string;
};

type KvAction = 'SET' | 'GET' | 'DELETE' | 'KEYS';

type KvResult = {
  kind: 'success' | 'info' | 'error';
  title: string;
  message: string;
  value?: string;
};

type BenchmarkPoint = {
  clients: number;
  qps: number;
  p50: number;
  p95: number;
  p99: number;
  success: number;
};

type Subscriber = {
  id: number;
  name: string;
  active: boolean;
  received: string[];
};

type CompactResult = {
  beforeRecords: number;
  beforeBytes: number;
  afterRecords: number;
  afterBytes: number;
};

const INITIAL_KEY_COUNT = 327;
const INITIAL_WAL_RECORDS = 12_430;
const INITIAL_WAL_BYTES = 2.8 * 1024 * 1024;

const navigation = [
  { id: 'overview' as const, label: '系统总览', hint: 'Overview', icon: LayoutDashboard },
  { id: 'kv' as const, label: '键值操作', hint: 'KV Operations', icon: Database },
  { id: 'concurrency' as const, label: '并发实验', hint: 'Concurrency', icon: Network },
  { id: 'recovery' as const, label: '崩溃恢复', hint: 'Crash Recovery', icon: ShieldCheck },
  { id: 'performance' as const, label: '性能测试', hint: 'Performance', icon: Gauge },
  { id: 'pubsub' as const, label: '发布订阅', hint: 'Pub / Sub', icon: Radio },
];

const serverLabels: Record<ServerState, { label: string; detail: string }> = {
  ONLINE: { label: '服务在线', detail: '网络与存储正常' },
  OFFLINE: { label: '服务离线', detail: '进程已被强制终止' },
  STARTING: { label: '正在启动', detail: '初始化存储引擎' },
  RECOVERING: { label: '正在恢复', detail: '重放 WAL 日志' },
  ERROR: { label: '服务异常', detail: '需要人工检查' },
};

const initialNamedEntries: KvEntry[] = [
  { key: 'course:name', value: 'Rust 网络 KV 存储' },
  { key: 'course:stage', value: '答辩演示' },
  { key: 'user:1001', value: 'Alice' },
  { key: 'user:1002', value: 'Bob' },
  { key: 'config:engine', value: 'BTreeMap + WAL' },
  { key: 'config:port', value: '7878' },
  { key: 'session:demo', value: 'ready' },
  { key: 'feature:recovery', value: 'enabled' },
  { key: 'feature:pubsub', value: 'prototype' },
];

function makeInitialEntries(): KvEntry[] {
  const generated = Array.from(
    { length: INITIAL_KEY_COUNT - initialNamedEntries.length },
    (_, index) => ({
      key: `cache:item:${String(index + 1).padStart(3, '0')}`,
      value: `value-${String((index * 37) % 997).padStart(3, '0')}`,
    }),
  );
  return [...initialNamedEntries.map((entry) => ({ ...entry })), ...generated];
}

function formatClock() {
  const now = new Date();
  const base = now.toLocaleTimeString('zh-CN', { hour12: false });
  return `${base}.${String(now.getMilliseconds()).padStart(3, '0')}`;
}

function formatNumber(value: number) {
  return new Intl.NumberFormat('zh-CN').format(Math.max(0, Math.round(value)));
}

const textEncoder = new TextEncoder();

function hasControlCharacter(value: string) {
  return Array.from(value).some((character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return codePoint <= 0x1f || (codePoint >= 0x7f && codePoint <= 0x9f);
  });
}

function validateKey(key: string): KvResult | null {
  if (!key) {
    return { kind: 'error', title: '键不能为空', message: '请输入一个非空 Key 后再执行。' };
  }
  if (/\s/u.test(key) || hasControlCharacter(key)) {
    return { kind: 'error', title: '键格式无效 INVALID_KEY', message: 'Key 不能包含空白字符或控制字符。' };
  }
  if (textEncoder.encode(key).length > 256) {
    return { kind: 'error', title: '键过长 INVALID_KEY', message: 'Key 的 UTF-8 长度不能超过 256 字节。' };
  }
  return null;
}

function validateValue(value: string): KvResult | null {
  if (!value) {
    return { kind: 'error', title: '值不能为空', message: '请输入要写入的字符串 Value。' };
  }
  if (hasControlCharacter(value)) {
    return { kind: 'error', title: '值格式无效 INVALID_VALUE', message: 'Value 不能包含换行、制表符或其他控制字符；普通空格可以保留。' };
  }
  if (textEncoder.encode(value).length > 16 * 1024) {
    return { kind: 'error', title: '值过长 INVALID_VALUE', message: 'Value 的 UTF-8 长度不能超过 16 KiB。' };
  }
  return null;
}

function formatBytes(value: number) {
  if (value >= 1024 * 1024) return `${(value / 1024 / 1024).toFixed(1)} MB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KB`;
  return `${Math.round(value)} B`;
}

function fingerprintFor(entries: KvEntry[]) {
  let hash = 2166136261;
  for (const entry of entries) {
    const text = `${entry.key}=${entry.value};`;
    for (let index = 0; index < text.length; index += 1) {
      hash ^= text.charCodeAt(index);
      hash = Math.imul(hash, 16777619);
    }
  }
  return `0x${(hash >>> 0).toString(16).toUpperCase().padStart(8, '0')}`;
}

const initialOperations: OperationLog[] = [];

const simulationBenchmarkTemplate: BenchmarkPoint[] = [
  { clients: 1, qps: 1480, p50: 0.42, p95: 0.9, p99: 1.4, success: 100 },
  { clients: 10, qps: 7920, p50: 0.74, p95: 1.8, p99: 3.1, success: 100 },
  { clients: 50, qps: 12_340, p50: 1.3, p95: 4.7, p99: 8.2, success: 99.99 },
  { clients: 100, qps: 11_760, p50: 2.5, p95: 8.9, p99: 15.6, success: 99.97 },
];

const throughputChartConfig = {
  qps: { label: '吞吐量（req/s）', color: '#2add9d' },
} satisfies ChartConfig;

const latencyChartConfig = {
  p50: { label: 'P50 延迟', color: '#36cfe2' },
  p95: { label: 'P95 延迟', color: '#a78bfa' },
  p99: { label: 'P99 延迟', color: '#f09a58' },
} satisfies ChartConfig;

function Overview({
  serverState,
  operations,
  lastUpdate,
  concurrencyStatus,
  concurrencyThroughput,
  recoveryPhase,
  recoveryLost,
  benchmarkData,
}: {
  serverState: ServerState;
  operations: OperationLog[];
  lastUpdate: string;
  concurrencyStatus: ExperimentStatus;
  concurrencyThroughput: number;
  recoveryPhase: RecoveryPhase;
  recoveryLost: number;
  benchmarkData: BenchmarkPoint[];
}) {
  const online = serverState === 'ONLINE';
  const peak = benchmarkData.length ? Math.max(...benchmarkData.map((item) => item.qps)) : 0;
  return (
    <section className="overview-grid lab-page" aria-label="系统总览">
      <LabPanel className={`server-card ${online ? '' : 'offline-panel'}`}>
        <div className="panel-kicker"><Server size={15} /> 服务节点</div>
        <div className={`server-state state-${serverState.toLowerCase()}`}><LabStatusOrb offline={!online} /> {serverLabels[serverState].label}</div>
        <p className="muted">目标地址 127.0.0.1:7878 · 本次不连接后端</p>
        <div className="frontend-scope">
          <div><span>状态来源</span><strong>前端本地状态机</strong></div>
          <div><span>网络请求</span><strong>未发送</strong></div>
          <div><span>数据范围</span><strong>仅当前页面会话</strong></div>
        </div>
        <div className={online ? 'healthy-row' : 'error-row'}>
          {online ? <CheckCircle2 size={15} /> : <WifiOff size={15} />}
          {online ? '前端交互状态正常，可开始本地演示' : `本地模拟已切换为离线 · ${lastUpdate}`}
        </div>
      </LabPanel>

      <LabPanel className="experiment-card">
        <LabPanelHeader icon={<Boxes size={15} />} eyebrow="本地实验" title="页面交互进度" action={<span className="prototype-label">前端本地模拟</span>} />
        <div className="experiment-list">
          <div><span className="experiment-icon cyan"><Network size={17} /></span><p><strong>多客户端并发</strong><small>本地计时器驱动活动矩阵</small></p><b>{concurrencyThroughput ? formatNumber(concurrencyThroughput) : '—'} <small>模拟请求/秒</small></b><em>{concurrencyStatus === 'RUNNING' ? '运行中' : concurrencyStatus === 'COMPLETED' ? '模拟完成' : concurrencyStatus === 'STOPPED' ? '已停止' : '待运行'}</em></div>
          <div><span className="experiment-icon green"><ShieldCheck size={17} /></span><p><strong>崩溃恢复</strong><small>本地快照 · 故障状态演示</small></p><b>{recoveryPhase === 'VERIFIED' ? recoveryLost : '—'} <small>模拟丢失</small></b><em>{recoveryPhase === 'VERIFIED' ? (recoveryLost > 0 ? '发现不一致' : '模拟通过') : '待运行'}</em></div>
          <div><span className="experiment-icon violet"><Gauge size={17} /></span><p><strong>性能测试</strong><small>运行后逐点生成演示数据</small></p><b>{peak ? formatNumber(peak) : '—'} <small>模拟请求/秒</small></b><em>{benchmarkData.length ? '模拟完成' : '待运行'}</em></div>
        </div>
      </LabPanel>

      <LabPanel className="ops-card">
        <LabPanelHeader icon={<TerminalSquare size={15} />} eyebrow="本地操作" title="最近交互记录" action={<span className={online ? 'streaming' : 'streaming stopped'}><i /> {online ? '等待操作' : '已暂停'}</span>} />
        <div className="ops-list">
          {operations.length ? operations.slice(0, 5).map((row) => (
            <div key={row.id}>
              <time>{row.time}</time><span className={`op-tag ${row.tone}`}>{row.op}</span><code>{row.detail}</code><b>{row.result}</b>
            </div>
          )) : <div className="empty-log">执行一次键值操作或本地实验后，这里会显示记录。</div>}
        </div>
      </LabPanel>
    </section>
  );
}

function KvOperations({
  online,
  entries,
  operations,
  onAction,
}: {
  online: boolean;
  entries: KvEntry[];
  operations: OperationLog[];
  onAction: (action: KvAction, key: string, value: string) => KvResult;
}) {
  const [key, setKey] = useState('course:name');
  const [value, setValue] = useState('Rust 网络 KV 存储');
  const [search, setSearch] = useState('');
  const [page, setPage] = useState(0);
  const [view, setView] = useState<'grid' | 'list'>('grid');
  const [result, setResult] = useState<KvResult>({
    kind: 'info',
    title: '操作台已就绪',
    message: '选择命令并执行，结果会立即显示在这里。',
  });

  const filteredEntries = useMemo(() => {
    const normalized = search.trim().toLowerCase();
    if (!normalized) return entries;
    return entries.filter((entry) => entry.key.toLowerCase().includes(normalized) || entry.value.toLowerCase().includes(normalized));
  }, [entries, search]);

  const pageSize = view === 'grid' ? 9 : 7;
  const pageCount = Math.max(1, Math.ceil(filteredEntries.length / pageSize));
  const safePage = Math.min(page, pageCount - 1);
  const visibleEntries = filteredEntries.slice(safePage * pageSize, (safePage + 1) * pageSize);
  const visibleResult = online ? result : {
    kind: 'error' as const,
    title: '本地服务状态为离线',
    message: '请先在“崩溃恢复”页面执行模拟重启，再继续键值操作。',
  };

  const execute = (action: KvAction) => {
    const nextResult = onAction(action, key, value);
    setResult(nextResult);
    if (action === 'GET' && nextResult.value !== undefined) setValue(nextResult.value);
  };

  const selectEntry = (entry: KvEntry) => {
    setKey(entry.key);
    setValue(entry.value);
    setResult({ kind: 'info', title: '已载入键值', message: `可以继续 GET、SET 或 DELETE：${entry.key}` });
  };

  return (
    <section className="kv-page lab-page">
      <LabPanel className="command-panel">
        <LabPanelHeader icon={<KeyRound size={15} />} eyebrow="命令面板" title="键值基础操作" action={<LabStatusPill tone={online ? 'online' : 'offline'}>{online ? '可执行' : '连接已断开'}</LabStatusPill>} />
        <div className="form-stack">
          <LabField label="键 Key" htmlFor="kv-key"><Input id="kv-key" value={key} disabled={!online} onChange={(event) => setKey(event.target.value)} placeholder="例如 user:1001" /></LabField>
          <LabField label="值 Value" htmlFor="kv-value"><Input id="kv-value" value={value} disabled={!online} onChange={(event) => setValue(event.target.value)} placeholder="请输入字符串值" /></LabField>
        </div>
        <div className="kv-actions">
          <LabButton tone="success" disabled={!online} onClick={() => execute('SET')}><Plus /> 写入 SET</LabButton>
          <LabButton tone="info" disabled={!online} onClick={() => execute('GET')}><Search /> 读取 GET</LabButton>
          <LabButton tone="danger" disabled={!online} onClick={() => execute('DELETE')}><Trash2 /> 删除</LabButton>
          <LabButton tone="secondary" disabled={!online} onClick={() => execute('KEYS')}><List /> 查看全部键</LabButton>
        </div>
        <div className={`command-result ${visibleResult.kind}`} aria-live="polite">
          <span>{visibleResult.kind === 'success' ? <CheckCircle2 /> : visibleResult.kind === 'error' ? <CircleAlert /> : <TerminalSquare />}</span>
          <div><small>最近结果</small><strong>{visibleResult.title}</strong><p>{visibleResult.message}</p></div>
        </div>
        <div className="protocol-note secondary-detail"><code>{'>'} LOCAL STATE</code><span>本页只更新浏览器内存，不发送 TCP 或 HTTP 请求。</span></div>
      </LabPanel>

      <LabPanel className="store-panel">
        <LabPanelHeader icon={<Database size={15} />} eyebrow="内存存储视图" title={`${formatNumber(entries.length)} 个键正在内存中`} action={<div className="view-toggle"><button className={view === 'grid' ? 'active' : ''} onClick={() => { setView('grid'); setPage(0); }} aria-label="卡片视图"><Grid3X3 /></button><button className={view === 'list' ? 'active' : ''} onClick={() => { setView('list'); setPage(0); }} aria-label="列表视图"><List /></button></div>} />
        <div className="store-toolbar">
          <div className="search-box"><Search /><Input aria-label="搜索键或值" value={search} onChange={(event) => { setSearch(event.target.value); setPage(0); }} placeholder="搜索键或值" /></div>
          <span>已筛选 {formatNumber(filteredEntries.length)} 个</span>
        </div>
        {visibleEntries.length ? (
          <div className={`key-cells ${view}`}>
            {visibleEntries.map((entry) => (
                <button key={entry.key} type="button" className="key-cell" onClick={() => selectEntry(entry)}>
                  <span><Database /><code>{entry.key}</code></span>
                  <strong>{entry.value}</strong>
                  <small>永久保存</small>
                </button>
            ))}
          </div>
        ) : (
          <div className="empty-state"><Database /><strong>没有匹配的键</strong><p>修改搜索词，或用 SET 创建第一个键。</p></div>
        )}
      <div className="pager"><LabButton variant="ghost" size="icon-sm" disabled={safePage === 0} onClick={() => setPage(safePage - 1)} aria-label="上一页"><ChevronLeft /></LabButton><span>第 {safePage + 1} / {pageCount} 页</span><LabButton variant="ghost" size="icon-sm" disabled={safePage + 1 >= pageCount} onClick={() => setPage(safePage + 1)} aria-label="下一页"><ChevronRight /></LabButton></div>
      </LabPanel>

      <LabPanel className="history-panel">
        <LabPanelHeader icon={<Clock3 size={15} />} eyebrow="操作历史" title="请求与响应" action={<span className="history-count">最近 {Math.min(8, operations.length)} 条</span>} />
        <div className="history-list">
          {operations.length ? operations.slice(0, 8).map((row) => (
            <div key={row.id}><span className={`history-op ${row.tone}`}>{row.op}</span><p><code>{row.detail}</code><small>{row.result} · {row.latency}</small></p><time>{row.time.slice(0, 8)}</time></div>
          )) : <div className="empty-log">暂无本地操作记录。</div>}
        </div>
      </LabPanel>
    </section>
  );
}

function ConcurrencyPage({
  online,
  clients,
  setClients,
  requestsPerClient,
  setRequestsPerClient,
  workload,
  setWorkload,
  status,
  progress,
  successful,
  failed,
  throughput,
  elapsed,
  onStart,
  onStop,
}: {
  online: boolean;
  clients: number;
  setClients: (value: number) => void;
  requestsPerClient: number;
  setRequestsPerClient: (value: number) => void;
  workload: Workload;
  setWorkload: (value: Workload) => void;
  status: ExperimentStatus;
  progress: number;
  successful: number;
  failed: number;
  throughput: number;
  elapsed: number;
  onStart: () => void;
  onStop: () => void;
}) {
  const running = status === 'RUNNING';
  const total = clients * requestsPerClient;
  const renderedClients = Array.from({ length: Math.min(100, clients) }, (_, index) => index);
  const gridColumns = clients <= 1 ? 1 : clients <= 10 ? Math.min(5, clients) : 10;
  const currentCompleted = successful + failed;

  return (
    <section className="concurrency-page lab-page">
      <LabPanel className="experiment-config">
        <div className="config-title"><span className="panel-kicker"><Users size={15} /> 实验参数</span><h2>让多个客户端同时读写</h2></div>
        <div className="config-group"><span>客户端数量</span><div className="preset-buttons">{[1, 10, 50, 100].map((value) => <button key={value} className={clients === value ? 'active' : ''} disabled={running} onClick={() => setClients(value)}>{value}</button>)}</div></div>
        <LabField className="number-config" label="每客户端请求数" htmlFor="requests-per-client"><Input id="requests-per-client" type="number" min="10" max="1000" value={requestsPerClient} disabled={running} onChange={(event) => setRequestsPerClient(Math.max(10, Number(event.target.value) || 10))} /></LabField>
        <div className="config-group"><span>负载类型</span><div className="segmented">{(['读取为主', '写入为主', '混合读写'] as Workload[]).map((item) => <button key={item} className={workload === item ? 'active' : ''} disabled={running} onClick={() => setWorkload(item)}>{item}</button>)}</div></div>
        {running ? <LabButton variant="destructive" className="run-button" onClick={onStop}><Square /> 停止模拟</LabButton> : <LabButton className="run-button" disabled={!online} onClick={onStart}><Play /> 开始本地模拟</LabButton>}
      </LabPanel>

      <LabPanel className="client-grid-card">
        <LabPanelHeader icon={<Network size={15} />} eyebrow="客户端活动矩阵 · 前端本地模拟" title={`${clients} 个虚拟客户端并行工作`} action={<LabBadge variant="experiment" tone={status === 'IDLE' ? 'idle' : status === 'RUNNING' ? 'running' : status === 'COMPLETED' ? 'completed' : 'stopped'}>{status === 'IDLE' ? '准备就绪' : status === 'RUNNING' ? '模拟中' : status === 'COMPLETED' ? '模拟完成' : status === 'INTERRUPTED' ? '已中断' : '已停止'}</LabBadge>} />
        <div className="client-legend"><span><i className="read" />读取</span><span><i className="write" />写入</span><span><i className="delete" />删除</span><span><i className="idle" />等待</span></div>
        <div className={`client-grid ${running ? 'is-running' : ''}`} style={{ gridTemplateColumns: `repeat(${gridColumns}, minmax(0, 1fr))` }}>
          {renderedClients.map((index) => {
            const phase = (index + Math.floor(progress / 3)) % 7;
            const tone = !running && status !== 'COMPLETED' ? 'idle' : phase < 3 ? 'read' : phase < 6 ? 'write' : 'delete';
            return <div key={index} className={`client-node ${tone}`} title={`客户端 ${index + 1}`}><span>C{index + 1}</span><i /></div>;
          })}
        </div>
        <div className="sample-stream secondary-detail">
          <span>请求采样</span>
          <code>Client {Math.max(1, Math.min(clients, Math.floor(progress / 2) + 1))} → {workload === '写入为主' ? 'SET' : progress % 3 === 0 ? 'GET' : 'SET'} lab:key:{Math.floor(progress * 1.7)} → SUCCESS</code>
        </div>
      </LabPanel>

      <LabPanel className="concurrency-result">
        <LabPanelHeader icon={<Gauge size={15} />} eyebrow="实时结果" title="请求完成情况" />
        <div className="progress-ring" style={{ '--progress': `${progress * 3.6}deg` } as CSSProperties}><div><strong>{Math.round(progress)}%</strong><span>{formatNumber(currentCompleted)} / {formatNumber(total)}</span></div></div>
        <div className="result-kpis"><div><span>成功</span><strong className="success-text">{formatNumber(successful)}</strong></div><div><span>失败</span><strong className={failed ? 'danger-text' : ''}>{formatNumber(failed)}</strong></div><div><span>模拟吞吐</span><strong>{formatNumber(throughput)}<small> 请求/秒</small></strong></div><div><span>已用时间</span><strong>{elapsed.toFixed(1)}<small> 秒</small></strong></div></div>
        <LabProgress value={progress} className="experiment-progress" />
        <div className={`verdict-strip ${status === 'COMPLETED' ? 'pass' : status === 'STOPPED' || status === 'INTERRUPTED' ? 'stopped' : ''}`}>
          {status === 'COMPLETED' ? <CheckCircle2 /> : status === 'STOPPED' || status === 'INTERRUPTED' ? <Pause /> : <Activity />}
          <div><strong>{status === 'COMPLETED' ? '本地并发流程模拟完成' : status === 'STOPPED' ? '模拟已手动停止' : status === 'INTERRUPTED' ? '服务离线，模拟已中断' : running ? '虚拟请求正在执行' : online ? '参数就绪，等待开始' : '服务离线，无法开始'}</strong><span>{status === 'COMPLETED' ? '结果仅用于展示前端流程，不代表真实并发测试结论。' : status === 'INTERRUPTED' ? '已完成统计会保留，模拟服务恢复后可重新运行。' : '运行中可以切换页面，前端状态不会丢失。'}</span></div>
        </div>
      </LabPanel>
    </section>
  );
}

function RecoveryPage({
  serverState,
  phase,
  seedCount,
  setSeedCount,
  injectFailure,
  setInjectFailure,
  beforeCount,
  recoveredCount,
  recoveryLost,
  progress,
  logs,
  beforeFingerprint,
  afterFingerprint,
  sampleBefore,
  currentEntries,
  onSeed,
  onKill,
  onRestart,
  compactRunning,
  compactProgress,
  compactResult,
  onCompact,
}: {
  serverState: ServerState;
  phase: RecoveryPhase;
  seedCount: number;
  setSeedCount: (value: number) => void;
  injectFailure: boolean;
  setInjectFailure: (value: boolean) => void;
  beforeCount: number;
  recoveredCount: number;
  recoveryLost: number;
  progress: number;
  logs: string[];
  beforeFingerprint: string;
  afterFingerprint: string;
  sampleBefore: KvEntry[];
  currentEntries: KvEntry[];
  onSeed: () => void;
  onKill: () => void;
  onRestart: () => void;
  compactRunning: boolean;
  compactProgress: number;
  compactResult: CompactResult | null;
  onCompact: () => void;
}) {
  const phaseIndex = phase === 'IDLE' ? 0 : phase === 'PREPARED' ? 1 : phase === 'CRASHED' ? 2 : phase === 'RECOVERING' ? 3 : 4;
  const verified = phase === 'VERIFIED';
  const pass = verified && recoveryLost === 0 && beforeFingerprint === afterFingerprint;
  const offline = serverState === 'OFFLINE';
  const steps = ['准备数据', '强制断电', '重启服务', 'WAL 重放', '自动校验'];
  const beforeDisplay = beforeCount || currentEntries.length;
  const afterDisplay = verified ? currentEntries.length : phase === 'RECOVERING' ? recoveredCount : phase === 'CRASHED' ? 0 : '—';

  return (
    <section className={`recovery-page lab-page ${offline ? 'power-cut-mode' : ''}`}>
      <div className="recovery-steps" aria-label="恢复实验步骤">
        {steps.map((step, index) => <div key={step} className={`${index < phaseIndex || verified ? 'done' : ''} ${index === phaseIndex && !verified ? 'active' : ''}`}><span>{index < phaseIndex || verified ? <Check /> : index + 1}</span><b>{step}</b>{index < steps.length - 1 && <i />}</div>)}
      </div>

      <LabPanel className="recovery-control">
        <LabPanelHeader icon={<ShieldAlert size={15} />} eyebrow="断电实验控制 · 前端本地模拟" title="切换状态，再演示恢复流程" action={<LabStatusPill tone={serverState === 'ONLINE' ? 'online' : serverState === 'OFFLINE' ? 'offline' : 'warning'}>{serverLabels[serverState].label}</LabStatusPill>} />
        <div className="seed-presets"><span>准备演示数据</span><div>{[50, 100, 500, 1000].map((value) => <button key={value} className={seedCount === value ? 'active' : ''} disabled={serverState !== 'ONLINE' || phase === 'RECOVERING'} onClick={() => setSeedCount(value)}>{value} 键</button>)}</div></div>
        <div className="failure-toggle"><div><strong>故障注入</strong><small>仅让恢复后的前端快照少 2 个键，用于展示 FAIL</small></div><Switch aria-label="故障注入" checked={injectFailure} onCheckedChange={setInjectFailure} disabled={phase === 'RECOVERING' || phase === 'CRASHED'} /></div>
        <div className="recovery-actions">
          <LabButton variant="secondary" disabled={serverState !== 'ONLINE' || phase === 'RECOVERING'} onClick={onSeed}><Database /> ① 写入本地快照</LabButton>
          <LabButton variant="destructive" className="kill-button" disabled={phase !== 'PREPARED' || serverState !== 'ONLINE'} onClick={onKill}><Zap /> ② 模拟强制终止</LabButton>
          <LabButton className="restart-button" disabled={phase !== 'CRASHED'} onClick={onRestart}><RefreshCcw /> ③ 模拟重启与重放</LabButton>
        </div>
        <div className="power-explainer"><div className={serverState === 'OFFLINE' ? 'lost' : ''}><HardDrive /><span>页面内存中的键</span><strong>{serverState === 'OFFLINE' ? 0 : phase === 'RECOVERING' ? recoveredCount : currentEntries.length}</strong></div><ArrowRight /><div className="safe"><FileClock /><span>本地演示快照</span><strong>仍然保留 ✓</strong></div></div>
      </LabPanel>

      <LabPanel className={`recovery-proof ${verified ? (pass ? 'proof-pass' : 'proof-fail') : ''}`}>
        <LabPanelHeader icon={<ShieldCheck size={15} />} eyebrow="本地校验演示" title="恢复后的前端快照仍然一致吗？" action={verified ? <LabBadge variant="proof" tone={pass ? 'pass' : 'fail'}>{pass ? '模拟通过' : '发现不一致'}</LabBadge> : <LabBadge variant="proof" tone="waiting">等待实验</LabBadge>} />
        <div className="before-after">
          <div><span>崩溃前</span><strong>{formatNumber(beforeDisplay)}</strong><small>内存键</small></div>
          <ArrowRight />
          <div><span>重启后</span><strong>{typeof afterDisplay === 'number' ? formatNumber(afterDisplay) : afterDisplay}</strong><small>恢复键</small></div>
          <div className={`lost-count ${verified && recoveryLost ? 'danger' : ''}`}><span>丢失</span><strong>{verified ? recoveryLost : '—'}</strong><small>键</small></div>
        </div>
        <div className="integrity-checks">
          <div><span>数据指纹</span><code>{beforeFingerprint || '等待快照'}</code><ArrowRight /><code>{afterFingerprint || '等待恢复'}</code><em className={verified ? (beforeFingerprint === afterFingerprint ? 'pass' : 'fail') : ''}>{verified ? (beforeFingerprint === afterFingerprint ? '一致' : '不一致') : '待校验'}</em></div>
          {sampleBefore.slice(0, 3).map((sample) => {
            const after = currentEntries.find((entry) => entry.key === sample.key);
            const same = verified && after?.value === sample.value;
            return <div key={sample.key}><span>抽样值</span><code>{sample.key}</code><ArrowRight /><code>{verified ? (after?.value ?? 'MISSING') : sample.value}</code><em className={verified ? (same ? 'pass' : 'fail') : ''}>{verified ? (same ? '正确' : '错误') : '待校验'}</em></div>;
          })}
        </div>
        <div className={`big-verdict ${verified ? (pass ? 'pass' : 'fail') : 'waiting'}`}>
          {verified ? (pass ? <CheckCircle2 /> : <CircleAlert />) : <ShieldCheck />}
          <div><strong>{verified ? (pass ? '重启以后仍然对' : '重启以后发现不一致') : '等待重启后的自动比对'}</strong><span>{verified ? (pass ? `${formatNumber(beforeCount)} 个键全部恢复，抽样值与数据指纹完全一致。` : `发现 ${recoveryLost} 个键丢失，系统明确给出 FAIL，不掩盖问题。`) : '系统会同时比较键数量、抽样值和整体数据指纹。'}</span></div>
        </div>
      </LabPanel>

      <LabPanel className="replay-panel">
        <LabPanelHeader icon={<TerminalSquare size={15} />} eyebrow="WAL 重放动画" title="前端演示进度" action={<span className="replay-percent">{Math.round(progress)}%</span>} />
        <LabProgress value={progress} className="replay-progress" />
        <div className="replay-counter"><span>已恢复</span><strong>{formatNumber(phase === 'RECOVERING' ? recoveredCount : verified ? currentEntries.length : 0)}</strong><small>/ {formatNumber(beforeDisplay)} 键</small></div>
        <div className="replay-log">
          {logs.length ? logs.slice(-7).map((log, index) => <code key={`${log}-${index}`}><span>{String(index + 1).padStart(2, '0')}</span>{log}</code>) : <div className="empty-log">运行断电实验后，这里会逐条显示恢复过程。</div>}
        </div>
      </LabPanel>

      <LabPanel className="compact-panel">
        <LabPanelHeader icon={<FileClock size={15} />} eyebrow="WAL 压缩 · 扩展预留" title="前端本地模拟，不修改真实文件" action={<LabButton variant="outline" disabled={compactRunning || serverState !== 'ONLINE'} onClick={onCompact}>{compactRunning ? <Activity /> : <Sparkles />}{compactRunning ? '正在模拟' : '模拟 Compact'}</LabButton>} />
        <div className="compact-compare">
          <div><span>压缩前</span><strong>{formatNumber(compactResult?.beforeRecords ?? INITIAL_WAL_RECORDS)} 条</strong><i><b style={{ width: '92%' }} /></i><small>{formatBytes(compactResult?.beforeBytes ?? INITIAL_WAL_BYTES)}</small></div>
          <ArrowRight />
          <div><span>压缩后</span><strong>{compactResult ? formatNumber(compactResult.afterRecords) : '—'} 条</strong><i><b className="after" style={{ width: compactResult ? `${Math.max(7, (compactResult.afterBytes / compactResult.beforeBytes) * 100)}%` : '7%' }} /></i><small>{compactResult ? formatBytes(compactResult.afterBytes) : '等待执行'}</small></div>
        </div>
        <div className="compact-progress"><LabProgress value={compactProgress} /><span>{compactRunning ? `模拟构建快照与原子替换 · ${Math.round(compactProgress)}%` : compactResult ? `模拟体积减少 ${Math.round((1 - compactResult.afterBytes / compactResult.beforeBytes) * 100)}% · 前端流程完成` : '本地模拟：Build → Flush/Sync → Replace → Verify'}</span></div>
      </LabPanel>
    </section>
  );
}

function PerformancePage({
  online,
  scales,
  setScales,
  workload,
  setWorkload,
  status,
  progress,
  results,
  onStart,
  onStop,
}: {
  online: boolean;
  scales: number[];
  setScales: (value: number[]) => void;
  workload: Workload;
  setWorkload: (value: Workload) => void;
  status: ExperimentStatus;
  progress: number;
  results: BenchmarkPoint[];
  onStart: () => void;
  onStop: () => void;
}) {
  const running = status === 'RUNNING';
  const peak = results.length ? results.reduce((best, point) => point.qps > best.qps ? point : best, results[0]) : null;
  const bestP99 = results.length ? Math.min(...results.map((point) => point.p99)) : 0;
  const toggleScale = (scale: number) => setScales(scales.includes(scale) ? scales.filter((value) => value !== scale) : [...scales, scale].sort((a, b) => a - b));

  return (
    <section className="performance-page lab-page">
      <LabPanel className="benchmark-config">
        <div><span className="panel-kicker"><Gauge size={15} /> 性能演示参数</span><h2>生成不同并发规模下的前端演示数据</h2></div>
        <div className="config-group"><span>并发规模</span><div className="preset-buttons">{[1, 10, 50, 100].map((value) => <button key={value} className={scales.includes(value) ? 'active' : ''} disabled={running} onClick={() => toggleScale(value)}>{value}</button>)}</div></div>
        <div className="config-group"><span>负载类型</span><div className="segmented">{(['读取为主', '写入为主', '混合读写'] as Workload[]).map((item) => <button key={item} className={workload === item ? 'active' : ''} disabled={running} onClick={() => setWorkload(item)}>{item}</button>)}</div></div>
        <div className="request-summary"><span>每组虚拟请求</span><strong>10,000</strong><small>约 4 秒演示</small></div>
        {running ? <LabButton variant="destructive" onClick={onStop}><Pause /> 停止并保留结果</LabButton> : <LabButton disabled={!online || !scales.length} onClick={onStart}><Play /> 运行本地模拟</LabButton>}
      </LabPanel>

      <div className="benchmark-kpis">
        <div><span>模拟峰值吞吐</span><strong>{peak ? formatNumber(peak.qps) : '—'}<small> 请求/秒</small></strong><em>{peak ? `${peak.clients} 客户端` : '暂无数据'}</em></div>
        <div><span>并发甜点</span><strong>{peak?.clients ?? '—'}<small> 客户端</small></strong><em>吞吐最高点</em></div>
        <div><span>最低 P99</span><strong>{bestP99 ? bestP99.toFixed(1) : '—'}<small> ms</small></strong><em>尾延迟</em></div>
        <div><span>成功率</span><strong>{results.length ? Math.min(...results.map((point) => point.success)).toFixed(2) : '—'}<small>%</small></strong><em>{status === 'INTERRUPTED' ? '测试被中断' : '所有已完成规模'}</em></div>
        <div className="benchmark-progress"><span>{running ? '模拟进行中' : status === 'COMPLETED' ? '模拟完成' : status === 'INTERRUPTED' ? '已保留完成点' : '等待运行'}</span><strong>{Math.round(progress)}%</strong><LabProgress value={progress} /></div>
      </div>

      <LabPanel className="throughput-chart-card">
        <LabPanelHeader icon={<Activity size={15} />} eyebrow="吞吐量 · 前端本地模拟" title="客户端数量 vs 每秒请求数" action={<span className="prototype-label">非实测数据</span>} />
        {results.length ? (
          <ChartContainer config={throughputChartConfig} className="benchmark-chart" initialDimension={{ width: 640, height: 270 }}>
            <BarChart data={results} margin={{ top: 18, right: 12, bottom: 0, left: 0 }}>
              <CartesianGrid vertical={false} stroke="#202b36" strokeDasharray="3 5" />
              <XAxis dataKey="clients" tickLine={false} axisLine={false} tickFormatter={(value) => `${value} 客户端`} />
              <YAxis tickLine={false} axisLine={false} width={42} />
              <ChartTooltip cursor={{ fill: 'rgba(36,217,143,.05)' }} content={<ChartTooltipContent indicator="line" />} />
              <Bar dataKey="qps" fill="var(--color-qps)" radius={[5, 5, 0, 0]} />
            </BarChart>
          </ChartContainer>
        ) : <div className="empty-state chart-empty"><Gauge /><strong>等待运行本地模拟</strong><p>每完成一个并发规模，演示数据点会立即出现。</p></div>}
      </LabPanel>

      <LabPanel className="latency-chart-card">
        <LabPanelHeader icon={<Clock3 size={15} />} eyebrow="延迟分位 · 前端本地模拟" title="P50 / P95 / P99 尾延迟" action={<span className="latency-unit">单位：毫秒</span>} />
        {results.length ? (
          <ChartContainer config={latencyChartConfig} className="benchmark-chart" initialDimension={{ width: 540, height: 270 }}>
            <LineChart data={results} margin={{ top: 18, right: 14, bottom: 0, left: 0 }}>
              <CartesianGrid vertical={false} stroke="#202b36" strokeDasharray="3 5" />
              <XAxis dataKey="clients" tickLine={false} axisLine={false} />
              <YAxis tickLine={false} axisLine={false} width={34} />
              <ReferenceLine x={50} stroke="#2add9d" strokeDasharray="4 5" strokeOpacity={0.45} />
              <ChartTooltip content={<ChartTooltipContent indicator="line" />} />
              <Line type="monotone" dataKey="p50" stroke="var(--color-p50)" strokeWidth={2} dot={{ r: 3 }} />
              <Line type="monotone" dataKey="p95" stroke="var(--color-p95)" strokeWidth={2} dot={{ r: 3 }} />
              <Line type="monotone" dataKey="p99" stroke="var(--color-p99)" strokeWidth={2} dot={{ r: 3 }} />
            </LineChart>
          </ChartContainer>
        ) : <div className="empty-state chart-empty"><Clock3 /><strong>等待延迟样本</strong><p>已完成的数据不会因中途停止而清空。</p></div>}
      </LabPanel>

      <LabPanel className="workload-insight">
        <LabPanelHeader icon={<Sparkles size={15} />} eyebrow="结果解读" title="只解读本次已生成的数据点" action={<span className="prototype-label">非后端结论</span>} />
        {results.length ? <div className="insight-flow">{results.map((point) => <div key={point.clients} className={point.clients === peak?.clients ? 'best' : ''}><span>{point.clients} 客户端</span><i style={{ width: `${Math.max(8, point.qps / (peak?.qps ?? point.qps) * 88)}%` }} /><small>{point.clients === peak?.clients ? '模拟峰值' : `P99 ${point.p99.toFixed(1)} ms`}</small></div>)}</div> : <div className="empty-state"><Sparkles /><strong>暂无可解读结果</strong><p>运行本地模拟后再生成结论。</p></div>}
        <p>{results.length ? '这些数值由前端模板生成，只用于验证图表、停止与保留结果等交互，不代表真实 RustKV 性能。' : '本页不会在未运行时展示预置结果，也不会把模拟值标成实测。'}</p>
      </LabPanel>
    </section>
  );
}

function PubSubPage({
  online,
  subscribers,
  channel,
  setChannel,
  message,
  setMessage,
  publishing,
  logs,
  onPublish,
  onAddSubscriber,
  onToggleSubscriber,
}: {
  online: boolean;
  subscribers: Subscriber[];
  channel: string;
  setChannel: (value: string) => void;
  message: string;
  setMessage: (value: string) => void;
  publishing: boolean;
  logs: string[];
  onPublish: () => void;
  onAddSubscriber: () => void;
  onToggleSubscriber: (id: number) => void;
}) {
  const activeCount = subscribers.filter((subscriber) => subscriber.active).length;
  return (
    <section className="pubsub-page lab-page">
      <LabPanel className="pubsub-stage">
        <LabPanelHeader icon={<Radio size={15} />} eyebrow="发布订阅实验区" title="一条消息，推送给多个订阅者" action={<span className="prototype-label">前端本地模拟 · 扩展预留</span>} />
        <div className={`message-stage ${publishing ? 'is-publishing' : ''}`}>
          <div className="subscriber-zone">
            <div className="zone-title"><span>订阅者</span><LabButton variant="ghost" size="sm" onClick={onAddSubscriber} disabled={!online || publishing || subscribers.length >= 4}><Plus /> 添加</LabButton></div>
            {subscribers.map((subscriber) => (
              <div key={subscriber.id} className={`subscriber-card ${subscriber.active && online ? 'listening' : 'disconnected'}`}>
                <div><Bell /><span><strong>{subscriber.name}</strong><small>{subscriber.active && online ? `正在监听 #${channel}` : online ? '已取消订阅' : '连接已断开'}</small></span></div>
                <LabButton variant="ghost" size="xs" disabled={!online || publishing} onClick={() => onToggleSubscriber(subscriber.id)}>{subscriber.active ? '取消' : '订阅'}</LabButton>
                <p>{subscriber.received[0] ? `收到：“${subscriber.received[0]}”` : '等待第一条消息…'}</p>
              </div>
            ))}
          </div>

          <div className="broker-zone">
            <div className={`broker-node ${online ? 'online' : 'offline'}`}><Server /><strong>RustKV Broker（模拟）</strong><span>{online ? '正在演示消息路由' : '服务离线'}</span><code>#{channel}</code></div>
            <div className="broker-lines"><i /><i /><i /></div>
            {publishing && <div className="flying-message"><MessageSquare /><span>{message}</span></div>}
          </div>

          <div className="publisher-zone">
            <span className="zone-label">发布者</span>
            <div className="publisher-card"><Send /><strong>消息发布面板</strong><small>Publisher → Broker → Subscribers</small></div>
          <LabField label="频道 Channel" htmlFor="pubsub-channel"><Input id="pubsub-channel" value={channel} disabled={!online || publishing} onChange={(event) => setChannel(event.target.value.replace(/\s/g, ''))} placeholder="news" /></LabField>
          <LabField label="消息 Message" htmlFor="pubsub-message"><Input id="pubsub-message" value={message} disabled={!online || publishing} onChange={(event) => setMessage(event.target.value)} placeholder="Hello Rust" /></LabField>
          <LabButton className="publish-button" disabled={!online || publishing || !message.trim() || !channel.trim()} onClick={onPublish}><Send /> {publishing ? '正在推送…' : '发布消息'}</LabButton>
          </div>
        </div>
        <div className="delivery-summary"><div><span>当前频道</span><strong>#{channel || '—'}</strong></div><ArrowRight /><div><span>在线订阅者</span><strong>{online ? activeCount : 0}</strong></div><ArrowRight /><div><span>投递语义</span><strong>一对多推送</strong></div></div>
      </LabPanel>

      <LabPanel className="pubsub-log">
        <LabPanelHeader icon={<TerminalSquare size={15} />} eyebrow="事件日志" title="消息流转记录" action={<LabStatusPill tone={online ? 'online' : 'offline'}>{online ? '已连接' : '已断开'}</LabStatusPill>} />
        <div className="event-log">{logs.slice(-9).map((log, index) => <code key={`${log}-${index}`}><span>{String(index + 1).padStart(2, '0')}</span>{log}</code>)}</div>
        <div className="pubsub-compare"><div><Database /><span><strong>传统 KV</strong><small>客户端发起请求，服务端返回响应</small></span></div><div><Radio /><span><strong>发布订阅</strong><small>服务端主动把一条消息推给多个订阅者</small></span></div></div>
      </LabPanel>
    </section>
  );
}

function makeBenchmarkData(workload: Workload): BenchmarkPoint[] {
  const qpsFactor = workload === '读取为主' ? 1.14 : workload === '写入为主' ? 0.76 : 1;
  const latencyFactor = workload === '读取为主' ? 0.82 : workload === '写入为主' ? 1.36 : 1;
  return simulationBenchmarkTemplate.map((point) => ({
    ...point,
    qps: Math.round(point.qps * qpsFactor),
    p50: Number((point.p50 * latencyFactor).toFixed(2)),
    p95: Number((point.p95 * latencyFactor).toFixed(2)),
    p99: Number((point.p99 * latencyFactor).toFixed(2)),
  }));
}

export default function Home() {
  const [activeTab, setActiveTab] = useState<TabId>('overview');
  const [demoMode, setDemoMode] = useState(false);
  const [resetOpen, setResetOpen] = useState(false);
  const [serverState, setServerState] = useState<ServerState>('ONLINE');
  const [lastUpdate, setLastUpdate] = useState('刚刚');
  const [entries, setEntries] = useState<KvEntry[]>(makeInitialEntries);
  const [walRecords, setWalRecords] = useState(INITIAL_WAL_RECORDS);
  const [walBytes, setWalBytes] = useState(INITIAL_WAL_BYTES);
  const [operations, setOperations] = useState<OperationLog[]>(initialOperations);

  const [concurrencyClients, setConcurrencyClients] = useState(100);
  const [requestsPerClient, setRequestsPerClient] = useState(100);
  const [concurrencyWorkload, setConcurrencyWorkload] = useState<Workload>('混合读写');
  const [concurrencyStatus, setConcurrencyStatus] = useState<ExperimentStatus>('IDLE');
  const [concurrencyProgress, setConcurrencyProgress] = useState(0);
  const [concurrencySuccessful, setConcurrencySuccessful] = useState(0);
  const [concurrencyFailed, setConcurrencyFailed] = useState(0);
  const [concurrencyThroughput, setConcurrencyThroughput] = useState(0);
  const [concurrencyElapsed, setConcurrencyElapsed] = useState(0);

  const [recoveryPhase, setRecoveryPhase] = useState<RecoveryPhase>('IDLE');
  const [recoverySeedCount, setRecoverySeedCount] = useState(100);
  const [injectRecoveryFailure, setInjectRecoveryFailure] = useState(false);
  const [recoveryBeforeCount, setRecoveryBeforeCount] = useState(0);
  const [recoveredCount, setRecoveredCount] = useState(0);
  const [recoveryLost, setRecoveryLost] = useState(0);
  const [recoveryProgress, setRecoveryProgress] = useState(0);
  const [recoveryLogs, setRecoveryLogs] = useState<string[]>([]);
  const [beforeFingerprint, setBeforeFingerprint] = useState('');
  const [afterFingerprint, setAfterFingerprint] = useState('');
  const [recoverySamples, setRecoverySamples] = useState<KvEntry[]>([]);
  const [compactRunning, setCompactRunning] = useState(false);
  const [compactProgress, setCompactProgress] = useState(0);
  const [compactResult, setCompactResult] = useState<CompactResult | null>(null);

  const [benchmarkScales, setBenchmarkScales] = useState([1, 10, 50, 100]);
  const [benchmarkWorkload, setBenchmarkWorkload] = useState<Workload>('混合读写');
  const [benchmarkStatus, setBenchmarkStatus] = useState<ExperimentStatus>('IDLE');
  const [benchmarkProgress, setBenchmarkProgress] = useState(0);
  const [benchmarkResults, setBenchmarkResults] = useState<BenchmarkPoint[]>([]);

  const [subscribers, setSubscribers] = useState<Subscriber[]>([
    { id: 1, name: 'Subscriber A', active: true, received: [] },
    { id: 2, name: 'Subscriber B', active: true, received: [] },
    { id: 3, name: 'Subscriber C', active: false, received: [] },
  ]);
  const [pubsubChannel, setPubsubChannel] = useState('news');
  const [pubsubMessage, setPubsubMessage] = useState('Hello Rust');
  const [publishing, setPublishing] = useState(false);
  const [pubsubLogs, setPubsubLogs] = useState(['[READY] 发布订阅原型已就绪', '[SUB] Subscriber A → #news', '[SUB] Subscriber B → #news']);

  const logIdRef = useRef(0);
  const concurrencyTimerRef = useRef<number | null>(null);
  const recoveryTimerRef = useRef<number | null>(null);
  const compactTimerRef = useRef<number | null>(null);
  const benchmarkTimerRef = useRef<number | null>(null);
  const publishTimerRef = useRef<number | null>(null);
  const recoverySnapshotRef = useRef<KvEntry[]>([]);

  const clearTimer = (timerRef: { current: number | null }) => {
    if (timerRef.current !== null) window.clearInterval(timerRef.current);
    timerRef.current = null;
  };

  const addOperation = useCallback((op: string, detail: string, tone: OperationTone, result: string, latency = '—') => {
    logIdRef.current += 1;
    const next: OperationLog = { id: logIdRef.current, time: formatClock(), op, detail, latency, tone, result };
    setOperations((previous) => [next, ...previous].slice(0, 20));
    setLastUpdate(formatClock().slice(0, 8));
  }, []);

  useEffect(() => () => {
    clearTimer(concurrencyTimerRef);
    clearTimer(recoveryTimerRef);
    clearTimer(compactTimerRef);
    clearTimer(benchmarkTimerRef);
    if (publishTimerRef.current !== null) window.clearTimeout(publishTimerRef.current);
  }, []);

  const performKvAction = (action: KvAction, key: string, value: string): KvResult => {
    if (serverState !== 'ONLINE') {
      addOperation(action, key || '(empty)', 'error', '连接失败', '—');
      return { kind: 'error', title: '连接失败', message: 'RustKV 服务当前离线。请先在“崩溃恢复”页面重启服务。' };
    }
    if (action !== 'KEYS') {
      const keyError = validateKey(key);
      if (keyError) {
        addOperation(action, key || '(empty)', 'error', keyError.title, '—');
        return keyError;
      }
    }
    const normalizedKey = key;
    const existing = entries.find((entry) => entry.key === normalizedKey);

    if (action === 'SET') {
      const valueError = validateValue(value);
      if (valueError) {
        addOperation('SET', normalizedKey, 'error', valueError.title, '—');
        return valueError;
      }
      setEntries((previous) => existing ? previous.map((entry) => entry.key === normalizedKey ? { key: normalizedKey, value } : entry) : [{ key: normalizedKey, value }, ...previous]);
      setWalRecords((count) => count + 1);
      setWalBytes((bytes) => bytes + textEncoder.encode(normalizedKey).length + textEncoder.encode(value).length + 24);
      addOperation('SET', normalizedKey, 'write', existing ? '本地已更新' : '本地已创建', '—');
      return { kind: 'success', title: existing ? '本地写入成功 · 已更新' : '本地写入成功 · 已创建', message: '浏览器内存中的键值已更新；本次未发送后端请求。' };
    }
    if (action === 'GET') {
      if (!existing) {
        addOperation('GET', normalizedKey, 'read', '本地未找到', '—');
        return { kind: 'error', title: '未找到 NOT_FOUND', message: `存储中不存在键“${normalizedKey}”。` };
      }
      addOperation('GET', normalizedKey, 'read', '本地命中', '—');
      return { kind: 'success', title: '本地读取成功', message: `已从浏览器内存返回 ${existing.value.length} 个字符。`, value: existing.value };
    }
    if (action === 'DELETE') {
      if (!existing) {
        addOperation('DEL', normalizedKey, 'delete', '本地未找到', '—');
        return { kind: 'error', title: '删除失败 · 未找到', message: `键“${normalizedKey}”不存在，存储未发生变化。` };
      }
      setEntries((previous) => previous.filter((entry) => entry.key !== normalizedKey));
      setWalRecords((count) => count + 1);
      setWalBytes((bytes) => bytes + textEncoder.encode(normalizedKey).length + 18);
      addOperation('DEL', normalizedKey, 'delete', '本地删除', '—');
      return { kind: 'success', title: '本地删除成功', message: '浏览器内存中的记录已删除；本次未修改真实 WAL。' };
    }
    addOperation('KEYS', '*', 'system', `${entries.length} 个本地键`, '—');
    return { kind: 'info', title: `共有 ${formatNumber(entries.length)} 个本地键`, message: '右侧存储视图已分页展示，可通过搜索快速定位。' };
  };

  const startConcurrency = () => {
    if (serverState !== 'ONLINE') return;
    clearTimer(concurrencyTimerRef);
    setConcurrencyStatus('RUNNING');
    setConcurrencyProgress(0);
    setConcurrencySuccessful(0);
    setConcurrencyFailed(0);
    setConcurrencyThroughput(0);
    setConcurrencyElapsed(0);
    const total = concurrencyClients * requestsPerClient;
    const expectedThroughput = Math.round((concurrencyClients <= 50 ? 2200 + concurrencyClients * 205 : 12_450 - (concurrencyClients - 50) * 13) * (concurrencyWorkload === '读取为主' ? 1.12 : concurrencyWorkload === '写入为主' ? 0.78 : 1));
    let step = 0;
    concurrencyTimerRef.current = window.setInterval(() => {
      step += 2.5;
      const nextProgress = Math.min(100, step);
      const completed = Math.round(total * nextProgress / 100);
      setConcurrencyProgress(nextProgress);
      setConcurrencySuccessful(completed);
      setConcurrencyThroughput(Math.round(expectedThroughput * Math.min(1, 0.42 + nextProgress / 130)));
      setConcurrencyElapsed(nextProgress * 0.038);
      if (nextProgress >= 100) {
        clearTimer(concurrencyTimerRef);
        setConcurrencyStatus('COMPLETED');
        setConcurrencyThroughput(expectedThroughput);
        addOperation('LOAD', `${concurrencyClients} virtual clients`, 'system', '本地模拟完成', '约 3.8s');
      }
    }, 95);
  };

  const stopConcurrency = () => {
    clearTimer(concurrencyTimerRef);
    setConcurrencyStatus('STOPPED');
    addOperation('STOP', `${concurrencyClients} virtual clients`, 'system', '本地模拟已停止', '—');
  };

  const seedRecoveryData = () => {
    if (serverState !== 'ONLINE') return;
    const withoutOldSeed = entries.filter((entry) => !entry.key.startsWith('crash_test_'));
    const seeded = Array.from({ length: recoverySeedCount }, (_, index) => ({
      key: `crash_test_${String(index + 1).padStart(4, '0')}`,
      value: `durable-value-${String((index * 17 + 11) % 997).padStart(3, '0')}`,
    }));
    const next = [...seeded, ...withoutOldSeed];
    setEntries(next);
    recoverySnapshotRef.current = next.map((entry) => ({ ...entry }));
    setRecoveryBeforeCount(next.length);
    setBeforeFingerprint(fingerprintFor(next));
    setAfterFingerprint('');
    setRecoverySamples(seeded.slice(0, 3));
    setRecoveredCount(0);
    setRecoveryLost(0);
    setRecoveryProgress(0);
    setRecoveryPhase('PREPARED');
    setRecoveryLogs([`[SEED] 写入 ${recoverySeedCount} 个本地测试键`, '[SIM] 未连接后端；以下为 WAL 流程演示', `[SNAPSHOT] 模拟崩溃前 ${next.length} 个键 · ${fingerprintFor(next)}`]);
    setWalRecords((count) => count + recoverySeedCount);
    setWalBytes((bytes) => bytes + recoverySeedCount * 64);
    addOperation('SEED', `${recoverySeedCount} local keys`, 'write', '本地快照完成', '—');
  };

  const killServer = () => {
    if (recoveryPhase !== 'PREPARED' || serverState !== 'ONLINE') return;
    recoverySnapshotRef.current = entries.map((entry) => ({ ...entry }));
    setRecoveryBeforeCount(entries.length);
    setBeforeFingerprint(fingerprintFor(entries));
    setEntries([]);
    setServerState('OFFLINE');
    setRecoveryPhase('CRASHED');
    setRecoveredCount(0);
    setRecoveryLogs((previous) => [...previous, '[KILL] 前端模拟服务状态切换为离线', '[MEMORY] 页面键集合已清空 · 0 keys', '[SNAPSHOT] 本地演示快照仍完整保留']);
    setLastUpdate(formatClock().slice(0, 8));
    if (concurrencyStatus === 'RUNNING') {
      clearTimer(concurrencyTimerRef);
      setConcurrencyStatus('INTERRUPTED');
      addOperation('STOP', `${concurrencyClients} virtual clients`, 'error', '离线中断', '—');
    }
    if (benchmarkStatus === 'RUNNING') {
      clearTimer(benchmarkTimerRef);
      setBenchmarkStatus('INTERRUPTED');
    }
    if (compactRunning) {
      clearTimer(compactTimerRef);
      setCompactRunning(false);
    }
    if (publishTimerRef.current !== null) {
      window.clearTimeout(publishTimerRef.current);
      publishTimerRef.current = null;
      setPublishing(false);
      setPubsubLogs((previous) => [...previous, '[STOP] 服务离线，发布演示已取消']);
    }
    addOperation('KILL', 'local demo state', 'error', '模拟离线', '—');
  };

  const restartServer = () => {
    if (recoveryPhase !== 'CRASHED') return;
    clearTimer(recoveryTimerRef);
    setServerState('RECOVERING');
    setRecoveryPhase('RECOVERING');
    setRecoveryProgress(0);
    setRecoveryLogs((previous) => [...previous, '[BOOT] 前端模拟进入恢复状态', '[OPEN] 读取本地演示快照', '[REPLAY] 开始播放 WAL 重放动画']);
    const snapshot = recoverySnapshotRef.current.map((entry) => ({ ...entry }));
    let step = 0;
    const announced = new Set<number>();
    recoveryTimerRef.current = window.setInterval(() => {
      step += 4;
      const nextProgress = Math.min(100, step);
      const nextCount = Math.round(snapshot.length * nextProgress / 100);
      setRecoveryProgress(nextProgress);
      setRecoveredCount(nextCount);
      [25, 50, 75].forEach((threshold) => {
        if (nextProgress >= threshold && !announced.has(threshold)) {
          announced.add(threshold);
          setRecoveryLogs((previous) => [...previous, `[REPLAY] ${threshold}% · 已恢复 ${Math.round(snapshot.length * threshold / 100)} 个键`]);
        }
      });
      if (nextProgress >= 100) {
        clearTimer(recoveryTimerRef);
        const restored = injectRecoveryFailure ? snapshot.slice(0, Math.max(0, snapshot.length - 2)) : snapshot;
        const lost = snapshot.length - restored.length;
        setEntries(restored);
        setRecoveredCount(restored.length);
        setRecoveryLost(lost);
        setAfterFingerprint(fingerprintFor(restored));
        setServerState('ONLINE');
        setRecoveryPhase('VERIFIED');
        setRecoveryLogs((previous) => [...previous, `[VERIFY] 恢复 ${restored.length}/${snapshot.length} 个键`, `[HASH] ${fingerprintFor(restored)}`, lost ? `[FAIL] 检测到 ${lost} 个键丢失` : '[PASS] 数量、抽样值、数据指纹全部一致']);
        setLastUpdate('刚刚');
        addOperation('RESTART', 'local replay demo', lost ? 'error' : 'system', lost ? '模拟校验失败' : '模拟校验通过', '约 2.4s');
      }
    }, 90);
  };

  const runCompact = () => {
    if (serverState !== 'ONLINE' || compactRunning) return;
    clearTimer(compactTimerRef);
    const before: CompactResult = { beforeRecords: walRecords, beforeBytes: walBytes, afterRecords: entries.length, afterBytes: Math.max(18 * 1024, entries.length * 62) };
    setCompactRunning(true);
    setCompactProgress(0);
    setCompactResult(null);
    let step = 0;
    compactTimerRef.current = window.setInterval(() => {
      step += 4;
      setCompactProgress(Math.min(100, step));
      if (step >= 100) {
        clearTimer(compactTimerRef);
        setCompactRunning(false);
        setCompactResult(before);
        setWalRecords(before.afterRecords);
        setWalBytes(before.afterBytes);
        addOperation('COMPACT', 'local demo', 'system', '模拟流程完成', '约 1.8s');
      }
    }, 70);
  };

  const startBenchmark = () => {
    if (serverState !== 'ONLINE' || !benchmarkScales.length) return;
    clearTimer(benchmarkTimerRef);
    setBenchmarkStatus('RUNNING');
    setBenchmarkProgress(0);
    setBenchmarkResults([]);
    const allData = makeBenchmarkData(benchmarkWorkload).filter((point) => benchmarkScales.includes(point.clients));
    let index = 0;
    benchmarkTimerRef.current = window.setInterval(() => {
      const point = allData[index];
      if (point) {
        setBenchmarkResults((previous) => [...previous, point]);
        index += 1;
        setBenchmarkProgress(index / allData.length * 100);
      }
      if (index >= allData.length) {
        clearTimer(benchmarkTimerRef);
        setBenchmarkStatus('COMPLETED');
        addOperation('BENCH', `${allData.length} scales`, 'system', '本地模拟完成', `约 ${(allData.length * 0.9).toFixed(1)}s`);
      }
    }, 850);
  };

  const stopBenchmark = () => {
    clearTimer(benchmarkTimerRef);
    setBenchmarkStatus('INTERRUPTED');
    addOperation('STOP', 'benchmark', 'system', '保留已完成点', '—');
  };

  const publishMessage = () => {
    if (serverState !== 'ONLINE' || !pubsubMessage.trim() || !pubsubChannel.trim()) return;
    setPublishing(true);
    setPubsubLogs((previous) => [...previous, `[PUB] Publisher → #${pubsubChannel} · “${pubsubMessage}”`]);
    publishTimerRef.current = window.setTimeout(() => {
      const delivered = subscribers.filter((subscriber) => subscriber.active).length;
      setSubscribers((previous) => previous.map((subscriber) => subscriber.active ? { ...subscriber, received: [pubsubMessage, ...subscriber.received].slice(0, 3) } : subscriber));
      setPubsubLogs((previous) => [...previous, `[ROUTE] Broker 匹配到 ${delivered} 个订阅者`, `[DELIVER] 消息已成功投递 ${delivered} 次`]);
      setPublishing(false);
      addOperation('PUBLISH', `#${pubsubChannel}`, 'system', `本地模拟 ${delivered} 次投递`, '—');
      publishTimerRef.current = null;
    }, 720);
  };

  const addSubscriber = () => {
    if (serverState !== 'ONLINE' || publishing || subscribers.length >= 4) return;
    const id = Math.max(0, ...subscribers.map((subscriber) => subscriber.id)) + 1;
    setSubscribers((previous) => [...previous, { id, name: `Subscriber ${String.fromCharCode(64 + id)}`, active: true, received: [] }]);
    setPubsubLogs((previous) => [...previous, `[SUB] Subscriber ${String.fromCharCode(64 + id)} → #${pubsubChannel}`]);
  };

  const toggleSubscriber = (id: number) => {
    if (serverState !== 'ONLINE' || publishing) return;
    setSubscribers((previous) => previous.map((subscriber) => subscriber.id === id ? { ...subscriber, active: !subscriber.active } : subscriber));
    const target = subscribers.find((subscriber) => subscriber.id === id);
    if (target) setPubsubLogs((previous) => [...previous, `[${target.active ? 'UNSUB' : 'SUB'}] ${target.name} ${target.active ? '离开' : '加入'} #${pubsubChannel}`]);
  };

  const resetLab = () => {
    clearTimer(concurrencyTimerRef);
    clearTimer(recoveryTimerRef);
    clearTimer(compactTimerRef);
    clearTimer(benchmarkTimerRef);
    if (publishTimerRef.current !== null) window.clearTimeout(publishTimerRef.current);
    setActiveTab('overview');
    setDemoMode(false);
    setServerState('ONLINE');
    setLastUpdate('刚刚');
    setEntries(makeInitialEntries());
    setWalRecords(INITIAL_WAL_RECORDS);
    setWalBytes(INITIAL_WAL_BYTES);
    setOperations(initialOperations);
    setConcurrencyClients(100);
    setRequestsPerClient(100);
    setConcurrencyWorkload('混合读写');
    setConcurrencyStatus('IDLE');
    setConcurrencyProgress(0);
    setConcurrencySuccessful(0);
    setConcurrencyFailed(0);
    setConcurrencyThroughput(0);
    setConcurrencyElapsed(0);
    setRecoveryPhase('IDLE');
    setRecoverySeedCount(100);
    setInjectRecoveryFailure(false);
    setRecoveryBeforeCount(0);
    setRecoveredCount(0);
    setRecoveryLost(0);
    setRecoveryProgress(0);
    setRecoveryLogs([]);
    setBeforeFingerprint('');
    setAfterFingerprint('');
    setRecoverySamples([]);
    setCompactRunning(false);
    setCompactProgress(0);
    setCompactResult(null);
    setBenchmarkScales([1, 10, 50, 100]);
    setBenchmarkWorkload('混合读写');
    setBenchmarkStatus('IDLE');
    setBenchmarkProgress(0);
    setBenchmarkResults([]);
    setSubscribers([{ id: 1, name: 'Subscriber A', active: true, received: [] }, { id: 2, name: 'Subscriber B', active: true, received: [] }, { id: 3, name: 'Subscriber C', active: false, received: [] }]);
    setPubsubChannel('news');
    setPubsubMessage('Hello Rust');
    setPublishing(false);
    setPubsubLogs(['[READY] 发布订阅原型已就绪', '[SUB] Subscriber A → #news', '[SUB] Subscriber B → #news']);
    recoverySnapshotRef.current = [];
    publishTimerRef.current = null;
    logIdRef.current = 0;
    setResetOpen(false);
  };

  const displayedKeyCount = serverState === 'RECOVERING' ? recoveredCount : entries.length;
  const metricStrip: LabMetricItem[] = [
    { label: '本地键总数', value: formatNumber(displayedKeyCount), suffix: '仅浏览器内存', accent: 'cyan' },
  ];
  const activeNavigation = navigation.find((item) => item.id === activeTab)!;

  return (
    <main className={`lab-shell ${demoMode ? 'demo-mode' : ''} server-${serverState.toLowerCase()}`}>
      <header className="topbar">
        <div className="brand-block"><div className="brand-mark"><Database size={20} /></div><div><strong>RustKV <span>实验室</span></strong><small>纯前端交互演示 · 本次不连接后端</small></div></div>
        <div className={`connection-pill state-${serverState.toLowerCase()}`}><LabStatusOrb offline={serverState !== 'ONLINE'} /><b>{serverLabels[serverState].label}</b><code>LOCAL DEMO · 未联调</code></div>
        <div className="top-actions">
        <LabButton variant="ghost" className="shell-button" onClick={() => setDemoMode((value) => !value)} aria-pressed={demoMode}><Presentation /> {demoMode ? '退出答辩模式' : '答辩模式'}</LabButton>
        <LabButton variant="outline" className="shell-button" onClick={() => setResetOpen(true)}><RotateCcw /> 重置实验室</LabButton>
        </div>
      </header>

      <nav className="tab-nav" aria-label="实验页面导航">
        {navigation.map((item) => {
          const Icon = item.icon;
          const running = item.id === 'concurrency' && concurrencyStatus === 'RUNNING' || item.id === 'recovery' && recoveryPhase === 'RECOVERING' || item.id === 'performance' && benchmarkStatus === 'RUNNING';
          return <button key={item.id} type="button" className={`${activeTab === item.id ? 'active' : ''} ${running ? 'has-running' : ''}`} onClick={() => setActiveTab(item.id)}><Icon /><span><b>{item.label}</b><small>{item.hint}</small></span>{running && <i className="running-dot" />}</button>;
        })}
      </nav>

      <LabMetricStrip metrics={metricStrip} aria-label="本地数据概览" />

      <div className="content-area">
        <div className="page-title-row"><div><p><Activity size={14} /> RUSTKV SYSTEMS LAB / {activeNavigation.hint.toUpperCase()}</p><h1>{activeNavigation.label}</h1></div><div className={`last-update ${serverState !== 'ONLINE' ? 'frozen' : ''}`}><LabStatusOrb offline={serverState !== 'ONLINE'} /> {serverState === 'ONLINE' ? '本地状态 · 刚刚' : `${serverLabels[serverState].label} · 最后更新 ${lastUpdate}`}</div></div>

        {activeTab === 'overview' && <Overview serverState={serverState} operations={operations} lastUpdate={lastUpdate} concurrencyStatus={concurrencyStatus} concurrencyThroughput={concurrencyThroughput} recoveryPhase={recoveryPhase} recoveryLost={recoveryLost} benchmarkData={benchmarkResults} />}
        {activeTab === 'kv' && <KvOperations online={serverState === 'ONLINE'} entries={entries} operations={operations} onAction={performKvAction} />}
        {activeTab === 'concurrency' && <ConcurrencyPage online={serverState === 'ONLINE'} clients={concurrencyClients} setClients={setConcurrencyClients} requestsPerClient={requestsPerClient} setRequestsPerClient={setRequestsPerClient} workload={concurrencyWorkload} setWorkload={setConcurrencyWorkload} status={concurrencyStatus} progress={concurrencyProgress} successful={concurrencySuccessful} failed={concurrencyFailed} throughput={concurrencyThroughput} elapsed={concurrencyElapsed} onStart={startConcurrency} onStop={stopConcurrency} />}
        {activeTab === 'recovery' && <RecoveryPage serverState={serverState} phase={recoveryPhase} seedCount={recoverySeedCount} setSeedCount={setRecoverySeedCount} injectFailure={injectRecoveryFailure} setInjectFailure={setInjectRecoveryFailure} beforeCount={recoveryBeforeCount} recoveredCount={recoveredCount} recoveryLost={recoveryLost} progress={recoveryProgress} logs={recoveryLogs} beforeFingerprint={beforeFingerprint} afterFingerprint={afterFingerprint} sampleBefore={recoverySamples} currentEntries={entries} onSeed={seedRecoveryData} onKill={killServer} onRestart={restartServer} compactRunning={compactRunning} compactProgress={compactProgress} compactResult={compactResult} onCompact={runCompact} />}
        {activeTab === 'performance' && <PerformancePage online={serverState === 'ONLINE'} scales={benchmarkScales} setScales={setBenchmarkScales} workload={benchmarkWorkload} setWorkload={setBenchmarkWorkload} status={benchmarkStatus} progress={benchmarkProgress} results={benchmarkResults} onStart={startBenchmark} onStop={stopBenchmark} />}
        {activeTab === 'pubsub' && <PubSubPage online={serverState === 'ONLINE'} subscribers={subscribers} channel={pubsubChannel} setChannel={setPubsubChannel} message={pubsubMessage} setMessage={setPubsubMessage} publishing={publishing} logs={pubsubLogs} onPublish={publishMessage} onAddSubscriber={addSubscriber} onToggleSubscriber={toggleSubscriber} />}
      </div>

      <AlertDialog open={resetOpen} onOpenChange={setResetOpen}>
        <AlertDialogContent className="reset-dialog">
          <AlertDialogHeader><AlertDialogTitle>重置整个前端演示环境？</AlertDialogTitle><AlertDialogDescription>这只会清空浏览器中的实验进度，恢复初始键值与模拟在线状态；不会请求后端，也不会修改任何 WAL 文件。</AlertDialogDescription></AlertDialogHeader>
          <AlertDialogFooter><AlertDialogCancel>取消</AlertDialogCancel><AlertDialogAction onClick={resetLab}><RotateCcw /> 确认重置</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </main>
  );
}
