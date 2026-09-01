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
  expiresAt?: number;
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

const initialOperations: OperationLog[] = [
  { id: 5, time: '14:32:18.204', op: 'SET', detail: 'session:demo', latency: '1.4ms', tone: 'write', result: '成功' },
  { id: 4, time: '14:32:18.116', op: 'GET', detail: 'user:1001', latency: '0.6ms', tone: 'read', result: '命中' },
  { id: 3, time: '14:32:17.983', op: 'SET', detail: 'course:stage', latency: '1.1ms', tone: 'write', result: '成功' },
  { id: 2, time: '14:32:17.742', op: 'GET', detail: 'config:engine', latency: '0.5ms', tone: 'read', result: '命中' },
  { id: 1, time: '14:32:17.604', op: 'DEL', detail: 'temp:scan:04', latency: '0.9ms', tone: 'delete', result: '成功' },
];

const baselineBenchmark: BenchmarkPoint[] = [
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

function Sparkline({ offline = false }: { offline?: boolean }) {
  return (
    <svg className={`throughput-chart ${offline ? 'is-frozen' : ''}`} viewBox="0 0 680 180" aria-label="最近 60 秒吞吐量曲线">
      <defs>
        <linearGradient id="throughput-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={offline ? '#627083' : '#26d995'} stopOpacity="0.28" />
          <stop offset="100%" stopColor={offline ? '#627083' : '#26d995'} stopOpacity="0" />
        </linearGradient>
        <filter id="green-glow">
          <feGaussianBlur stdDeviation="3" result="blur" />
          <feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge>
        </filter>
      </defs>
      {[28, 68, 108, 148].map((y) => <line key={y} x1="0" y1={y} x2="680" y2={y} className="chart-grid" />)}
      <path d="M0 150 C30 142,42 118,68 126 S102 92,130 103 S170 122,198 88 S238 58,266 72 S306 100,334 67 S374 40,406 64 S440 112,470 93 S512 48,542 58 S582 87,612 72 S652 42,680 52 L680 180 L0 180 Z" fill="url(#throughput-fill)" />
      <path d="M0 150 C30 142,42 118,68 126 S102 92,130 103 S170 122,198 88 S238 58,266 72 S306 100,334 67 S374 40,406 64 S440 112,470 93 S512 48,542 58 S582 87,612 72 S652 42,680 52" className="chart-line" filter="url(#green-glow)" />
      <circle cx="680" cy="52" r="4" className="chart-dot" />
    </svg>
  );
}

function Overview({
  serverState,
  qps,
  walRecords,
  walBytes,
  operations,
  lastUpdate,
  concurrencyStatus,
  concurrencyThroughput,
  recoveryPhase,
  recoveryLost,
  benchmarkData,
}: {
  serverState: ServerState;
  qps: number;
  walRecords: number;
  walBytes: number;
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
        <p className="muted">RustKV Server · 127.0.0.1:7878</p>
        <div className="server-meta">
          <div><span>运行时长</span><strong>{online ? '02:14:33' : '已冻结'}</strong></div>
          <div><span>版本</span><strong>v0.3.0</strong></div>
          <div><span>存储引擎</span><strong>BTreeMap</strong></div>
        </div>
        <div className={online ? 'healthy-row' : 'error-row'}>
          {online ? <CheckCircle2 size={15} /> : <WifiOff size={15} />}
          {online ? '网络、内存与持久化均正常' : `最后更新 ${lastUpdate}`}
        </div>
      </LabPanel>

      <LabPanel className="throughput-card">
        <LabPanelHeader icon={<Activity size={15} />} eyebrow="实时吞吐" title={online ? '请求正在实时进入' : '服务离线，曲线已冻结'} action={<div className="live-number"><strong>{formatNumber(qps)}</strong><span>请求/秒</span></div>} />
        <Sparkline offline={!online} />
        <div className="chart-footer"><span>60 秒前</span><span>峰值 <strong>{formatNumber(Math.max(1438, peak))} 请求/秒</strong></span><span>现在</span></div>
      </LabPanel>

      <LabPanel className="wal-card">
        <div className="panel-kicker orange"><FileClock size={15} /> 持久化 / WAL</div>
        <div className="wal-state"><span>{serverState === 'RECOVERING' ? '正在重放' : '磁盘日志健康'}</span><strong>{formatBytes(walBytes)}</strong></div>
        <div className="wal-track"><i style={{ width: `${Math.min(88, 38 + walRecords / 260)}%` }} /></div>
        <div className="wal-stats">
          <div><span>日志记录</span><strong>{formatNumber(walRecords)}</strong></div>
          <div><span>最近同步</span><strong>{online ? '8ms 前' : '服务终止前'}</strong></div>
          <div><span>恢复策略</span><strong>顺序重放</strong></div>
        </div>
        <div className="wal-note"><Zap size={14} /> 先落盘，再修改内存；崩溃后日志仍在。</div>
      </LabPanel>

      <LabPanel className="experiment-card">
        <LabPanelHeader icon={<Boxes size={15} />} eyebrow="最近实验" title="答辩关键结论" action={<span className="all-pass">结论可复验</span>} />
        <div className="experiment-list">
          <div><span className="experiment-icon cyan"><Network size={17} /></span><p><strong>多客户端并发</strong><small>100 客户端 · 混合读写</small></p><b>{concurrencyThroughput ? `${(concurrencyThroughput / 1000).toFixed(1)}k` : '12.3k'} <small>请求/秒</small></b><em>{concurrencyStatus === 'RUNNING' ? '运行中' : '通过'}</em></div>
          <div><span className="experiment-icon green"><ShieldCheck size={17} /></span><p><strong>断电重启恢复</strong><small>WAL 重放 · 前后自动校验</small></p><b>{recoveryPhase === 'VERIFIED' ? recoveryLost : 0} <small>丢失</small></b><em>{recoveryPhase === 'VERIFIED' && recoveryLost > 0 ? '异常' : '重启后仍然对'}</em></div>
          <div><span className="experiment-icon violet"><Gauge size={17} /></span><p><strong>性能基准测试</strong><small>并发甜点 · 50 客户端</small></p><b>{formatNumber(peak || 12_340)} <small>请求/秒</small></b><em>已测量</em></div>
        </div>
      </LabPanel>

      <LabPanel className="ops-card">
        <LabPanelHeader icon={<TerminalSquare size={15} />} eyebrow="实时操作" title="最近请求流" action={<span className={online ? 'streaming' : 'streaming stopped'}><i /> {online ? '正在采样' : '已停止'}</span>} />
        <div className="ops-list">
          {operations.slice(0, 5).map((row) => (
            <div key={row.id}>
              <time>{row.time}</time><span className={`op-tag ${row.tone}`}>{row.op}</span><code>{row.detail}</code><b>{row.result}</b>
            </div>
          ))}
        </div>
      </LabPanel>
    </section>
  );
}

function KvOperations({
  online,
  entries,
  now,
  operations,
  onAction,
}: {
  online: boolean;
  entries: KvEntry[];
  now: number;
  operations: OperationLog[];
  onAction: (action: KvAction, key: string, value: string, ttlSeconds?: number) => KvResult;
}) {
  const [key, setKey] = useState('course:name');
  const [value, setValue] = useState('Rust 网络 KV 存储');
  const [ttl, setTtl] = useState('');
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

  const execute = (action: KvAction) => {
    const seconds = ttl ? Number(ttl) : undefined;
    const nextResult = onAction(action, key, value, seconds);
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
          <LabField label="键 Key" htmlFor="kv-key"><Input id="kv-key" value={key} onChange={(event) => setKey(event.target.value)} placeholder="例如 user:1001" /></LabField>
          <LabField label="值 Value" htmlFor="kv-value"><Input id="kv-value" value={value} onChange={(event) => setValue(event.target.value)} placeholder="请输入字符串值" /></LabField>
          <LabField className="ttl-field" label="过期时间 TTL（秒，可选）" htmlFor="kv-ttl"><Input id="kv-ttl" type="number" min="1" value={ttl} onChange={(event) => setTtl(event.target.value)} placeholder="不填写则永久有效" /></LabField>
        </div>
        <div className="kv-actions">
          <LabButton tone="success" onClick={() => execute('SET')}><Plus /> 写入 SET</LabButton>
          <LabButton tone="info" onClick={() => execute('GET')}><Search /> 读取 GET</LabButton>
          <LabButton tone="danger" onClick={() => execute('DELETE')}><Trash2 /> 删除</LabButton>
          <LabButton tone="secondary" onClick={() => execute('KEYS')}><List /> 查看全部键</LabButton>
        </div>
        <div className={`command-result ${result.kind}`} aria-live="polite">
          <span>{result.kind === 'success' ? <CheckCircle2 /> : result.kind === 'error' ? <CircleAlert /> : <TerminalSquare />}</span>
          <div><small>最近结果</small><strong>{result.title}</strong><p>{result.message}</p></div>
        </div>
        <div className="protocol-note secondary-detail"><code>{'>'} TCP JSON</code><span>所有交互均为本地原型模拟，不连接后端。</span></div>
      </LabPanel>

      <LabPanel className="store-panel">
        <LabPanelHeader icon={<Database size={15} />} eyebrow="内存存储视图" title={`${formatNumber(entries.length)} 个键正在内存中`} action={<div className="view-toggle"><button className={view === 'grid' ? 'active' : ''} onClick={() => { setView('grid'); setPage(0); }} aria-label="卡片视图"><Grid3X3 /></button><button className={view === 'list' ? 'active' : ''} onClick={() => { setView('list'); setPage(0); }} aria-label="列表视图"><List /></button></div>} />
        <div className="store-toolbar">
          <div className="search-box"><Search /><Input aria-label="搜索键或值" value={search} onChange={(event) => { setSearch(event.target.value); setPage(0); }} placeholder="搜索键或值" /></div>
          <span>已筛选 {formatNumber(filteredEntries.length)} 个</span>
        </div>
        {visibleEntries.length ? (
          <div className={`key-cells ${view}`}>
            {visibleEntries.map((entry) => {
              const ttlLeft = entry.expiresAt ? Math.max(0, Math.ceil((entry.expiresAt - now) / 1000)) : null;
              return (
                <button key={entry.key} type="button" className={`key-cell ${ttlLeft !== null && ttlLeft <= 10 ? 'ttl-warning' : ''}`} onClick={() => selectEntry(entry)}>
                  <span><Database /><code>{entry.key}</code></span>
                  <strong>{entry.value}</strong>
                  <small>{ttlLeft === null ? '永久保存' : `TTL ${ttlLeft} 秒`}</small>
                  {ttlLeft !== null && <i style={{ '--ttl': `${Math.min(100, ttlLeft)}%` } as CSSProperties} />}
                </button>
              );
            })}
          </div>
        ) : (
          <div className="empty-state"><Database /><strong>没有匹配的键</strong><p>修改搜索词，或用 SET 创建第一个键。</p></div>
        )}
      <div className="pager"><LabButton variant="ghost" size="icon-sm" disabled={safePage === 0} onClick={() => setPage(safePage - 1)} aria-label="上一页"><ChevronLeft /></LabButton><span>第 {safePage + 1} / {pageCount} 页</span><LabButton variant="ghost" size="icon-sm" disabled={safePage + 1 >= pageCount} onClick={() => setPage(safePage + 1)} aria-label="下一页"><ChevronRight /></LabButton></div>
      </LabPanel>

      <LabPanel className="history-panel">
        <LabPanelHeader icon={<Clock3 size={15} />} eyebrow="操作历史" title="请求与响应" action={<span className="history-count">最近 {Math.min(8, operations.length)} 条</span>} />
        <div className="history-list">
          {operations.slice(0, 8).map((row) => (
            <div key={row.id}><span className={`history-op ${row.tone}`}>{row.op}</span><p><code>{row.detail}</code><small>{row.result} · {row.latency}</small></p><time>{row.time.slice(0, 8)}</time></div>
          ))}
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
        {running ? <LabButton variant="destructive" className="run-button" onClick={onStop}><Square /> 停止实验</LabButton> : <LabButton className="run-button" disabled={!online} onClick={onStart}><Play /> 开始并发实验</LabButton>}
      </LabPanel>

      <LabPanel className="client-grid-card">
        <LabPanelHeader icon={<Network size={15} />} eyebrow="客户端活动矩阵" title={`${clients} 个客户端并行工作`} action={<LabBadge variant="experiment" tone={status === 'IDLE' ? 'idle' : status === 'RUNNING' ? 'running' : status === 'COMPLETED' ? 'completed' : 'stopped'}>{status === 'IDLE' ? '准备就绪' : status === 'RUNNING' ? '运行中' : status === 'COMPLETED' ? '已完成' : '已停止'}</LabBadge>} />
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
        <div className="result-kpis"><div><span>成功</span><strong className="success-text">{formatNumber(successful)}</strong></div><div><span>失败</span><strong className={failed ? 'danger-text' : ''}>{formatNumber(failed)}</strong></div><div><span>实时吞吐</span><strong>{formatNumber(throughput)}<small> 请求/秒</small></strong></div><div><span>已用时间</span><strong>{elapsed.toFixed(1)}<small> 秒</small></strong></div></div>
        <LabProgress value={progress} className="experiment-progress" />
        <div className={`verdict-strip ${status === 'COMPLETED' ? 'pass' : status === 'STOPPED' ? 'stopped' : ''}`}>
          {status === 'COMPLETED' ? <CheckCircle2 /> : status === 'STOPPED' ? <Pause /> : <Activity />}
          <div><strong>{status === 'COMPLETED' ? '并发一致性通过' : status === 'STOPPED' ? '实验已手动停止' : running ? '并发请求正在执行' : online ? '参数就绪，等待开始' : '服务离线，无法开始'}</strong><span>{status === 'COMPLETED' ? '全部客户端共享同一存储，未发现数据竞争。' : '运行中可以切换页面，实验状态不会丢失。'}</span></div>
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
        <LabPanelHeader icon={<ShieldAlert size={15} />} eyebrow="断电实验控制" title="亲手杀掉，再重新启动" action={<LabStatusPill tone={serverState === 'ONLINE' ? 'online' : serverState === 'OFFLINE' ? 'offline' : 'warning'}>{serverLabels[serverState].label}</LabStatusPill>} />
        <div className="seed-presets"><span>写入测试数据</span><div>{[50, 100, 500, 1000].map((value) => <button key={value} className={seedCount === value ? 'active' : ''} disabled={phase === 'RECOVERING'} onClick={() => setSeedCount(value)}>{value} 键</button>)}</div></div>
        <div className="failure-toggle"><div><strong>故障注入</strong><small>恢复时故意丢失 2 个键，用于展示 FAIL 状态</small></div><Switch checked={injectFailure} onCheckedChange={setInjectFailure} disabled={phase === 'RECOVERING' || phase === 'CRASHED'} /></div>
        <div className="recovery-actions">
          <LabButton variant="secondary" disabled={serverState !== 'ONLINE' || phase === 'RECOVERING'} onClick={onSeed}><Database /> ① 写入并记录快照</LabButton>
          <LabButton variant="destructive" className="kill-button" disabled={phase !== 'PREPARED' || serverState !== 'ONLINE'} onClick={onKill}><Zap /> ② 强制终止服务</LabButton>
          <LabButton className="restart-button" disabled={phase !== 'CRASHED'} onClick={onRestart}><RefreshCcw /> ③ 重启并重放 WAL</LabButton>
        </div>
        <div className="power-explainer"><div className={serverState === 'OFFLINE' ? 'lost' : ''}><HardDrive /><span>内存中的键</span><strong>{serverState === 'OFFLINE' ? 0 : phase === 'RECOVERING' ? recoveredCount : currentEntries.length}</strong></div><ArrowRight /><div className="safe"><FileClock /><span>磁盘 WAL</span><strong>仍然存在 ✓</strong></div></div>
      </LabPanel>

      <LabPanel className={`recovery-proof ${verified ? (pass ? 'proof-pass' : 'proof-fail') : ''}`}>
        <LabPanelHeader icon={<ShieldCheck size={15} />} eyebrow="核心证明" title="重启以后，数据仍然对吗？" action={verified ? <LabBadge variant="proof" tone={pass ? 'pass' : 'fail'}>{pass ? '校验通过' : '发现不一致'}</LabBadge> : <LabBadge variant="proof" tone="waiting">等待实验</LabBadge>} />
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
        <LabPanelHeader icon={<TerminalSquare size={15} />} eyebrow="WAL 重放" title="恢复过程实时可见" action={<span className="replay-percent">{Math.round(progress)}%</span>} />
        <LabProgress value={progress} className="replay-progress" />
        <div className="replay-counter"><span>已恢复</span><strong>{formatNumber(phase === 'RECOVERING' ? recoveredCount : verified ? currentEntries.length : 0)}</strong><small>/ {formatNumber(beforeDisplay)} 键</small></div>
        <div className="replay-log">
          {logs.length ? logs.slice(-7).map((log, index) => <code key={`${log}-${index}`}><span>{String(index + 1).padStart(2, '0')}</span>{log}</code>) : <div className="empty-log">运行断电实验后，这里会逐条显示恢复过程。</div>}
        </div>
      </LabPanel>

      <LabPanel className="compact-panel">
        <LabPanelHeader icon={<FileClock size={15} />} eyebrow="WAL 压缩" title="日志会增长，但可以安全压缩" action={<LabButton variant="outline" disabled={compactRunning || serverState !== 'ONLINE'} onClick={onCompact}>{compactRunning ? <Activity /> : <Sparkles />}{compactRunning ? '正在压缩' : '运行 Compact'}</LabButton>} />
        <div className="compact-compare">
          <div><span>压缩前</span><strong>{formatNumber(compactResult?.beforeRecords ?? INITIAL_WAL_RECORDS)} 条</strong><i><b style={{ width: '92%' }} /></i><small>{formatBytes(compactResult?.beforeBytes ?? INITIAL_WAL_BYTES)}</small></div>
          <ArrowRight />
          <div><span>压缩后</span><strong>{compactResult ? formatNumber(compactResult.afterRecords) : '—'} 条</strong><i><b className="after" style={{ width: compactResult ? `${Math.max(7, (compactResult.afterBytes / compactResult.beforeBytes) * 100)}%` : '7%' }} /></i><small>{compactResult ? formatBytes(compactResult.afterBytes) : '等待执行'}</small></div>
        </div>
        <div className="compact-progress"><LabProgress value={compactProgress} /><span>{compactRunning ? `构建快照并原子替换 · ${Math.round(compactProgress)}%` : compactResult ? `体积减少 ${Math.round((1 - compactResult.afterBytes / compactResult.beforeBytes) * 100)}% · 一致性通过` : 'Build → Flush/Sync → Replace → Verify'}</span></div>
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
        <div><span className="panel-kicker"><Gauge size={15} /> 基准测试参数</span><h2>测量不同并发规模下的吞吐与尾延迟</h2></div>
        <div className="config-group"><span>并发规模</span><div className="preset-buttons">{[1, 10, 50, 100].map((value) => <button key={value} className={scales.includes(value) ? 'active' : ''} disabled={running} onClick={() => toggleScale(value)}>{value}</button>)}</div></div>
        <div className="config-group"><span>负载类型</span><div className="segmented">{(['读取为主', '写入为主', '混合读写'] as Workload[]).map((item) => <button key={item} className={workload === item ? 'active' : ''} disabled={running} onClick={() => setWorkload(item)}>{item}</button>)}</div></div>
        <div className="request-summary"><span>每组请求</span><strong>10,000</strong><small>预计约 4 秒</small></div>
        {running ? <LabButton variant="destructive" onClick={onStop}><Pause /> 停止并保留结果</LabButton> : <LabButton disabled={!online || !scales.length} onClick={onStart}><Play /> 运行基准测试</LabButton>}
      </LabPanel>

      <div className="benchmark-kpis">
        <div><span>峰值吞吐</span><strong>{peak ? formatNumber(peak.qps) : '—'}<small> 请求/秒</small></strong><em>{peak ? `${peak.clients} 客户端` : '暂无数据'}</em></div>
        <div><span>并发甜点</span><strong>{peak?.clients ?? '—'}<small> 客户端</small></strong><em>吞吐最高点</em></div>
        <div><span>最低 P99</span><strong>{bestP99 ? bestP99.toFixed(1) : '—'}<small> ms</small></strong><em>尾延迟</em></div>
        <div><span>成功率</span><strong>{results.length ? Math.min(...results.map((point) => point.success)).toFixed(2) : '—'}<small>%</small></strong><em>{status === 'INTERRUPTED' ? '测试被中断' : '所有已完成规模'}</em></div>
        <div className="benchmark-progress"><span>{running ? '测试进行中' : status === 'COMPLETED' ? '测试完成' : status === 'INTERRUPTED' ? '已保留完成点' : '历史结果'}</span><strong>{Math.round(progress)}%</strong><LabProgress value={progress} /></div>
      </div>

      <LabPanel className="throughput-chart-card">
        <LabPanelHeader icon={<Activity size={15} />} eyebrow="吞吐量" title="客户端数量 vs 每秒请求数" action={peak && <span className="sweet-spot">甜点：{peak.clients} 客户端</span>} />
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
        ) : <div className="empty-state chart-empty"><Gauge /><strong>正在生成吞吐量数据点</strong><p>每完成一个并发规模，柱形会立即出现。</p></div>}
      </LabPanel>

      <LabPanel className="latency-chart-card">
        <LabPanelHeader icon={<Clock3 size={15} />} eyebrow="延迟分位" title="P50 / P95 / P99 尾延迟" action={<span className="latency-unit">单位：毫秒</span>} />
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
        <LabPanelHeader icon={<Sparkles size={15} />} eyebrow="结果解读" title="老师一眼能讲清楚的结论" />
        <div className="insight-flow"><div><span>1 客户端</span><i style={{ width: '18%' }} /><small>并行度不足</small></div><div className="best"><span>50 客户端</span><i style={{ width: '88%' }} /><small>吞吐峰值</small></div><div><span>100 客户端</span><i style={{ width: '78%' }} /><small>P99 开始上升</small></div></div>
        <p>写操作需要进入共享临界区，同时 WAL 会执行持久化同步，因此并发继续增大后吞吐不会无限增长。</p>
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
            <div className="zone-title"><span>订阅者</span><LabButton variant="ghost" size="sm" onClick={onAddSubscriber} disabled={subscribers.length >= 4}><Plus /> 添加</LabButton></div>
            {subscribers.map((subscriber) => (
              <div key={subscriber.id} className={`subscriber-card ${subscriber.active && online ? 'listening' : 'disconnected'}`}>
                <div><Bell /><span><strong>{subscriber.name}</strong><small>{subscriber.active && online ? `正在监听 #${channel}` : online ? '已取消订阅' : '连接已断开'}</small></span></div>
                <LabButton variant="ghost" size="xs" onClick={() => onToggleSubscriber(subscriber.id)}>{subscriber.active ? '取消' : '订阅'}</LabButton>
                <p>{subscriber.received[0] ? `收到：“${subscriber.received[0]}”` : '等待第一条消息…'}</p>
              </div>
            ))}
          </div>

          <div className="broker-zone">
            <div className={`broker-node ${online ? 'online' : 'offline'}`}><Server /><strong>RustKV Broker</strong><span>{online ? '正在路由消息' : '服务离线'}</span><code>#{channel}</code></div>
            <div className="broker-lines"><i /><i /><i /></div>
            {publishing && <div className="flying-message"><MessageSquare /><span>{message}</span></div>}
          </div>

          <div className="publisher-zone">
            <span className="zone-label">发布者</span>
            <div className="publisher-card"><Send /><strong>消息发布面板</strong><small>Publisher → Broker → Subscribers</small></div>
          <LabField label="频道 Channel" htmlFor="pubsub-channel"><Input id="pubsub-channel" value={channel} onChange={(event) => setChannel(event.target.value.replace(/\s/g, ''))} placeholder="news" /></LabField>
          <LabField label="消息 Message" htmlFor="pubsub-message"><Input id="pubsub-message" value={message} onChange={(event) => setMessage(event.target.value)} placeholder="Hello Rust" /></LabField>
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
  return baselineBenchmark.map((point) => ({
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
  const [totalRequests, setTotalRequests] = useState(1_281_440);
  const [failedRequests, setFailedRequests] = useState(246);
  const [operations, setOperations] = useState<OperationLog[]>(initialOperations);
  const [now, setNow] = useState(0);

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
  const [benchmarkStatus, setBenchmarkStatus] = useState<ExperimentStatus>('COMPLETED');
  const [benchmarkProgress, setBenchmarkProgress] = useState(100);
  const [benchmarkResults, setBenchmarkResults] = useState<BenchmarkPoint[]>(baselineBenchmark);

  const [subscribers, setSubscribers] = useState<Subscriber[]>([
    { id: 1, name: 'Subscriber A', active: true, received: [] },
    { id: 2, name: 'Subscriber B', active: true, received: [] },
    { id: 3, name: 'Subscriber C', active: false, received: [] },
  ]);
  const [pubsubChannel, setPubsubChannel] = useState('news');
  const [pubsubMessage, setPubsubMessage] = useState('Hello Rust');
  const [publishing, setPublishing] = useState(false);
  const [pubsubLogs, setPubsubLogs] = useState(['[READY] 发布订阅原型已就绪', '[SUB] Subscriber A → #news', '[SUB] Subscriber B → #news']);

  const logIdRef = useRef(10);
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

  const addOperation = useCallback((op: string, detail: string, tone: OperationTone, result: string, latency = '0.8ms') => {
    logIdRef.current += 1;
    const next: OperationLog = { id: logIdRef.current, time: formatClock(), op, detail, latency, tone, result };
    setOperations((previous) => [next, ...previous].slice(0, 20));
    setLastUpdate(formatClock().slice(0, 8));
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => {
      const timestamp = Date.now();
      setNow(timestamp);
      setEntries((previous) => {
        const expired = previous.some((entry) => entry.expiresAt !== undefined && entry.expiresAt <= timestamp);
        return expired ? previous.filter((entry) => entry.expiresAt === undefined || entry.expiresAt > timestamp) : previous;
      });
    }, 1000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => () => {
    clearTimer(concurrencyTimerRef);
    clearTimer(recoveryTimerRef);
    clearTimer(compactTimerRef);
    clearTimer(benchmarkTimerRef);
    if (publishTimerRef.current !== null) window.clearTimeout(publishTimerRef.current);
  }, []);

  const performKvAction = (action: KvAction, key: string, value: string, ttlSeconds?: number): KvResult => {
    setTotalRequests((count) => count + 1);
    if (serverState !== 'ONLINE') {
      setFailedRequests((count) => count + 1);
      addOperation(action, key || '(empty)', 'error', '连接失败', '—');
      return { kind: 'error', title: '连接失败', message: 'RustKV 服务当前离线。请先在“崩溃恢复”页面重启服务。' };
    }
    if (action !== 'KEYS' && !key.trim()) {
      setFailedRequests((count) => count + 1);
      return { kind: 'error', title: '键不能为空', message: '请输入一个非空 Key 后再执行。' };
    }
    const normalizedKey = key.trim();
    const existing = entries.find((entry) => entry.key === normalizedKey);

    if (action === 'SET') {
      if (!value.trim()) {
        setFailedRequests((count) => count + 1);
        return { kind: 'error', title: '值不能为空', message: '请输入要写入的字符串 Value。' };
      }
      const expiresAt = ttlSeconds && ttlSeconds > 0 ? Date.now() + ttlSeconds * 1000 : undefined;
      setEntries((previous) => existing ? previous.map((entry) => entry.key === normalizedKey ? { key: normalizedKey, value, expiresAt } : entry) : [{ key: normalizedKey, value, expiresAt }, ...previous]);
      setWalRecords((count) => count + 1);
      setWalBytes((bytes) => bytes + normalizedKey.length + value.length + 24);
      addOperation('SET', normalizedKey, 'write', existing ? '已更新' : '已创建', '1.1ms');
      return { kind: 'success', title: existing ? '写入成功 · 已更新' : '写入成功 · 已创建', message: ttlSeconds ? `键将在 ${ttlSeconds} 秒后过期。` : '该键已先写入 WAL，再更新内存。' };
    }
    if (action === 'GET') {
      if (!existing) {
        setFailedRequests((count) => count + 1);
        addOperation('GET', normalizedKey, 'read', '未找到', '0.5ms');
        return { kind: 'error', title: '未找到 NOT_FOUND', message: `存储中不存在键“${normalizedKey}”。` };
      }
      addOperation('GET', normalizedKey, 'read', '命中', '0.5ms');
      return { kind: 'success', title: '读取成功', message: `已返回 ${existing.value.length} 个字符。`, value: existing.value };
    }
    if (action === 'DELETE') {
      if (!existing) {
        setFailedRequests((count) => count + 1);
        addOperation('DEL', normalizedKey, 'delete', '未找到', '0.7ms');
        return { kind: 'error', title: '删除失败 · 未找到', message: `键“${normalizedKey}”不存在，存储未发生变化。` };
      }
      setEntries((previous) => previous.filter((entry) => entry.key !== normalizedKey));
      setWalRecords((count) => count + 1);
      setWalBytes((bytes) => bytes + normalizedKey.length + 18);
      addOperation('DEL', normalizedKey, 'delete', '成功', '0.9ms');
      return { kind: 'success', title: '删除成功', message: '删除记录已持久化到 WAL。' };
    }
    addOperation('KEYS', '*', 'system', `${entries.length} 个键`, '1.8ms');
    return { kind: 'info', title: `共有 ${formatNumber(entries.length)} 个键`, message: '右侧存储视图已分页展示，可通过搜索快速定位。' };
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
        setTotalRequests((count) => count + total);
        setWalRecords((count) => count + Math.round(total * (concurrencyWorkload === '读取为主' ? 0.2 : concurrencyWorkload === '写入为主' ? 0.8 : 0.5)));
        setWalBytes((bytes) => bytes + total * 22);
        addOperation('LOAD', `${concurrencyClients} clients`, 'system', '并发通过', '3.8s');
      }
    }, 95);
  };

  const stopConcurrency = () => {
    clearTimer(concurrencyTimerRef);
    setConcurrencyStatus('STOPPED');
    addOperation('STOP', `${concurrencyClients} clients`, 'system', '已停止', '—');
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
    setRecoveryLogs([`[SEED] 写入 ${recoverySeedCount} 个测试键`, `[SYNC] WAL 已 flush + sync_data`, `[SNAPSHOT] 崩溃前 ${next.length} 个键 · ${fingerprintFor(next)}`]);
    setWalRecords((count) => count + recoverySeedCount);
    setWalBytes((bytes) => bytes + recoverySeedCount * 64);
    setTotalRequests((count) => count + recoverySeedCount);
    addOperation('SEED', `${recoverySeedCount} keys`, 'write', '快照完成', '42ms');
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
    setRecoveryLogs((previous) => [...previous, '[KILL] 服务进程被强制终止', '[MEMORY] BTreeMap 已丢失 · 0 keys', '[DISK] WAL 文件仍完整保留']);
    setLastUpdate(formatClock().slice(0, 8));
    if (concurrencyStatus === 'RUNNING') stopConcurrency();
    if (benchmarkStatus === 'RUNNING') {
      clearTimer(benchmarkTimerRef);
      setBenchmarkStatus('INTERRUPTED');
    }
    addOperation('KILL', 'rustkv-server', 'error', '服务离线', '—');
  };

  const restartServer = () => {
    if (recoveryPhase !== 'CRASHED') return;
    clearTimer(recoveryTimerRef);
    setServerState('RECOVERING');
    setRecoveryPhase('RECOVERING');
    setRecoveryProgress(0);
    setRecoveryLogs((previous) => [...previous, '[BOOT] RustKV 进程重新启动', '[OPEN] 打开 rustkv.wal', '[REPLAY] 开始顺序重放日志记录']);
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
        addOperation('RESTART', 'wal replay', lost ? 'error' : 'system', lost ? '校验失败' : '重启后仍然对', '2.4s');
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
        addOperation('COMPACT', 'rustkv.wal', 'system', '一致性通过', '1.8s');
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
        setTotalRequests((count) => count + 10_000);
      }
      if (index >= allData.length) {
        clearTimer(benchmarkTimerRef);
        setBenchmarkStatus('COMPLETED');
        addOperation('BENCH', `${allData.length} scales`, 'system', '测试完成', `${(allData.length * 0.9).toFixed(1)}s`);
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
      setTotalRequests((count) => count + 1);
      addOperation('PUBLISH', `#${pubsubChannel}`, 'system', `${delivered} 次投递`, '1.2ms');
      publishTimerRef.current = null;
    }, 720);
  };

  const addSubscriber = () => {
    if (subscribers.length >= 4) return;
    const id = Math.max(0, ...subscribers.map((subscriber) => subscriber.id)) + 1;
    setSubscribers((previous) => [...previous, { id, name: `Subscriber ${String.fromCharCode(64 + id)}`, active: true, received: [] }]);
    setPubsubLogs((previous) => [...previous, `[SUB] Subscriber ${String.fromCharCode(64 + id)} → #${pubsubChannel}`]);
  };

  const toggleSubscriber = (id: number) => {
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
    setServerState('ONLINE');
    setLastUpdate('刚刚');
    setEntries(makeInitialEntries());
    setWalRecords(INITIAL_WAL_RECORDS);
    setWalBytes(INITIAL_WAL_BYTES);
    setTotalRequests(1_281_440);
    setFailedRequests(246);
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
    setBenchmarkStatus('COMPLETED');
    setBenchmarkProgress(100);
    setBenchmarkResults(baselineBenchmark);
    setSubscribers([{ id: 1, name: 'Subscriber A', active: true, received: [] }, { id: 2, name: 'Subscriber B', active: true, received: [] }, { id: 3, name: 'Subscriber C', active: false, received: [] }]);
    setPubsubChannel('news');
    setPubsubMessage('Hello Rust');
    setPublishing(false);
    setPubsubLogs(['[READY] 发布订阅原型已就绪', '[SUB] Subscriber A → #news', '[SUB] Subscriber B → #news']);
    recoverySnapshotRef.current = [];
    setResetOpen(false);
  };

  const displayedKeyCount = serverState === 'RECOVERING' ? recoveredCount : entries.length;
  const displayedClients = concurrencyStatus === 'RUNNING' ? concurrencyClients : serverState === 'OFFLINE' ? 0 : 3;
  const liveQps = serverState !== 'ONLINE' ? 0 : concurrencyStatus === 'RUNNING' ? concurrencyThroughput : benchmarkStatus === 'RUNNING' ? 8_640 : 1_204;
  const successRate = ((totalRequests - failedRequests) / totalRequests * 100).toFixed(2);
  const metricStrip: LabMetricItem[] = [
    { label: '键总数', value: formatNumber(displayedKeyCount), accent: 'cyan' },
    { label: '活跃客户端', value: formatNumber(displayedClients), accent: 'violet' },
    { label: '实时吞吐', value: formatNumber(liveQps), suffix: '请求/秒', accent: 'green' },
    { label: '累计请求', value: totalRequests >= 1_000_000 ? `${(totalRequests / 1_000_000).toFixed(2)}M` : formatNumber(totalRequests), accent: 'blue' },
    { label: '成功率', value: `${successRate}%`, accent: 'green' },
    { label: 'WAL 大小', value: formatBytes(walBytes), accent: 'orange' },
  ];
  const activeNavigation = navigation.find((item) => item.id === activeTab)!;

  return (
    <main className={`lab-shell ${demoMode ? 'demo-mode' : ''} server-${serverState.toLowerCase()}`}>
      <header className="topbar">
        <div className="brand-block"><div className="brand-mark"><Database size={20} /></div><div><strong>RustKV <span>实验室</span></strong><small>Rust 网络 KV 存储 · 可视化实验与测试平台</small></div></div>
        <div className={`connection-pill state-${serverState.toLowerCase()}`}><LabStatusOrb offline={serverState !== 'ONLINE'} /><b>{serverLabels[serverState].label}</b><code>127.0.0.1:7878</code></div>
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

      <LabMetricStrip metrics={metricStrip} />

      <div className="content-area">
        <div className="page-title-row"><div><p><Activity size={14} /> RUSTKV SYSTEMS LAB / {activeNavigation.hint.toUpperCase()}</p><h1>{activeNavigation.label}</h1></div><div className={`last-update ${serverState !== 'ONLINE' ? 'frozen' : ''}`}><LabStatusOrb offline={serverState !== 'ONLINE'} /> {serverState === 'ONLINE' ? '实时更新 · 刚刚' : `${serverLabels[serverState].label} · 最后更新 ${lastUpdate}`}</div></div>

        {activeTab === 'overview' && <Overview serverState={serverState} qps={liveQps} walRecords={walRecords} walBytes={walBytes} operations={operations} lastUpdate={lastUpdate} concurrencyStatus={concurrencyStatus} concurrencyThroughput={concurrencyThroughput} recoveryPhase={recoveryPhase} recoveryLost={recoveryLost} benchmarkData={benchmarkResults} />}
        {activeTab === 'kv' && <KvOperations online={serverState === 'ONLINE'} entries={entries} now={now} operations={operations} onAction={performKvAction} />}
        {activeTab === 'concurrency' && <ConcurrencyPage online={serverState === 'ONLINE'} clients={concurrencyClients} setClients={setConcurrencyClients} requestsPerClient={requestsPerClient} setRequestsPerClient={setRequestsPerClient} workload={concurrencyWorkload} setWorkload={setConcurrencyWorkload} status={concurrencyStatus} progress={concurrencyProgress} successful={concurrencySuccessful} failed={concurrencyFailed} throughput={concurrencyThroughput} elapsed={concurrencyElapsed} onStart={startConcurrency} onStop={stopConcurrency} />}
        {activeTab === 'recovery' && <RecoveryPage serverState={serverState} phase={recoveryPhase} seedCount={recoverySeedCount} setSeedCount={setRecoverySeedCount} injectFailure={injectRecoveryFailure} setInjectFailure={setInjectRecoveryFailure} beforeCount={recoveryBeforeCount} recoveredCount={recoveredCount} recoveryLost={recoveryLost} progress={recoveryProgress} logs={recoveryLogs} beforeFingerprint={beforeFingerprint} afterFingerprint={afterFingerprint} sampleBefore={recoverySamples} currentEntries={entries} onSeed={seedRecoveryData} onKill={killServer} onRestart={restartServer} compactRunning={compactRunning} compactProgress={compactProgress} compactResult={compactResult} onCompact={runCompact} />}
        {activeTab === 'performance' && <PerformancePage online={serverState === 'ONLINE'} scales={benchmarkScales} setScales={setBenchmarkScales} workload={benchmarkWorkload} setWorkload={setBenchmarkWorkload} status={benchmarkStatus} progress={benchmarkProgress} results={benchmarkResults} onStart={startBenchmark} onStop={stopBenchmark} />}
        {activeTab === 'pubsub' && <PubSubPage online={serverState === 'ONLINE'} subscribers={subscribers} channel={pubsubChannel} setChannel={setPubsubChannel} message={pubsubMessage} setMessage={setPubsubMessage} publishing={publishing} logs={pubsubLogs} onPublish={publishMessage} onAddSubscriber={addSubscriber} onToggleSubscriber={toggleSubscriber} />}
      </div>

      <AlertDialog open={resetOpen} onOpenChange={setResetOpen}>
        <AlertDialogContent className="reset-dialog">
          <AlertDialogHeader><AlertDialogTitle>重置整个演示环境？</AlertDialogTitle><AlertDialogDescription>这会清空当前实验进度、恢复初始键值、重新将服务设为在线，并还原基准测试与订阅者。</AlertDialogDescription></AlertDialogHeader>
          <AlertDialogFooter><AlertDialogCancel>取消</AlertDialogCancel><AlertDialogAction onClick={resetLab}><RotateCcw /> 确认重置</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </main>
  );
}
