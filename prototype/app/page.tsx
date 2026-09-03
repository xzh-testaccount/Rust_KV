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
  ListChecks,
  List,
  Network,
  Pause,
  Play,
  Plus,
  Presentation,
  RefreshCcw,
  RotateCcw,
  Search,
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
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ReferenceDot,
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
  LAB_BENCHMARK_SERIES_COLORS,
  type LabMetricItem,
} from '@/components/design_lab';
import {
  RustKvApiError,
  compactRemoteStorage,
  killRemoteRecovery,
  prepareRemoteRecovery,
  readRemoteBenchmark,
  readRemoteConcurrency,
  readRemoteRecovery,
  readRemoteStorageState,
  resetRemoteBenchmarkEnvironment,
  restartRemoteRecovery,
  sendKvCommand,
  startRemoteBenchmark,
  startRemoteConcurrency,
  stopRemoteBenchmark,
  stopRemoteConcurrency,
  type RemoteRecoveryState,
  type RemoteStorageCompactResult,
  type RemoteStorageState,
} from '@/lib/rustkv-api';

type TabId =
  | 'overview'
  | 'kv'
  | 'concurrency'
  | 'recovery'
  | 'performance';

type ServerState = 'ONLINE' | 'OFFLINE' | 'STARTING' | 'RECOVERING' | 'ERROR';
type OperationTone = 'read' | 'write' | 'delete' | 'system' | 'error';
type ExperimentStatus = 'IDLE' | 'RUNNING' | 'COMPLETED' | 'STOPPED' | 'INTERRUPTED';
type RecoveryPhase = 'IDLE' | 'PREPARED' | 'CRASHED' | 'RECOVERING' | 'VERIFIED' | 'ERROR';
type Workload = 'READ_HEAVY' | 'MIXED' | 'WRITE_HEAVY';
type RuntimeModel = 'Sync' | 'Async';
type LockStrategy = 'Mutex' | 'RwLock';
type ExperimentMode = 'SINGLE' | 'COMPARE';
type ResearchVariable = 'RUNTIME' | 'LOCK' | 'WORKLOAD';
type BenchmarkPreset = 'A' | 'B' | 'C' | null;
type ScaleStatus = 'WAITING' | 'RUNNING' | 'DONE' | 'FAILED';

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
};

type BenchmarkConfig = {
  runtime: RuntimeModel;
  lock: LockStrategy;
  workload: Workload;
  requests: number;
};

type BenchmarkSeries = {
  id: 'single' | 'a' | 'b' | 'c';
  label: string;
  role: '单次实验' | '对照组' | '实验组';
  config: BenchmarkConfig;
  points: BenchmarkPoint[];
  scaleStatus: Record<number, ScaleStatus>;
};

type BenchmarkJob = {
  seriesId: BenchmarkSeries['id'];
  clients: number;
};

const INITIAL_KEY_COUNT = 327;
const BENCHMARK_REQUESTS = 10_000;
const BENCHMARK_SCALES = [1, 10, 50, 100] as const;

type StorageHistoryBar = {
  id: 'basic' | 'advanced-before' | 'advanced-after';
  label: string;
  value: number | null;
  display: string;
};

type StorageHistoryMetric = {
  id: string;
  label: string;
  scale: string;
  note: string;
  bars: StorageHistoryBar[];
};

const STORAGE_HISTORY_METRICS: StorageHistoryMetric[] = [
  {
    id: 'disk',
    label: '持久化文件大小',
    scale: '独立尺度：0–233.3 KiB，越小越好',
    note: 'Snapshot 发布并清理旧 WAL 后，空间较压缩前减少 98.1%。',
    bars: [
      { id: 'basic', label: '基础 WAL', value: 166, display: '166.0 KiB' },
      { id: 'advanced-before', label: '创新版压缩前', value: 233.3, display: '233.3 KiB' },
      { id: 'advanced-after', label: 'Snapshot + 压缩后', value: 4.4, display: '4.4 KiB' },
    ],
  },
  {
    id: 'recovery',
    label: '启动恢复时间',
    scale: '独立尺度：0–3.3 ms，越小越好',
    note: '压缩后先加载最终状态快照，再重放少量增量 WAL。',
    bars: [
      { id: 'basic', label: '基础 WAL', value: 1.8, display: '1.8 ms' },
      { id: 'advanced-before', label: '创新版压缩前', value: 3.3, display: '3.3 ms' },
      { id: 'advanced-after', label: 'Snapshot + 压缩后', value: 1.1, display: '1.1 ms' },
    ],
  },
  {
    id: 'compact',
    label: 'Compact 一次性暂停',
    scale: '独立尺度：0–12.0 ms，展示操作成本',
    note: '基础版没有 Compact；创新版以一次短暂停换取后续空间与恢复收益。',
    bars: [
      { id: 'basic', label: '基础 WAL', value: null, display: '不支持' },
      { id: 'advanced-after', label: 'Snapshot + WAL 压缩', value: 12, display: '12.0 ms' },
    ],
  },
  {
    id: 'writes',
    label: '2,000 次正常写入',
    scale: '独立尺度：0–4,847.9 ms，越小越好',
    note: '两次结果接近；小幅差异只作实测记录，不宣称写入性能必然提升。',
    bars: [
      { id: 'basic', label: '基础 WAL', value: 4847.9, display: '4,847.9 ms' },
      { id: 'advanced-after', label: '创新版', value: 4631.3, display: '4,631.3 ms' },
    ],
  },
];

const workloadMeta: Record<Workload, { label: string; ratio: string; short: string }> = {
  READ_HEAVY: { label: 'Read Heavy', ratio: '90R / 10W', short: '读多写少' },
  MIXED: { label: 'Mixed', ratio: '50R / 50W', short: '均衡读写' },
  WRITE_HEAVY: { label: 'Write Heavy', ratio: '10R / 90W', short: '写入为主' },
};

const researchVariableLabels: Record<ResearchVariable, string> = {
  RUNTIME: '并发模型',
  LOCK: '锁策略',
  WORKLOAD: '工作负载',
};

const navigation = [
  { id: 'overview' as const, label: '系统总览', hint: 'Overview', icon: LayoutDashboard },
  { id: 'kv' as const, label: '键值操作', hint: 'KV Operations', icon: Database },
  { id: 'concurrency' as const, label: '并发实验', hint: 'Concurrency', icon: Network },
  { id: 'recovery' as const, label: '崩溃恢复', hint: 'Crash Recovery', icon: ShieldCheck },
  { id: 'performance' as const, label: '性能实验室', hint: 'Performance Lab', icon: Gauge },
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
  { key: 'feature:performance_lab', value: 'ready' },
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
  if (value < 1024) return `${formatNumber(value)} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / 1024 / 1024).toFixed(2)} MiB`;
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

function fingerprintFor(entries: KvEntry[]) {
  let hash = 2166136261;
  const ordered = [...entries].sort((left, right) => left.key < right.key ? -1 : left.key > right.key ? 1 : 0);
  for (const entry of ordered) {
    const text = `${entry.key}=${entry.value};`;
    for (let index = 0; index < text.length; index += 1) {
      hash ^= text.charCodeAt(index);
      hash = Math.imul(hash, 16777619);
    }
  }
  return `0x${(hash >>> 0).toString(16).toUpperCase().padStart(8, '0')}`;
}

const initialOperations: OperationLog[] = [];

function makeScaleStatus(scales: number[]): Record<number, ScaleStatus> {
  return Object.fromEntries(scales.map((scale) => [scale, 'WAITING'])) as Record<number, ScaleStatus>;
}

function makeBenchmarkPoint(config: BenchmarkConfig, clients: number): BenchmarkPoint {
  const scaleIndex = BENCHMARK_SCALES.indexOf(clients as (typeof BENCHMARK_SCALES)[number]);
  const baseThroughput = [1580, 7680, 11_920, 11_180][Math.max(0, scaleIndex)];
  const baseP99 = [1.35, 3.05, 8.4, 15.2][Math.max(0, scaleIndex)];
  const concurrencyFactor = config.runtime === 'Async'
    ? [0.97, 1.04, 1.16, 1.24][Math.max(0, scaleIndex)]
    : [1.03, 1, 0.9, 0.78][Math.max(0, scaleIndex)];
  const lockFactor = config.lock === 'RwLock'
    ? config.workload === 'READ_HEAVY'
      ? [1.02, 1.08, 1.25, 1.38][Math.max(0, scaleIndex)]
      : config.workload === 'WRITE_HEAVY'
        ? [0.99, 0.96, 0.91, 0.87][Math.max(0, scaleIndex)]
        : [1, 1.03, 1.08, 1.1][Math.max(0, scaleIndex)]
    : 1;
  const workloadFactor = config.workload === 'READ_HEAVY' ? 1.16 : config.workload === 'WRITE_HEAVY' ? 0.69 : 1;
  const qps = Math.round(baseThroughput * concurrencyFactor * lockFactor * workloadFactor);
  const workloadLatency = config.workload === 'READ_HEAVY' ? 0.82 : config.workload === 'WRITE_HEAVY' ? 1.48 : 1;
  const runtimeLatency = config.runtime === 'Async'
    ? [1.04, 0.98, 0.85, 0.78][Math.max(0, scaleIndex)]
    : [0.96, 1, 1.12, 1.28][Math.max(0, scaleIndex)];
  const lockLatency = config.lock === 'RwLock'
    ? config.workload === 'READ_HEAVY'
      ? [0.98, 0.92, 0.78, 0.68][Math.max(0, scaleIndex)]
      : config.workload === 'WRITE_HEAVY'
        ? [1.01, 1.06, 1.16, 1.25][Math.max(0, scaleIndex)]
        : [1, 0.98, 0.94, 0.92][Math.max(0, scaleIndex)]
    : 1;
  const p99 = Number((baseP99 * workloadLatency * runtimeLatency * lockLatency).toFixed(2));
  return {
    clients,
    qps,
    p50: Number((p99 * 0.22).toFixed(2)),
    p95: Number((p99 * 0.61).toFixed(2)),
    p99,
  };
}

function buildBenchmarkSeries(
  mode: ExperimentMode,
  researchVariable: ResearchVariable,
  baseConfig: BenchmarkConfig,
  scales: number[],
): BenchmarkSeries[] {
  const makeSeries = (
    id: BenchmarkSeries['id'],
    label: string,
    role: BenchmarkSeries['role'],
    config: BenchmarkConfig,
  ): BenchmarkSeries => ({ id, label, role, config, points: [], scaleStatus: makeScaleStatus(scales) });

  if (mode === 'SINGLE') {
    return [makeSeries('single', `${baseConfig.runtime} · ${baseConfig.lock} · ${workloadMeta[baseConfig.workload].label}`, '单次实验', baseConfig)];
  }
  if (researchVariable === 'RUNTIME') {
    return [
      makeSeries('a', 'Sync', '对照组', { ...baseConfig, runtime: 'Sync' }),
      makeSeries('b', 'Async', '实验组', { ...baseConfig, runtime: 'Async' }),
    ];
  }
  if (researchVariable === 'LOCK') {
    return [
      makeSeries('a', 'Mutex', '对照组', { ...baseConfig, lock: 'Mutex' }),
      makeSeries('b', 'RwLock', '实验组', { ...baseConfig, lock: 'RwLock' }),
    ];
  }
  return [
    makeSeries('a', 'Read Heavy', '对照组', { ...baseConfig, workload: 'READ_HEAVY' }),
    makeSeries('b', 'Mixed', '实验组', { ...baseConfig, workload: 'MIXED' }),
    makeSeries('c', 'Write Heavy', '实验组', { ...baseConfig, workload: 'WRITE_HEAVY' }),
  ];
}

function percentageChange(from: number, to: number) {
  if (!from) return 0;
  return (to - from) / from * 100;
}

function formatSignedPercentage(value: number) {
  const normalized = Math.abs(value) < 0.05 ? 0 : value;
  return `${normalized > 0 ? '+' : ''}${normalized.toFixed(1)}%`;
}

function workloadForApi(workload: Workload): 'read' | 'mixed' | 'write' {
  if (workload === 'READ_HEAVY') return 'read';
  if (workload === 'WRITE_HEAVY') return 'write';
  return 'mixed';
}

function isValidBenchmarkPoint(point: BenchmarkPoint) {
  return [point.clients, point.qps, point.p50, point.p95, point.p99].every(Number.isFinite)
    && point.clients > 0
    && point.qps >= 0
    && point.p50 >= 0
    && point.p50 <= point.p95
    && point.p95 <= point.p99;
}

function Overview({
  backendMode,
  serverState,
  operations,
  lastUpdate,
  concurrencyStatus,
  concurrencyFailed,
  recoveryPhase,
  recoveryLost,
  benchmarkStatus,
  benchmarkSeries,
}: {
  backendMode: boolean;
  serverState: ServerState;
  operations: OperationLog[];
  lastUpdate: string;
  concurrencyStatus: ExperimentStatus;
  concurrencyFailed: number;
  recoveryPhase: RecoveryPhase;
  recoveryLost: number;
  benchmarkStatus: ExperimentStatus;
  benchmarkSeries: BenchmarkSeries[];
}) {
  const online = serverState === 'ONLINE';
  const benchmarkPointCount = benchmarkSeries.reduce((count, series) => count + series.points.length, 0);
  return (
    <section className="overview-grid lab-page" aria-label="系统总览">
      <LabPanel className={`server-card ${online ? '' : 'offline-panel'}`}>
        <div className="panel-kicker"><Server size={15} /> 服务节点</div>
        <div className={`server-state state-${serverState.toLowerCase()}`}><LabStatusOrb offline={!online} /> {serverLabels[serverState].label}</div>
        <p className="muted">{backendMode ? '控制器 127.0.0.1:7879 → RustKV 127.0.0.1:7878' : '纯前端 UI / 动画测试 · 不连接后端'}</p>
        <div className="frontend-scope">
          <div><span>运行模式</span><strong>{backendMode ? '答辩模式 · 后端实测' : '纯前端模式 · UI 测试'}</strong></div>
          <div><span>状态来源</span><strong>{backendMode ? '接入层接口响应' : '前端本地状态机'}</strong></div>
          <div><span>网络请求</span><strong>{backendMode ? '按操作发送' : '不发送'}</strong></div>
        </div>
        <div className={online ? 'healthy-row' : 'error-row'}>
          {online ? <CheckCircle2 size={15} /> : <WifiOff size={15} />}
          {online ? (backendMode ? '接入层状态正常，可开始答辩实测' : '前端交互状态正常，可测试 UI 与动画') : `${backendMode ? '后端连接' : '本地模拟'}已离线 · ${lastUpdate}`}
        </div>
      </LabPanel>

      <LabPanel className="experiment-card">
        <LabPanelHeader icon={<Boxes size={15} />} eyebrow="答辩流程" title="四步实验进度" action={<span className="prototype-label">{backendMode ? '后端实测模式' : '纯前端 UI 测试'}</span>} />
        <div className="experiment-list">
          <div><span className="experiment-icon cyan"><Network size={17} /></span><p><strong>多客户端并发</strong><small>只验证完成数、成功数与失败数</small></p><b>{concurrencyStatus === 'COMPLETED' ? (concurrencyFailed ? 'FAIL' : 'PASS') : '—'} <small>正确性</small></b><em>{concurrencyStatus === 'RUNNING' ? '运行中' : concurrencyStatus === 'COMPLETED' ? '已校验' : concurrencyStatus === 'STOPPED' || concurrencyStatus === 'INTERRUPTED' ? '未完成' : '待运行'}</em></div>
          <div><span className="experiment-icon green"><ShieldCheck size={17} /></span><p><strong>崩溃恢复</strong><small>持久化 Snapshot + 增量 WAL</small></p><b>{recoveryPhase === 'VERIFIED' ? recoveryLost : '—'} <small>丢失键</small></b><em>{recoveryPhase === 'VERIFIED' ? (recoveryLost > 0 ? 'CONSISTENCY FAIL' : 'CONSISTENCY PASS') : '待运行'}</em></div>
          <div><span className="experiment-icon violet"><Gauge size={17} /></span><p><strong>性能实验室</strong><small>控制变量 · 自动多规模 / A/B</small></p><b>{benchmarkPointCount || '—'} <small>已收集点</small></b><em>{benchmarkStatus === 'RUNNING' ? '运行中' : benchmarkStatus === 'COMPLETED' ? '已完成' : benchmarkStatus === 'INTERRUPTED' ? '可重试' : '待运行'}</em></div>
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
  backendMode,
  online,
  entries,
  operations,
  onAction,
}: {
  backendMode: boolean;
  online: boolean;
  entries: KvEntry[];
  operations: OperationLog[];
  onAction: (action: KvAction, key: string, value: string) => Promise<KvResult>;
}) {
  const [key, setKey] = useState('course:name');
  const [value, setValue] = useState('Rust 网络 KV 存储');
  const [search, setSearch] = useState('');
  const [page, setPage] = useState(0);
  const [view, setView] = useState<'grid' | 'list'>('grid');
  const [pending, setPending] = useState(false);
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
    title: backendMode ? '后端不可达' : '本地测试状态为离线',
    message: backendMode ? '请确认本地控制器与 RustKV Server 已启动，再重新连接。' : '请先在“崩溃恢复”页面执行模拟重启。',
  };

  const execute = async (action: KvAction) => {
    setPending(true);
    try {
      const nextResult = await onAction(action, key, value);
      setResult(nextResult);
      if (action === 'GET' && nextResult.value !== undefined) setValue(nextResult.value);
    } finally {
      setPending(false);
    }
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
          <LabField label="键 Key" htmlFor="kv-key"><Input id="kv-key" value={key} disabled={!online || pending} onChange={(event) => setKey(event.target.value)} placeholder="例如 user:1001" /></LabField>
          <LabField label="值 Value" htmlFor="kv-value"><Input id="kv-value" value={value} disabled={!online || pending} onChange={(event) => setValue(event.target.value)} placeholder="请输入字符串值" /></LabField>
        </div>
        <div className="kv-actions">
          <LabButton tone="success" disabled={!online || pending} onClick={() => execute('SET')}><Plus /> 写入 SET</LabButton>
          <LabButton tone="info" disabled={!online || pending} onClick={() => execute('GET')}><Search /> 读取 GET</LabButton>
          <LabButton tone="danger" disabled={!online || pending} onClick={() => execute('DELETE')}><Trash2 /> 删除</LabButton>
          <LabButton tone="secondary" disabled={!online || pending} onClick={() => execute('KEYS')}><List /> {pending ? '请求中…' : '查看全部键'}</LabButton>
        </div>
        <div className={`command-result ${visibleResult.kind}`} aria-live="polite">
          <span>{visibleResult.kind === 'success' ? <CheckCircle2 /> : visibleResult.kind === 'error' ? <CircleAlert /> : <TerminalSquare />}</span>
          <div><small>最近结果</small><strong>{visibleResult.title}</strong><p>{visibleResult.message}</p></div>
        </div>
        <div className="protocol-note secondary-detail"><code>{'>'} {backendMode ? 'POST /api/kv' : 'LOCAL STATE'}</code><span>{backendMode ? '答辩模式只在控制器返回成功后更新页面，不会降级为模拟结果。' : '本页只更新浏览器内存，不发送 TCP 或 HTTP 请求。'}</span></div>
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
                  <small>{backendMode ? 'RustKV 后端数据' : '仅浏览器内存 · 非实测'}</small>
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
          )) : <div className="empty-log">暂无{backendMode ? '后端请求' : '本地'}操作记录。</div>}
        </div>
      </LabPanel>
    </section>
  );
}

function ConcurrencyPage({
  backendMode,
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
  stopping,
  onStart,
  onStop,
}: {
  backendMode: boolean;
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
  stopping: boolean;
  onStart: () => void;
  onStop: () => void;
}) {
  const running = status === 'RUNNING';
  const controlsLocked = running || stopping;
  const total = clients * requestsPerClient;
  const renderedClients = Array.from({ length: Math.min(100, clients) }, (_, index) => index);
  const gridColumns = clients <= 1 ? 1 : clients <= 10 ? Math.min(5, clients) : 10;
  const currentCompleted = successful + failed;
  const passed = status === 'COMPLETED' && progress >= 100 && failed === 0 && currentCompleted === total;

  return (
    <section className="concurrency-page lab-page">
      <LabPanel className="experiment-config">
        <div className="config-title"><span className="panel-kicker"><Users size={15} /> 实验参数</span><h2>让多个客户端同时读写</h2></div>
        <div className="config-group"><span>客户端数量</span><div className="preset-buttons">{[1, 10, 50, 100].map((value) => <button key={value} className={clients === value ? 'active' : ''} disabled={controlsLocked} onClick={() => setClients(value)}>{value}</button>)}</div></div>
        <LabField className="number-config" label="每客户端请求数" htmlFor="requests-per-client"><Input id="requests-per-client" type="number" min="10" max="1000" value={requestsPerClient} disabled={controlsLocked} onChange={(event) => setRequestsPerClient(Math.max(10, Number(event.target.value) || 10))} /></LabField>
        <div className="config-group"><span>访问模式</span><div className="segmented">{(['READ_HEAVY', 'MIXED', 'WRITE_HEAVY'] as Workload[]).map((item) => <button key={item} className={workload === item ? 'active' : ''} disabled={controlsLocked} onClick={() => setWorkload(item)}>{item === 'READ_HEAVY' ? 'Read' : item === 'WRITE_HEAVY' ? 'Write' : 'Mixed'}</button>)}</div></div>
        {running ? <LabButton variant="destructive" className="run-button" onClick={onStop}><Square /> 停止实验</LabButton> : <LabButton className="run-button" disabled={!online || stopping} onClick={onStart}>{stopping ? <Activity /> : <Play />} {stopping ? '正在停止后端…' : backendMode ? '开始并发实验' : '测试并发动画'}</LabButton>}
      </LabPanel>

      <LabPanel className="client-grid-card">
        <LabPanelHeader icon={<Network size={15} />} eyebrow={`客户端活动矩阵 · ${backendMode ? '后端实验' : '纯前端动画'}`} title={`${clients} 个${backendMode ? '客户端' : '虚拟客户端'}同时工作`} action={<LabBadge variant="experiment" tone={status === 'IDLE' ? 'idle' : status === 'RUNNING' ? 'running' : status === 'COMPLETED' ? 'completed' : 'stopped'}>{status === 'IDLE' ? '准备就绪' : status === 'RUNNING' ? '运行中' : status === 'COMPLETED' ? '校验完成' : status === 'INTERRUPTED' ? '已中断' : '已停止'}</LabBadge>} />
        <div className="client-legend"><span><i className="read" />读取</span><span><i className="write" />写入</span><span><i className="delete" />删除</span><span><i className="idle" />等待</span></div>
        <div className={`client-grid ${running ? 'is-running' : ''}`} style={{ gridTemplateColumns: `repeat(${gridColumns}, minmax(0, 1fr))` }}>
          {renderedClients.map((index) => {
            const phase = (index + Math.floor(progress / 3)) % 7;
            const tone = backendMode || !running && status !== 'COMPLETED' ? 'idle' : phase < 3 ? 'read' : phase < 6 ? 'write' : 'delete';
            return <div key={index} className={`client-node ${tone}`} title={backendMode ? `客户端 ${index + 1} · 页面不伪造单客户端轨迹` : `虚拟客户端 ${index + 1}`}><span>C{index + 1}</span><i /></div>;
          })}
        </div>
        <div className="sample-stream secondary-detail">
          <span>{backendMode ? '控制器聚合状态' : '请求采样 · 非实测'}</span>
          <code>{backendMode ? `${status} · Completed ${formatNumber(currentCompleted)} · Success ${formatNumber(successful)} · Failed ${formatNumber(failed)}` : `Virtual Client ${Math.max(1, Math.min(clients, Math.floor(progress / 2) + 1))} → ${workload === 'WRITE_HEAVY' ? 'SET' : progress % 3 === 0 ? 'GET' : 'SET'} demo:key:${Math.floor(progress * 1.7)} → UI SAMPLE`}</code>
        </div>
      </LabPanel>

      <LabPanel className="concurrency-result">
        <LabPanelHeader icon={<Gauge size={15} />} eyebrow="实时结果" title="请求完成情况" />
        <div className="progress-ring" style={{ '--progress': `${progress * 3.6}deg` } as CSSProperties}><div><strong>{Math.round(progress)}%</strong><span>{formatNumber(currentCompleted)} / {formatNumber(total)}</span></div></div>
        <div className="result-kpis"><div><span>总请求</span><strong>{formatNumber(total)}</strong></div><div><span>已完成</span><strong>{formatNumber(currentCompleted)}</strong></div><div><span>Success</span><strong className="success-text">{formatNumber(successful)}</strong></div><div><span>Failed</span><strong className={failed ? 'danger-text' : ''}>{formatNumber(failed)}</strong></div></div>
        <LabProgress value={progress} className="experiment-progress" />
        <div className={`verdict-strip ${status === 'COMPLETED' ? (passed ? 'pass' : 'fail') : status === 'STOPPED' || status === 'INTERRUPTED' ? 'stopped' : ''}`}>
          {status === 'COMPLETED' ? (passed ? <CheckCircle2 /> : <CircleAlert />) : status === 'STOPPED' || status === 'INTERRUPTED' ? <Pause /> : <Activity />}
          <div><strong>{status === 'COMPLETED' ? (passed ? 'CONCURRENCY PASS' : 'CONCURRENCY FAIL') : status === 'STOPPED' ? '实验已手动停止 · 未判定' : status === 'INTERRUPTED' ? '服务离线，实验中断 · 未判定' : running ? '多个客户端正在并发执行' : online ? '参数就绪，等待开始' : '服务离线，无法开始'}</strong><span>{status === 'COMPLETED' ? (backendMode ? '所有请求完成后才给出正确性结论；本页不评价吞吐性能。' : '纯前端模式只验证 UI、进度与 PASS / FAIL 动画。') : status === 'INTERRUPTED' ? '已完成统计会保留，恢复服务后可重新运行。' : '本页只证明并发访问结果正确，性能对比统一在性能实验室进行。'}</span></div>
        </div>
      </LabPanel>
    </section>
  );
}

function RecoveryPage({
  backendMode,
  serverState,
  phase,
  seedCount,
  setSeedCount,
  actionPending,
  verifiedBySource,
  beforeCount,
  recoveredCount,
  recoveryLost,
  progress,
  logs,
  beforeFingerprint,
  afterFingerprint,
  sampleBefore,
  currentEntries,
  verificationEntries,
  walReplayCount,
  recoveryTime,
  onSeed,
  onKill,
  onRestart,
}: {
  backendMode: boolean;
  serverState: ServerState;
  phase: RecoveryPhase;
  seedCount: number;
  setSeedCount: (value: number) => void;
  actionPending: 'prepare' | 'kill' | 'restart' | null;
  verifiedBySource: boolean;
  beforeCount: number;
  recoveredCount: number;
  recoveryLost: number;
  progress: number;
  logs: string[];
  beforeFingerprint: string;
  afterFingerprint: string;
  sampleBefore: KvEntry[];
  currentEntries: KvEntry[];
  verificationEntries: KvEntry[];
  walReplayCount: number | null;
  recoveryTime: number;
  onSeed: () => void;
  onKill: () => void;
  onRestart: () => void;
}) {
  const phaseIndex = phase === 'IDLE' || phase === 'ERROR' ? 0 : phase === 'PREPARED' ? 1 : phase === 'CRASHED' ? 2 : phase === 'RECOVERING' ? 3 : 4;
  const verified = phase === 'VERIFIED';
  const samplesMatch = sampleBefore.every((sample) => verificationEntries.find((entry) => entry.key === sample.key)?.value === sample.value);
  const pass = verified && verifiedBySource && beforeCount > 0 && recoveredCount === beforeCount && recoveryLost === 0 && beforeFingerprint === afterFingerprint && samplesMatch;
  const offline = serverState === 'OFFLINE';
  const steps = ['准备数据', '强制断电', '重启服务', 'WAL 重放', '自动校验'];
  const beforeDisplay: number | string = beforeCount || '—';
  const afterDisplay = verified ? recoveredCount : phase === 'RECOVERING' ? recoveredCount : phase === 'CRASHED' ? 0 : '—';
  const memoryCount = offline
    ? 0
    : phase === 'RECOVERING' || verified
      ? recoveredCount
      : backendMode && phase === 'PREPARED'
        ? beforeCount
        : currentEntries.length;

  return (
    <section className={`recovery-page lab-page ${offline ? 'power-cut-mode' : ''}`}>
      <div className="recovery-steps" aria-label="恢复实验步骤">
        {steps.map((step, index) => <div key={step} className={`${index < phaseIndex || verified ? 'done' : ''} ${index === phaseIndex && !verified ? 'active' : ''}`}><span>{index < phaseIndex || verified ? <Check /> : index + 1}</span><b>{step}</b>{index < steps.length - 1 && <i />}</div>)}
      </div>

      <LabPanel className="recovery-control">
        <LabPanelHeader icon={<ShieldAlert size={15} />} eyebrow={`断电实验控制 · ${backendMode ? '后端进程' : '纯前端动画'}`} title="Seed → Kill → Restart → Verify" action={<LabStatusPill tone={serverState === 'ONLINE' ? 'online' : serverState === 'OFFLINE' ? 'offline' : 'warning'}>{serverLabels[serverState].label}</LabStatusPill>} />
        <div className="seed-presets"><span>准备演示数据</span><div>{[50, 100, 500, 1000].map((value) => <button key={value} className={seedCount === value ? 'active' : ''} disabled={serverState !== 'ONLINE' || phase === 'RECOVERING' || actionPending !== null} onClick={() => setSeedCount(value)}>{value} 键</button>)}</div></div>
        <div className="recovery-actions">
          <LabButton variant="secondary" disabled={serverState !== 'ONLINE' || phase === 'RECOVERING' || actionPending !== null} onClick={onSeed}><Database /> {actionPending === 'prepare' ? '正在准备真实证据…' : '① Seed Data + Before'}</LabButton>
          <LabButton variant="destructive" className="kill-button" disabled={phase !== 'PREPARED' || serverState !== 'ONLINE' || actionPending !== null} onClick={onKill}><Zap /> {actionPending === 'kill' ? '正在强制终止…' : '② Kill Server'}</LabButton>
          <LabButton className="restart-button" disabled={phase !== 'CRASHED' || actionPending !== null} onClick={onRestart}><RefreshCcw /> {actionPending === 'restart' ? '正在启动恢复…' : '③ Restart + Snapshot / WAL'}</LabButton>
        </div>
        <div className="recovery-evidence">
          <div className={serverState === 'OFFLINE' ? 'lost' : ''}><HardDrive /><span>Memory Store</span><strong>{memoryCount} Keys</strong><small>易失 · Kill 后清空</small></div>
          <div className="wal-source"><FileClock /><span>Snapshot + WAL</span><strong>{phase === 'IDLE' ? '等待 Seed' : phase === 'ERROR' ? '状态未知' : 'Preserved'}</strong><small>真实恢复来源 · 快照后重放增量</small></div>
          <div className="verify-only"><ListChecks /><span>{backendMode ? 'Backend Before Evidence' : 'Frontend Snapshot'}</span><strong>{beforeCount || '—'} Keys</strong><small>{backendMode ? '控制器返回，仅用于校验' : '只用于 Before / After 校验'}</small></div>
        </div>
      </LabPanel>

      <LabPanel className={`recovery-proof ${verified ? (pass ? 'proof-pass' : 'proof-fail') : ''}`}>
        <LabPanelHeader icon={<ShieldCheck size={15} />} eyebrow="自动一致性校验" title="持久化数据重建的 Memory 是否等于 Before？" action={verified ? <LabBadge variant="proof" tone={pass ? 'pass' : 'fail'}>{pass ? 'CONSISTENCY PASS' : 'CONSISTENCY FAIL'}</LabBadge> : <LabBadge variant="proof" tone="waiting">等待实验</LabBadge>} />
        <div className="recovery-metrics">
          <div><span>Before Keys</span><strong>{typeof beforeDisplay === 'number' ? formatNumber(beforeDisplay) : beforeDisplay}</strong><small>{backendMode ? 'Controller Evidence' : 'Frontend Snapshot'}</small></div>
          <div><span>Recovered Keys</span><strong>{typeof afterDisplay === 'number' ? formatNumber(afterDisplay) : afterDisplay}</strong><small>Memory Store</small></div>
          <div className={verified && recoveryLost ? 'danger' : ''}><span>Lost Keys</span><strong>{verified ? recoveryLost : '—'}</strong><small>Before − After</small></div>
          <div><span>增量 WAL Replay</span><strong>{walReplayCount === null ? '—' : formatNumber(walReplayCount)}</strong><small>{walReplayCount === null ? '等待接入层返回' : 'Records'}</small></div>
          <div><span>Recovery Time</span><strong>{recoveryTime ? recoveryTime.toFixed(2) : '—'}</strong><small>Seconds</small></div>
        </div>
        <div className="hash-compare">
          <span>Before / After Hash</span><code>{beforeFingerprint || '等待快照'}</code><ArrowRight /><code>{afterFingerprint || '等待恢复'}</code><em className={verified ? (beforeFingerprint === afterFingerprint ? 'pass' : 'fail') : ''}>{verified ? (beforeFingerprint === afterFingerprint ? 'MATCH' : 'MISMATCH') : 'WAITING'}</em>
        </div>
        <div className="integrity-checks">
          {sampleBefore.slice(0, 3).map((sample) => {
            const after = verificationEntries.find((entry) => entry.key === sample.key);
            const same = verified && after?.value === sample.value;
            return <div key={sample.key}><span>抽样值</span><code>{sample.key}</code><ArrowRight /><code>{verified ? (after?.value ?? 'MISSING') : sample.value}</code><em className={verified ? (same ? 'pass' : 'fail') : ''}>{verified ? (same ? '正确' : '错误') : '待校验'}</em></div>;
          })}
        </div>
        <div className={`big-verdict ${verified ? (pass ? 'pass' : 'fail') : 'waiting'}`}>
          {verified ? (pass ? <CheckCircle2 /> : <CircleAlert />) : <ShieldCheck />}
          <div><strong>{verified ? (pass ? 'CONSISTENCY PASS' : 'CONSISTENCY FAIL') : phase === 'ERROR' ? 'RECOVERY ERROR' : '等待 Restart 后自动比对'}</strong><span>{verified ? (pass ? `${formatNumber(beforeCount)} 个键由持久化 Snapshot + WAL 重建，数量、抽样值与 Hash 一致。` : `恢复证据未通过一致性校验；Before Evidence 只负责发现差异，从未参与恢复。`) : backendMode ? '恢复来自持久化 Snapshot + 增量 WAL；控制器 Before / After 只负责校验。' : '纯前端只播放 WAL 恢复动画；Frontend Verification Snapshot 只提供校验基线。'}</span></div>
        </div>
      </LabPanel>

      <LabPanel className="replay-panel">
        <LabPanelHeader icon={<TerminalSquare size={15} />} eyebrow={`Storage Replay · ${backendMode ? '恢复过程' : 'UI 动画'}`} title={backendMode ? '从 Snapshot + 增量 WAL 重建 Memory Store' : '从模拟 WAL 重建 Memory Store'} action={<span className="replay-percent">{Math.round(progress)}%</span>} />
        <LabProgress value={progress} className="replay-progress" />
        <div className="replay-counter"><span>已恢复</span><strong>{formatNumber(phase === 'RECOVERING' || verified ? recoveredCount : 0)}</strong><small>/ {typeof beforeDisplay === 'number' ? formatNumber(beforeDisplay) : beforeDisplay} 键</small></div>
        <div className="replay-log">
          {logs.length ? logs.slice(-9).map((log, index) => <code key={`${log}-${index}`}><span>{String(index + 1).padStart(2, '0')}</span>{log}</code>) : <div className="empty-log">运行断电实验后，这里会显示 Snapshot 加载与增量 WAL Replay 过程。</div>}
        </div>
      </LabPanel>
    </section>
  );
}

function StorageHistoryPanel() {
  return <LabPanel className="storage-history-comparison">
    <LabPanelHeader
      icon={<Sparkles size={15} />}
      eyebrow="B 模块提高项 · 历史实测"
      title="基础 WAL vs Snapshot + WAL 压缩"
      action={<span className="experiment-source measured">2026-09-02 · 固定负载历史实测</span>}
    />
    <div className="storage-history-legend" aria-label="历史实测系列图例">
      <span className="basic"><i />基础 WAL</span>
      <span className="advanced-before"><i />创新版压缩前</span>
      <span className="advanced-after"><i />Snapshot + 压缩后</span>
    </div>
    <div className="storage-history-conditions"><span>2,000 Operations</span><span>100 Live Keys</span><span>5 Runs Median</span><span>Release · Windows GNU</span></div>
    <div className="storage-history-grid">
      {STORAGE_HISTORY_METRICS.map((metric) => {
        const maxValue = Math.max(...metric.bars.map((bar) => bar.value ?? 0), 1);
        return <article key={metric.id} className="storage-history-metric">
          <div className="storage-history-title"><strong>{metric.label}</strong><small>{metric.scale}</small></div>
          <div className="storage-history-bars">
            {metric.bars.map((bar) => <div key={bar.id} className="storage-history-row">
              <span>{bar.label}</span>
              <div className="storage-history-track"><i className={bar.id} style={{ width: bar.value === null ? 0 : `${bar.value / maxValue * 100}%` }} /></div>
              <b>{bar.display}</b>
            </div>)}
          </div>
          <p>{metric.note}</p>
        </article>;
      })}
    </div>
    <footer>每张指标卡使用自己的纵向数值范围，柱长只能在同一卡片内比较；该区域展示已保存实验记录，不会随当前服务器状态变化。</footer>
  </LabPanel>;
}

function StorageInnovationPanels({
  backendMode,
  online,
}: {
  backendMode: boolean;
  online: boolean;
}) {
  const [storageState, setStorageState] = useState<RemoteStorageState | null>(null);
  const [compactResult, setCompactResult] = useState<RemoteStorageCompactResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [compacting, setCompacting] = useState(false);
  const [error, setError] = useState('');
  const requestEpochRef = useRef(0);
  const visibleStorageState = backendMode && online ? storageState : null;
  const visibleCompactResult = backendMode ? compactResult : null;
  const visibleError = backendMode && !online
    ? '后端当前离线，未提供实时存储数据。'
    : error;

  const refreshStorage = useCallback(async () => {
    const requestEpoch = ++requestEpochRef.current;
    if (!backendMode) {
      setStorageState(null);
      setCompactResult(null);
      setError('');
      setLoading(false);
      return;
    }
    if (!online) {
      setStorageState(null);
      setError('后端当前离线，未提供实时存储数据。');
      setLoading(false);
      return;
    }

    setLoading(true);
    setError('');
    try {
      const state = await readRemoteStorageState();
      if (requestEpoch === requestEpochRef.current) setStorageState(state);
    } catch (loadError) {
      if (requestEpoch !== requestEpochRef.current) return;
      setStorageState(null);
      setError(loadError instanceof RustKvApiError
        ? `${loadError.code} · ${loadError.message}`
        : '实时存储状态读取失败');
    } finally {
      if (requestEpoch === requestEpochRef.current) setLoading(false);
    }
  }, [backendMode, online]);

  useEffect(() => {
    const timer = window.setTimeout(() => void refreshStorage(), 0);
    return () => {
      window.clearTimeout(timer);
      requestEpochRef.current += 1;
    };
  }, [refreshStorage]);

  const compactStorage = async () => {
    if (!backendMode || !online || compacting) return;
    requestEpochRef.current += 1;
    setCompacting(true);
    setError('');
    try {
      const result = await compactRemoteStorage();
      setCompactResult(result);
      setStorageState(await readRemoteStorageState());
    } catch (compactError) {
      setStorageState(null);
      setError(compactError instanceof RustKvApiError
        ? `${compactError.code} · ${compactError.message}`
        : 'Compact 请求失败');
    } finally {
      setCompacting(false);
    }
  };

  return <LabPanel className="storage-live-state" aria-live="polite">
      <LabPanelHeader
        icon={<HardDrive size={15} />}
        eyebrow="Current Storage · 实时后端"
        title="Snapshot / WAL 状态与手动压缩"
        action={visibleStorageState
          ? <LabStatusPill tone={visibleStorageState.writable ? 'online' : 'warning'}>{visibleStorageState.writable ? 'WRITABLE' : 'READ ONLY'}</LabStatusPill>
          : <LabStatusPill tone={visibleError ? 'offline' : 'neutral'}>{loading ? 'LOADING' : backendMode ? 'NO DATA' : 'BACKEND ONLY'}</LabStatusPill>}
      />

      {visibleStorageState ? <>
        <div className="storage-engine"><span>Storage Engine</span><strong>{visibleStorageState.engine}</strong></div>
        <div className="storage-live-grid">
          <div><span>Entries</span><strong>{formatNumber(visibleStorageState.entries)}</strong></div>
          <div><span>Last Sequence</span><strong>{formatNumber(visibleStorageState.lastSequence)}</strong></div>
          <div><span>WAL Records</span><strong>{formatNumber(visibleStorageState.walRecords)}</strong></div>
          <div><span>WAL Size</span><strong>{formatBytes(visibleStorageState.walBytes)}</strong></div>
          <div><span>Snapshot Size</span><strong>{formatBytes(visibleStorageState.snapshotBytes)}</strong></div>
          <div><span>Total Persistent</span><strong>{formatBytes(visibleStorageState.totalBytes)}</strong></div>
        </div>
      </> : <div className="storage-live-empty">
        <HardDrive />
        <strong>{loading ? '正在读取真实存储状态' : backendMode ? '没有可展示的实时状态' : '进入答辩模式后读取真实后端'}</strong>
        <p>{backendMode ? visibleError || '等待 /api/storage/state 返回。' : '纯前端模式不会构造 WAL、Snapshot 或 Compact 结果。'}</p>
      </div>}

      {visibleCompactResult && <div className="compact-result">
        <span><CheckCircle2 /> 最近一次真实 Compact</span>
        <div><strong>{formatBytes(visibleCompactResult.before.totalBytes)} → {formatBytes(visibleCompactResult.after.totalBytes)}</strong><b>{visibleCompactResult.compactMs.toFixed(2)} ms</b></div>
        <small>WAL {formatNumber(visibleCompactResult.before.walRecords)} → {formatNumber(visibleCompactResult.after.walRecords)} Records · {visibleCompactResult.compacted ? 'COMPACTED' : 'NO CHANGE'}</small>
      </div>}

      {visibleError && visibleStorageState && <p className="storage-live-error"><CircleAlert /> {visibleError}</p>}
      <div className="storage-live-actions">
        <LabButton variant="outline" onClick={() => void refreshStorage()} disabled={!backendMode || !online || loading || compacting}><RefreshCcw />刷新状态</LabButton>
        <LabButton onClick={() => void compactStorage()} disabled={!backendMode || !online || loading || compacting || !visibleStorageState?.writable}>{compacting ? <Activity /> : <HardDrive />}{compacting ? '正在 Compact…' : '执行真实 Compact'}</LabButton>
      </div>
      <p className="compact-warning">Compact 会把当前最终状态原子发布为 Snapshot，再清理已覆盖的 WAL；操作期间写入会短暂停顿。</p>
    </LabPanel>;
}

function PerformancePage({
  backendMode,
  online,
  mode,
  setMode,
  researchVariable,
  setResearchVariable,
  runtime,
  setRuntime,
  lock,
  setLock,
  scales,
  setScales,
  workload,
  setWorkload,
  preset,
  onApplyPreset,
  status,
  progress,
  series,
  currentJob,
  stage,
  environmentResets,
  stopping,
  onStart,
  onStop,
  onRetry,
}: {
  backendMode: boolean;
  online: boolean;
  mode: ExperimentMode;
  setMode: (value: ExperimentMode) => void;
  researchVariable: ResearchVariable;
  setResearchVariable: (value: ResearchVariable) => void;
  runtime: RuntimeModel;
  setRuntime: (value: RuntimeModel) => void;
  lock: LockStrategy;
  setLock: (value: LockStrategy) => void;
  scales: number[];
  setScales: (value: number[]) => void;
  workload: Workload;
  setWorkload: (value: Workload) => void;
  preset: BenchmarkPreset;
  onApplyPreset: (value: Exclude<BenchmarkPreset, null>) => void;
  status: ExperimentStatus;
  progress: number;
  series: BenchmarkSeries[];
  currentJob: BenchmarkJob | null;
  stage: string;
  environmentResets: number;
  stopping: boolean;
  onStart: () => void;
  onStop: () => void;
  onRetry: () => void;
}) {
  const running = status === 'RUNNING';
  const controlsLocked = running || stopping;
  const toggleScale = (scale: number) => setScales(scales.includes(scale) ? scales.filter((value) => value !== scale) : [...scales, scale].sort((a, b) => a - b));
  const baseConfig = useMemo<BenchmarkConfig>(() => ({ runtime, lock, workload, requests: BENCHMARK_REQUESTS }), [runtime, lock, workload]);
  const previewSeries = useMemo(() => buildBenchmarkSeries(mode, researchVariable, baseConfig, scales), [mode, researchVariable, baseConfig, scales]);
  const displayedSeries = series.length ? series : previewSeries;
  const runScales = useMemo(() => {
    if (!series.length) return scales;
    return Array.from(new Set(series.flatMap((item) => Object.keys(item.scaleStatus).map(Number)))).sort((a, b) => a - b);
  }, [series, scales]);
  const allPoints = series.flatMap((item) => item.points.map((point) => ({ ...point, seriesId: item.id, seriesLabel: item.label })));
  const hasFailedScale = series.some((item) => Object.values(item.scaleStatus).includes('FAILED'));
  const hasWaitingScale = series.some((item) => Object.values(item.scaleStatus).includes('WAITING'));
  const canResume = !running && (status === 'STOPPED' || status === 'INTERRUPTED') && (hasFailedScale || hasWaitingScale);
  const completedSteps = series.reduce((count, item) => count + Object.values(item.scaleStatus).filter((value) => value === 'DONE').length, 0);
  const totalSteps = series.reduce((count, item) => count + Object.keys(item.scaleStatus).length, 0);

  const chartData = useMemo(() => runScales.map((clients) => {
    const row: Record<string, number> = { clients };
    for (const item of series) {
      const point = item.points.find((candidate) => candidate.clients === clients);
      if (!point) continue;
      row[`${item.id}Qps`] = point.qps;
      row[`${item.id}P50`] = point.p50;
      row[`${item.id}P95`] = point.p95;
      row[`${item.id}P99`] = point.p99;
    }
    return row;
  }), [runScales, series]);

  const throughputConfig = useMemo(() => Object.fromEntries(series.map((item, index) => [
    `${item.id}Qps`,
    { label: item.label, color: LAB_BENCHMARK_SERIES_COLORS[index] },
  ])) as ChartConfig, [series]);
  const latencyConfig = useMemo(() => Object.fromEntries(series.flatMap((item, index) => ([
    [`${item.id}P50`, { label: `${item.label} · P50`, color: LAB_BENCHMARK_SERIES_COLORS[index] }],
    [`${item.id}P95`, { label: `${item.label} · P95`, color: LAB_BENCHMARK_SERIES_COLORS[index] }],
    [`${item.id}P99`, { label: `${item.label} · P99`, color: LAB_BENCHMARK_SERIES_COLORS[index] }],
  ]))) as ChartConfig, [series]);

  const peakMarker = allPoints.length ? allPoints.reduce((best, point) => point.qps > best.qps ? point : best, allPoints[0]) : null;
  const turningMarker = series.flatMap((item) => {
    const ordered = [...item.points].sort((a, b) => a.clients - b.clients);
    const index = ordered.findIndex((point, pointIndex) => pointIndex > 0 && point.qps < ordered[pointIndex - 1].qps);
    return index > 0 ? [{ ...ordered[index], seriesId: item.id, seriesLabel: item.label }] : [];
  })[0] ?? null;
  const maxDifferenceMarker = mode === 'COMPARE' && series.length > 1
    ? runScales.map((clients) => {
        const points = series.map((item) => item.points.find((point) => point.clients === clients)).filter((point): point is BenchmarkPoint => Boolean(point));
        if (points.length < 2) return null;
        const max = Math.max(...points.map((point) => point.qps));
        const min = Math.min(...points.map((point) => point.qps));
        return { clients, qps: max, gap: max - min };
      }).filter((value): value is { clients: number; qps: number; gap: number } => Boolean(value)).reduce<{ clients: number; qps: number; gap: number } | null>((best, value) => !best || value.gap > best.gap ? value : best, null)
    : null;

  const commonScales = runScales.filter((scale) => series.length > 0 && series.every((item) => item.points.some((point) => point.clients === scale)));
  const summaryScale = commonScales.length ? Math.max(...commonScales) : null;
  const summaryPoints = summaryScale === null ? [] : series.map((item) => ({ item, point: item.points.find((point) => point.clients === summaryScale)! }));
  const controlSummary = summaryPoints[0];
  const comparisonSummaries = summaryPoints.slice(1).map((entry) => ({
    ...entry,
    qpsDelta: percentageChange(controlSummary.point.qps, entry.point.qps),
    p99Delta: percentageChange(controlSummary.point.p99, entry.point.p99),
  }));
  const primaryComparison = comparisonSummaries[comparisonSummaries.length - 1] ?? null;

  const fixedConditionText = series.length
    ? [
        researchVariable !== 'RUNTIME' || mode === 'SINGLE' ? series[0].config.runtime : null,
        researchVariable !== 'LOCK' || mode === 'SINGLE' ? series[0].config.lock : null,
        researchVariable !== 'WORKLOAD' || mode === 'SINGLE' ? workloadMeta[series[0].config.workload].label : null,
        `${formatNumber(series[0].config.requests)} Requests`,
      ].filter(Boolean).join(' · ')
    : '等待运行';

  let conclusion = '完成至少一个可比较的客户端规模后，系统会根据当前结果生成结论。';
  if (mode === 'SINGLE' && peakMarker) {
    conclusion = `本轮 ${peakMarker.seriesLabel} 在 ${peakMarker.clients} Clients 达到当前最高吞吐 ${formatNumber(peakMarker.qps)} req/s；该结论只描述本轮已收集数据。`;
  } else if (comparisonSummaries.length && summaryScale !== null) {
    const findings = comparisonSummaries.map((comparison) => {
      const throughputDirection = comparison.qpsDelta > 0 ? '更高' : comparison.qpsDelta < 0 ? '更低' : '相同';
      const latencyDirection = comparison.p99Delta < 0 ? '降低' : comparison.p99Delta > 0 ? '升高' : '不变';
      const mixedDirection = comparison.qpsDelta > 0 && comparison.p99Delta < 0
        ? '同时提高吞吐并降低尾延迟'
        : comparison.qpsDelta < 0 && comparison.p99Delta > 0
          ? '吞吐下降且尾延迟升高'
          : '吞吐与尾延迟方向不一致';
      return `${comparison.item.label} 的吞吐${throughputDirection}、P99 ${latencyDirection}，相对 ${controlSummary.item.label} ${mixedDirection}`;
    });
    conclusion = `在 ${summaryScale} Clients 下，${findings.join('；')}。`;
  }

  const sourceLabel = backendMode ? '后端实测结果' : '本地实验执行器 · 非实测';

  return (
    <section className="performance-page lab-page">
      <StorageHistoryPanel />
      <StorageInnovationPanels backendMode={backendMode} online={online} />
      <LabPanel className="benchmark-config">
        <div className="benchmark-config-heading">
          <div><span className="panel-kicker"><Gauge size={15} /> 可控变量性能实验室</span><h2>固定条件 → 改变一个变量 → 自动运行 → 比较结果</h2></div>
          <span className={`experiment-source ${backendMode ? 'measured' : ''}`}>{sourceLabel}</span>
        </div>
        <div className="experiment-mode" aria-label="实验方式"><span>实验方式</span><div><button type="button" className={mode === 'SINGLE' ? 'active' : ''} disabled={controlsLocked} aria-pressed={mode === 'SINGLE'} onClick={() => setMode('SINGLE')}>单次实验</button><button type="button" className={mode === 'COMPARE' ? 'active' : ''} disabled={controlsLocked} aria-pressed={mode === 'COMPARE'} onClick={() => setMode('COMPARE')}>对照实验</button></div></div>
        {mode === 'COMPARE' && <fieldset className="research-variable" disabled={controlsLocked}><legend>研究变量</legend>{(['RUNTIME', 'LOCK', 'WORKLOAD'] as ResearchVariable[]).map((value) => <label key={value}><input type="radio" name="research-variable" checked={researchVariable === value} onChange={() => setResearchVariable(value)} /><span>{researchVariableLabels[value]}</span></label>)}</fieldset>}
        <div className="parameter-grid">
          <fieldset disabled={controlsLocked || mode === 'COMPARE' && researchVariable === 'RUNTIME'}><legend>并发模型</legend>{(['Sync', 'Async'] as RuntimeModel[]).map((value) => <label key={value} aria-label={`${value} 并发模型`}><input type="radio" name="runtime-model" checked={runtime === value} onChange={() => setRuntime(value)} /><span><b>{value}</b><small>{value === 'Sync' ? 'Thread-per-connection' : 'Tokio Runtime'}</small></span></label>)}{mode === 'COMPARE' && researchVariable === 'RUNTIME' && <em>自动比较 Sync / Async</em>}</fieldset>
          <fieldset disabled={controlsLocked || mode === 'COMPARE' && researchVariable === 'LOCK'}><legend>锁策略</legend>{(['Mutex', 'RwLock'] as LockStrategy[]).map((value) => <label key={value} aria-label={`${value} 锁策略`}><input type="radio" name="lock-strategy" checked={lock === value} onChange={() => setLock(value)} /><span><b>{value}</b><small>{value === 'Mutex' ? '互斥访问' : '并发读 / 排他写'}</small></span></label>)}{mode === 'COMPARE' && researchVariable === 'LOCK' && <em>自动比较 Mutex / RwLock</em>}</fieldset>
          <fieldset className="workload-fieldset" disabled={controlsLocked || mode === 'COMPARE' && researchVariable === 'WORKLOAD'}><legend>Workload</legend>{(['READ_HEAVY', 'MIXED', 'WRITE_HEAVY'] as Workload[]).map((value) => <label key={value} aria-label={`${workloadMeta[value].label} ${workloadMeta[value].ratio}`}><input type="radio" name="performance-workload" checked={workload === value} onChange={() => setWorkload(value)} /><span><b>{workloadMeta[value].label}</b><small>{workloadMeta[value].ratio}</small></span></label>)}{mode === 'COMPARE' && researchVariable === 'WORKLOAD' && <em>自动比较三种工作负载</em>}</fieldset>
          <div className="scale-selector"><span>Clients</span><div>{BENCHMARK_SCALES.map((value) => <button key={value} type="button" className={scales.includes(value) ? 'active' : ''} disabled={controlsLocked} aria-pressed={scales.includes(value)} onClick={() => toggleScale(value)}>{value}</button>)}</div><small>按 1 → 10 → 50 → 100 顺序执行</small></div>
          <div className="request-summary"><span>Requests / Scale</span><strong>{formatNumber(BENCHMARK_REQUESTS)}</strong><small>每组条件保持一致</small></div>
        </div>
        <div className="benchmark-actions">
          <p>{backendMode ? '答辩模式会调用接入层并收集实测 QPS 与延迟分位。' : '纯前端模式只测试实验流程、动画、失败与重试，不代表 RustKV 性能。'}</p>
          {canResume && <LabButton variant="outline" onClick={onRetry} disabled={!online || stopping}><RefreshCcw /> {hasFailedScale ? 'Retry Failed Scale' : '继续未完成 Scale'}</LabButton>}
          {running ? <LabButton variant="destructive" onClick={onStop}><Pause /> 停止并保留结果</LabButton> : <LabButton disabled={!online || !scales.length || stopping} onClick={onStart}>{stopping ? <Activity /> : <Play />} {stopping ? '正在停止后端…' : mode === 'COMPARE' ? '运行对照实验' : '运行单次实验'}</LabButton>}
        </div>
      </LabPanel>

      <LabPanel className="experiment-presets">
        <LabPanelHeader icon={<Sparkles size={15} />} eyebrow="答辩预设" title="一键装载标准控制变量实验" />
        <div className="preset-cards">
          <button type="button" className={preset === 'A' ? 'active' : ''} disabled={controlsLocked} onClick={() => onApplyPreset('A')}><span>A</span><div><strong>Sync vs Async</strong><small>固定 Mutex · Mixed · 全部 Clients</small></div></button>
          <button type="button" className={preset === 'B' ? 'active' : ''} disabled={controlsLocked} onClick={() => onApplyPreset('B')}><span>B</span><div><strong>Mutex vs RwLock</strong><small>固定 Async · Read Heavy · 全部 Clients</small></div></button>
          <button type="button" className={preset === 'C' ? 'active' : ''} disabled={controlsLocked} onClick={() => onApplyPreset('C')}><span>C</span><div><strong>Workload Comparison</strong><small>固定 Async · RwLock · 全部 Clients</small></div></button>
        </div>
        <p>RwLock 的潜在优势主要来自并发读；Write Heavy 下不保证优于 Mutex。</p>
      </LabPanel>

      <LabPanel className="fixed-conditions">
        <LabPanelHeader icon={<ListChecks size={15} />} eyebrow="Fixed Conditions" title="所有对照组共享同一实验条件" action={<span className="prototype-label">WAL / sync_data 不可切换</span>} />
        <div><span>Dataset Size</span><strong>10,000 Keys</strong></div><div><span>Value Size</span><strong>128 B</strong></div><div><span>Requests / Scale</span><strong>10,000</strong></div><div><span>Persistence</span><strong>WAL + sync_data</strong></div><div><span>Protocol</span><strong>JSON Lines</strong></div><div><span>Network</span><strong>Localhost</strong></div>
      </LabPanel>

      <LabPanel className="scale-runner" aria-live="polite">
        <LabPanelHeader icon={<Activity size={15} />} eyebrow="执行序列" title={stage || '等待运行实验'} action={<div className="runner-progress"><span>{completedSteps} / {totalSteps || displayedSeries.length * scales.length} Steps</span><strong>{Math.round(progress)}%</strong></div>} />
        <div className="scale-series-list">
          {displayedSeries.map((item, seriesIndex) => <div key={item.id} className="scale-series-row"><div className="series-identity"><i style={{ background: LAB_BENCHMARK_SERIES_COLORS[seriesIndex] }} /><span><strong>{item.label}</strong><small>{item.role}</small></span></div>{runScales.map((scale) => {
            const scaleStatus = item.scaleStatus[scale] ?? 'WAITING';
            return <div key={scale} className={`scale-state ${scaleStatus.toLowerCase()}`}><span>{scale}</span>{scaleStatus === 'DONE' ? <CheckCircle2 /> : scaleStatus === 'RUNNING' ? <Activity /> : scaleStatus === 'FAILED' ? <CircleAlert /> : <i />}<small>{scaleStatus}</small></div>;
          })}</div>)}
        </div>
        <div className="runner-footer"><span>{currentJob ? `当前：${series.find((item) => item.id === currentJob.seriesId)?.label ?? currentJob.seriesId} · ${currentJob.clients} Clients` : status === 'COMPLETED' ? '全部规模已完成' : status === 'INTERRUPTED' ? '失败点已标记，可单独重试' : status === 'STOPPED' ? '已完成点保留，可继续未完成 Scale' : '运行 A 完成后恢复相同数据环境，再运行 B / C'}</span><span>Environment Resets <b>{environmentResets}</b></span><LabProgress value={progress} /></div>
      </LabPanel>

      <LabPanel className="throughput-chart-card">
        <LabPanelHeader icon={<Activity size={15} />} eyebrow="Clients vs Throughput" title="吞吐量随客户端数变化" action={<span className="prototype-label">{sourceLabel}</span>} />
        {allPoints.length ? (
          <ChartContainer config={throughputConfig} className="benchmark-chart" initialDimension={{ width: 640, height: 300 }} aria-label="客户端数量与吞吐量对照图">
            <LineChart data={chartData} margin={{ top: 28, right: 22, bottom: 4, left: 2 }}>
              <CartesianGrid vertical={false} stroke="#202b36" strokeDasharray="3 5" />
              <XAxis dataKey="clients" tickLine={false} axisLine={false} tickFormatter={(value) => `${value}C`} />
              <YAxis tickLine={false} axisLine={false} width={42} />
              <ChartTooltip content={<ChartTooltipContent indicator="line" />} />
              <Legend iconType="circle" iconSize={7} />
              {series.map((item) => <Line key={item.id} type="monotone" dataKey={`${item.id}Qps`} stroke={`var(--color-${item.id}Qps)`} strokeWidth={2.4} connectNulls={false} dot={{ r: 3 }} />)}
              {peakMarker && <ReferenceDot x={peakMarker.clients} y={peakMarker.qps} r={4} fill="var(--primary)" stroke="var(--background)" label={{ value: 'Peak', position: 'top', fill: 'var(--muted-foreground)', fontSize: 8 }} />}
              {turningMarker && <ReferenceDot x={turningMarker.clients} y={turningMarker.qps} r={4} fill="var(--benchmark-turning)" stroke="var(--background)" label={{ value: '拐点', position: 'right', fill: 'var(--muted-foreground)', fontSize: 8 }} />}
              {maxDifferenceMarker && <ReferenceDot x={maxDifferenceMarker.clients} y={maxDifferenceMarker.qps} r={4} fill="var(--benchmark-difference)" stroke="var(--background)" label={{ value: '最大差异', position: 'left', fill: 'var(--muted-foreground)', fontSize: 8 }} />}
            </LineChart>
          </ChartContainer>
        ) : <div className="empty-state chart-empty"><Gauge /><strong>等待实验数据</strong><p>每完成一个 Scale，曲线会立即增加一个数据点。</p></div>}
        <p className="chart-summary">标记整体 Peak、首个吞吐下降拐点，以及对照系列间最大差异点。</p>
      </LabPanel>

      <LabPanel className="latency-chart-card">
        <LabPanelHeader icon={<Clock3 size={15} />} eyebrow="Clients vs Tail Latency" title="P50 / P95 / P99 延迟分位" action={<span className="latency-unit">P99 重点 · ms</span>} />
        {allPoints.length ? (
          <ChartContainer config={latencyConfig} className="benchmark-chart" initialDimension={{ width: 640, height: 300 }} aria-label="客户端数量与延迟分位对照图">
            <LineChart data={chartData} margin={{ top: 28, right: 22, bottom: 4, left: 2 }}>
              <CartesianGrid vertical={false} stroke="#202b36" strokeDasharray="3 5" />
              <XAxis dataKey="clients" tickLine={false} axisLine={false} />
              <YAxis tickLine={false} axisLine={false} width={34} />
              <ReferenceLine x={50} stroke="var(--primary)" strokeDasharray="4 5" strokeOpacity={0.24} />
              <ChartTooltip content={<ChartTooltipContent indicator="line" />} />
              <Legend iconType="line" />
              {series.flatMap((item) => [
                <Line key={`${item.id}-p50`} type="monotone" dataKey={`${item.id}P50`} stroke={`var(--color-${item.id}P50)`} strokeWidth={1} strokeOpacity={0.32} dot={false} connectNulls={false} />,
                <Line key={`${item.id}-p95`} type="monotone" dataKey={`${item.id}P95`} stroke={`var(--color-${item.id}P95)`} strokeWidth={1.4} strokeOpacity={0.62} dot={false} connectNulls={false} />,
                <Line key={`${item.id}-p99`} type="monotone" dataKey={`${item.id}P99`} stroke={`var(--color-${item.id}P99)`} strokeWidth={2.6} dot={{ r: 3 }} connectNulls={false} />,
              ])}
            </LineChart>
          </ChartContainer>
        ) : <div className="empty-state chart-empty"><Clock3 /><strong>等待延迟样本</strong><p>P99 使用更粗线条；已完成点不会因后续失败而清空。</p></div>}
        <p className="chart-summary">P50 / P95 使用辅助线，P99 使用主线，便于观察尾延迟退化。</p>
      </LabPanel>

      <LabPanel className="comparison-summary">
        <LabPanelHeader icon={<ListChecks size={15} />} eyebrow="对照结果摘要" title={summaryScale === null ? '等待同规模结果配对' : `${summaryScale} Clients · 控制变量比较`} action={<span className="prototype-label">{sourceLabel}</span>} />
        {summaryPoints.length ? <>
          <div className="summary-series-cards">{summaryPoints.map(({ item, point }, index) => <div key={item.id}><span><i style={{ background: LAB_BENCHMARK_SERIES_COLORS[index] }} />{item.role}</span><strong>{item.label}</strong><dl><div><dt>Throughput</dt><dd>{formatNumber(point.qps)} <small>req/s</small></dd></div><div><dt>P50</dt><dd>{point.p50.toFixed(2)} ms</dd></div><div><dt>P95</dt><dd>{point.p95.toFixed(2)} ms</dd></div><div><dt>P99</dt><dd>{point.p99.toFixed(2)} ms</dd></div></dl></div>)}</div>
          {comparisonSummaries.length > 0 && <div className="delta-strips">{comparisonSummaries.map((entry) => <div key={entry.item.id}><span>{entry.item.label} vs {controlSummary.item.label}</span><strong>Throughput {formatSignedPercentage(entry.qpsDelta)}</strong><strong className={entry.p99Delta <= 0 ? 'good' : 'bad'}>P99 {formatSignedPercentage(entry.p99Delta)}</strong></div>)}</div>}
        </> : <div className="empty-state"><ListChecks /><strong>暂无完整对照</strong><p>A/B 在同一 Clients 规模完成后才计算百分比，缺失值不会用 0 代替。</p></div>}
      </LabPanel>

      <LabPanel className="experiment-conclusion">
        <LabPanelHeader icon={<Sparkles size={15} />} eyebrow="实验结论" title="只根据本轮已收集结果生成" action={<span className="prototype-label">{sourceLabel}</span>} />
        <div className="conclusion-facts"><div><span>研究变量</span><strong>{mode === 'SINGLE' ? '单次配置' : researchVariableLabels[researchVariable]}</strong></div><div><span>对照组</span><strong>{series[0]?.label ?? '—'}</strong></div><div><span>实验组</span><strong>{series.slice(1).map((item) => item.label).join(' / ') || '—'}</strong></div><div><span>固定条件</span><strong>{fixedConditionText}</strong></div></div>
        {primaryComparison && controlSummary && <div className="conclusion-deltas"><div><span>Throughput</span><strong>{formatNumber(controlSummary.point.qps)} → {formatNumber(primaryComparison.point.qps)}</strong><em className={primaryComparison.qpsDelta >= 0 ? 'good' : 'bad'}>{formatSignedPercentage(primaryComparison.qpsDelta)}</em></div><div><span>P99</span><strong>{controlSummary.point.p99.toFixed(2)} ms → {primaryComparison.point.p99.toFixed(2)} ms</strong><em className={primaryComparison.p99Delta <= 0 ? 'good' : 'bad'}>{formatSignedPercentage(primaryComparison.p99Delta)}</em></div></div>}
        <blockquote>{conclusion}</blockquote>
        <div className="workload-paths"><div><span>GET</span><code>Memory Read</code></div><ArrowRight /><div><span>SET</span><code>Exclusive Write → WAL → flush → sync_data → Memory Update</code></div></div>
      </LabPanel>
    </section>
  );
}

export default function Home() {
  const [activeTab, setActiveTab] = useState<TabId>('overview');
  const [backendMode, setBackendMode] = useState(false);
  const [backendProbeEpoch, setBackendProbeEpoch] = useState(0);
  const [resetOpen, setResetOpen] = useState(false);
  const [serverState, setServerState] = useState<ServerState>('ONLINE');
  const [lastUpdate, setLastUpdate] = useState('刚刚');
  const [entries, setEntries] = useState<KvEntry[]>(makeInitialEntries);
  const [operations, setOperations] = useState<OperationLog[]>(initialOperations);

  const [concurrencyClients, setConcurrencyClients] = useState(100);
  const [requestsPerClient, setRequestsPerClient] = useState(100);
  const [concurrencyWorkload, setConcurrencyWorkload] = useState<Workload>('MIXED');
  const [concurrencyStatus, setConcurrencyStatus] = useState<ExperimentStatus>('IDLE');
  const [concurrencyProgress, setConcurrencyProgress] = useState(0);
  const [concurrencySuccessful, setConcurrencySuccessful] = useState(0);
  const [concurrencyFailed, setConcurrencyFailed] = useState(0);
  const [concurrencyStopping, setConcurrencyStopping] = useState(false);

  const [recoveryPhase, setRecoveryPhase] = useState<RecoveryPhase>('IDLE');
  const [recoverySeedCount, setRecoverySeedCount] = useState(100);
  const [recoveryActionPending, setRecoveryActionPending] = useState<'prepare' | 'kill' | 'restart' | null>(null);
  const [recoveryVerified, setRecoveryVerified] = useState(false);
  const [recoveryBeforeCount, setRecoveryBeforeCount] = useState(0);
  const [recoveredCount, setRecoveredCount] = useState(0);
  const [recoveryLost, setRecoveryLost] = useState(0);
  const [recoveryProgress, setRecoveryProgress] = useState(0);
  const [recoveryLogs, setRecoveryLogs] = useState<string[]>([]);
  const [beforeFingerprint, setBeforeFingerprint] = useState('');
  const [afterFingerprint, setAfterFingerprint] = useState('');
  const [recoverySamples, setRecoverySamples] = useState<KvEntry[]>([]);
  const [recoveryVerificationEntries, setRecoveryVerificationEntries] = useState<KvEntry[]>([]);
  const [walReplayCount, setWalReplayCount] = useState<number | null>(0);
  const [recoveryTime, setRecoveryTime] = useState(0);

  const [benchmarkMode, setBenchmarkMode] = useState<ExperimentMode>('COMPARE');
  const [benchmarkResearchVariable, setBenchmarkResearchVariable] = useState<ResearchVariable>('LOCK');
  const [benchmarkRuntime, setBenchmarkRuntime] = useState<RuntimeModel>('Async');
  const [benchmarkLock, setBenchmarkLock] = useState<LockStrategy>('Mutex');
  const [benchmarkScales, setBenchmarkScales] = useState<number[]>([...BENCHMARK_SCALES]);
  const [benchmarkWorkload, setBenchmarkWorkload] = useState<Workload>('READ_HEAVY');
  const [benchmarkPreset, setBenchmarkPreset] = useState<BenchmarkPreset>('B');
  const [benchmarkStatus, setBenchmarkStatus] = useState<ExperimentStatus>('IDLE');
  const [benchmarkProgress, setBenchmarkProgress] = useState(0);
  const [benchmarkSeries, setBenchmarkSeries] = useState<BenchmarkSeries[]>([]);
  const [benchmarkCurrentJob, setBenchmarkCurrentJob] = useState<BenchmarkJob | null>(null);
  const [benchmarkStage, setBenchmarkStage] = useState('等待运行实验');
  const [benchmarkEnvironmentResets, setBenchmarkEnvironmentResets] = useState(0);
  const [benchmarkStopping, setBenchmarkStopping] = useState(false);

  const logIdRef = useRef(0);
  const concurrencyTimerRef = useRef<number | null>(null);
  const recoveryTimerRef = useRef<number | null>(null);
  const benchmarkTimerRef = useRef<number | null>(null);
  const recoverySnapshotRef = useRef<KvEntry[]>([]);
  const recoveryWalRef = useRef<KvEntry[]>([]);
  const backendFailureCountRef = useRef(0);
  const lifecycleEpochRef = useRef(0);
  const concurrencyRunRef = useRef(0);
  const concurrencyRunningRef = useRef(false);
  const recoveryRunRef = useRef(0);
  const benchmarkRunRef = useRef(0);
  const benchmarkRunningRef = useRef(false);
  const suspendHealthProbeRef = useRef(false);
  const benchmarkQueueRef = useRef<BenchmarkJob[]>([]);
  const benchmarkSeriesRef = useRef<BenchmarkSeries[]>([]);
  const benchmarkCurrentJobRef = useRef<BenchmarkJob | null>(null);
  const benchmarkCompletedRef = useRef(0);
  const benchmarkTotalRef = useRef(0);

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

  const refreshBackendEntries = useCallback(async () => {
    const keysResponse = await sendKvCommand({ cmd: 'keys' });
    if (keysResponse.kind !== 'keys') throw new RustKvApiError('INVALID_RESPONSE', 'keys 响应类型不匹配');
    const loaded: KvEntry[] = [];
    for (let start = 0; start < keysResponse.keys.length; start += 24) {
      const chunk = keysResponse.keys.slice(start, start + 24);
      const values = await Promise.all(chunk.map(async (key) => {
        const response = await sendKvCommand({ cmd: 'get', key });
        if (response.kind !== 'get') throw new RustKvApiError('INVALID_RESPONSE', 'get 响应类型不匹配');
        return { key, value: response.value };
      }));
      loaded.push(...values);
    }
    return loaded;
  }, []);

  const applyRemoteRecoveryState = useCallback((state: RemoteRecoveryState) => {
    setRecoveryPhase(state.phase);
    setRecoveryProgress(state.progress);
    setRecoveryBeforeCount(state.before?.count ?? 0);
    setRecoveredCount(state.after?.count ?? 0);
    setRecoveryLost(state.lost);
    setRecoveryVerified(state.verified);
    setBeforeFingerprint(state.before?.fingerprint ?? '');
    setAfterFingerprint(state.after?.fingerprint ?? '');
    setRecoverySamples(state.before?.samples ?? []);
    setRecoveryVerificationEntries(state.after?.samples ?? []);
    setWalReplayCount(state.walReplayCount);
    setRecoveryTime(state.recoveryTimeMs / 1000);
    setRecoveryLogs(state.logs);
    if (state.phase === 'CRASHED') {
      setEntries([]);
      setServerState('OFFLINE');
    } else if (state.phase === 'RECOVERING') {
      setServerState('RECOVERING');
    } else if (state.phase === 'ERROR') {
      setServerState('ERROR');
    } else {
      setServerState('ONLINE');
    }
    setLastUpdate(formatClock().slice(0, 8));
  }, []);

  const appendRecoveryLog = useCallback((message: string) => {
    setRecoveryLogs((previous) => [...previous, message]);
  }, []);

  useEffect(() => () => {
    clearTimer(concurrencyTimerRef);
    clearTimer(recoveryTimerRef);
    clearTimer(benchmarkTimerRef);
  }, []);

  useEffect(() => {
    if (!backendMode) {
      backendFailureCountRef.current = 0;
      return;
    }
    const epoch = lifecycleEpochRef.current;
    let cancelled = false;
    let loadedInitialStore = false;
    let probing = false;
    const probe = async () => {
      if (suspendHealthProbeRef.current || probing) return;
      probing = true;
      try {
        const response = await sendKvCommand({ cmd: 'ping' });
        if (response.kind !== 'ping') throw new RustKvApiError('INVALID_RESPONSE', 'ping 响应类型不匹配');
        if (cancelled || epoch !== lifecycleEpochRef.current || suspendHealthProbeRef.current) return;
        const shouldReloadStore = !loadedInitialStore || backendFailureCountRef.current > 0;
        if (shouldReloadStore) {
          const loaded = await refreshBackendEntries();
          if (cancelled || epoch !== lifecycleEpochRef.current || suspendHealthProbeRef.current) return;
          setEntries(loaded);
          loadedInitialStore = true;
        }
        backendFailureCountRef.current = 0;
        setServerState('ONLINE');
        setLastUpdate('刚刚');
      } catch {
        if (cancelled || epoch !== lifecycleEpochRef.current || suspendHealthProbeRef.current) return;
        backendFailureCountRef.current += 1;
        setServerState('OFFLINE');
        setLastUpdate(formatClock().slice(0, 8));
      } finally {
        probing = false;
      }
    };
    void probe();
    const healthTimer = window.setInterval(() => void probe(), 2_000);
    return () => {
      cancelled = true;
      window.clearInterval(healthTimer);
    };
  }, [backendMode, backendProbeEpoch, refreshBackendEntries]);

  const performKvAction = async (action: KvAction, key: string, value: string): Promise<KvResult> => {
    if (serverState !== 'ONLINE') {
      addOperation(action, key || '(empty)', 'error', '连接失败', '—');
      return { kind: 'error', title: '连接失败', message: backendMode ? 'RustKV 后端当前离线。请先在“崩溃恢复”页面重启服务。' : '前端测试状态已离线，请先在“崩溃恢复”页面恢复。' };
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
    const startedAt = performance.now();

    if (backendMode) {
      const requestEpoch = lifecycleEpochRef.current;
      const staleResult: KvResult = { kind: 'info', title: '请求已取消', message: '执行模式或实验状态已变化，迟到响应不会写回当前界面。' };
      const isStale = () => requestEpoch !== lifecycleEpochRef.current;
      try {
        if (action === 'SET') {
          const valueError = validateValue(value);
          if (valueError) {
            addOperation('SET', normalizedKey, 'error', valueError.title, '—');
            return valueError;
          }
          const response = await sendKvCommand({ cmd: 'set', key: normalizedKey, value });
          if (isStale()) return staleResult;
          if (response.kind !== 'set') throw new RustKvApiError('INVALID_RESPONSE', 'set 响应类型不匹配');
          setEntries((previous) => previous.some((entry) => entry.key === normalizedKey)
            ? previous.map((entry) => entry.key === normalizedKey ? { key: normalizedKey, value } : entry)
            : [{ key: normalizedKey, value }, ...previous]);
          const latency = `${(performance.now() - startedAt).toFixed(1)} ms`;
          addOperation('SET', normalizedKey, 'write', response.replaced ? '后端已更新' : '后端已创建', latency);
          return { kind: 'success', title: response.replaced ? '写入成功 · 已更新' : '写入成功 · 已创建', message: '后端成功响应表示 WAL、flush 与 sync_data 已完成。' };
        }
        if (action === 'GET') {
          const response = await sendKvCommand({ cmd: 'get', key: normalizedKey });
          if (isStale()) return staleResult;
          if (response.kind !== 'get') throw new RustKvApiError('INVALID_RESPONSE', 'get 响应类型不匹配');
          setEntries((previous) => previous.some((entry) => entry.key === normalizedKey) ? previous.map((entry) => entry.key === normalizedKey ? { key: normalizedKey, value: response.value } : entry) : [{ key: normalizedKey, value: response.value }, ...previous]);
          const latency = `${(performance.now() - startedAt).toFixed(1)} ms`;
          addOperation('GET', normalizedKey, 'read', '后端命中', latency);
          return { kind: 'success', title: '后端读取成功', message: `返回 ${response.value.length} 个字符。`, value: response.value };
        }
        if (action === 'DELETE') {
          const response = await sendKvCommand({ cmd: 'delete', key: normalizedKey });
          if (isStale()) return staleResult;
          if (response.kind !== 'delete') throw new RustKvApiError('INVALID_RESPONSE', 'delete 响应类型不匹配');
          if (!response.deleted) throw new RustKvApiError('NOT_FOUND', `键不存在：${normalizedKey}`);
          setEntries((previous) => previous.filter((entry) => entry.key !== normalizedKey));
          const latency = `${(performance.now() - startedAt).toFixed(1)} ms`;
          addOperation('DEL', normalizedKey, 'delete', '后端已删除', latency);
          return { kind: 'success', title: '后端删除成功', message: '删除记录已持久化到 WAL。' };
        }
        const loaded = await refreshBackendEntries();
        if (isStale()) return staleResult;
        setEntries(loaded);
        const latency = `${(performance.now() - startedAt).toFixed(1)} ms`;
        addOperation('KEYS', '*', 'system', `${loaded.length} 个后端键`, latency);
        return { kind: 'info', title: `共有 ${formatNumber(loaded.length)} 个后端键`, message: '已通过 KEYS + GET 刷新存储视图。' };
      } catch (error) {
        if (isStale()) return staleResult;
        const apiError = error instanceof RustKvApiError ? error : new RustKvApiError('UNKNOWN', '未知请求错误');
        if (apiError.code === 'BACKEND_UNREACHABLE' || apiError.code === 'TIMEOUT') setServerState('OFFLINE');
        if (apiError.code === 'STORAGE_ERROR') setServerState('ERROR');
        addOperation(action, normalizedKey || '*', 'error', apiError.code, `${(performance.now() - startedAt).toFixed(1)} ms`);
        return { kind: 'error', title: `${apiError.code}`, message: apiError.message };
      }
    }

    if (action === 'SET') {
      const valueError = validateValue(value);
      if (valueError) {
        addOperation('SET', normalizedKey, 'error', valueError.title, '—');
        return valueError;
      }
      setEntries((previous) => existing ? previous.map((entry) => entry.key === normalizedKey ? { key: normalizedKey, value } : entry) : [{ key: normalizedKey, value }, ...previous]);
      addOperation('SET', normalizedKey, 'write', existing ? '本地已更新' : '本地已创建', '—');
      return { kind: 'success', title: existing ? '纯前端写入 · 已更新' : '纯前端写入 · 已创建', message: '仅更新浏览器内存，用于测试 CRUD 界面与动画。' };
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
      addOperation('DEL', normalizedKey, 'delete', '本地删除', '—');
      return { kind: 'success', title: '纯前端删除成功', message: '浏览器内存中的记录已删除；未请求后端或修改 WAL。' };
    }
    addOperation('KEYS', '*', 'system', `${entries.length} 个本地键`, '—');
    return { kind: 'info', title: `共有 ${formatNumber(entries.length)} 个本地键`, message: '右侧存储视图已分页展示，可通过搜索快速定位。' };
  };

  const startConcurrency = async () => {
    if (serverState !== 'ONLINE' || concurrencyStopping) return;
    clearTimer(concurrencyTimerRef);
    const epoch = lifecycleEpochRef.current;
    const runId = ++concurrencyRunRef.current;
    const isActiveRun = () => epoch === lifecycleEpochRef.current && runId === concurrencyRunRef.current;
    setConcurrencyStatus('RUNNING');
    concurrencyRunningRef.current = true;
    setConcurrencyProgress(0);
    setConcurrencySuccessful(0);
    setConcurrencyFailed(0);
    const total = concurrencyClients * requestsPerClient;

    if (backendMode) {
      try {
        const startResponse = await startRemoteConcurrency({ clients: concurrencyClients, requestsPerClient, workload: workloadForApi(concurrencyWorkload) });
        if (!isActiveRun()) return;
        if (!startResponse.accepted) throw new RustKvApiError('NOT_ACCEPTED', '后端未接受并发实验');
        let attempts = 0;
        const poll = async () => {
          if (!isActiveRun()) return;
          try {
            const state = await readRemoteConcurrency();
            if (!isActiveRun()) return;
            attempts += 1;
            setConcurrencyProgress(state.progress);
            setConcurrencySuccessful(state.successful);
            setConcurrencyFailed(state.failed);
            if (state.status === 'COMPLETED') {
              clearTimer(concurrencyTimerRef);
              concurrencyRunningRef.current = false;
              setConcurrencyStatus('COMPLETED');
              const passed = state.progress >= 100 && state.successful === total && state.failed === 0;
              addOperation('LOAD', `${concurrencyClients} clients`, passed ? 'system' : 'error', passed ? 'CONCURRENCY PASS' : 'CONCURRENCY FAIL', '实测');
              return;
            }
            if (state.status === 'INTERRUPTED' || state.status === 'STOPPED') {
              clearTimer(concurrencyTimerRef);
              concurrencyRunningRef.current = false;
              setConcurrencyStatus(state.status);
              return;
            }
            if (attempts >= 1800) {
              clearTimer(concurrencyTimerRef);
              concurrencyRunningRef.current = false;
              setConcurrencyStatus('INTERRUPTED');
              addOperation('LOAD', `${concurrencyClients} clients`, 'error', '实验状态轮询超过 15 分钟 · 未判定', '—');
              return;
            }
            concurrencyTimerRef.current = window.setTimeout(() => void poll(), 500);
          } catch (error) {
            if (!isActiveRun()) return;
            clearTimer(concurrencyTimerRef);
            concurrencyRunningRef.current = false;
            setConcurrencyStatus('INTERRUPTED');
            if (error instanceof RustKvApiError && (error.code === 'BACKEND_UNREACHABLE' || error.code === 'TIMEOUT')) setServerState('OFFLINE');
          }
        };
        void poll();
      } catch (error) {
        if (!isActiveRun()) return;
        concurrencyRunningRef.current = false;
        setConcurrencyStatus('INTERRUPTED');
        if (error instanceof RustKvApiError && (error.code === 'BACKEND_UNREACHABLE' || error.code === 'TIMEOUT')) setServerState('OFFLINE');
      }
      return;
    }

    let step = 0;
    concurrencyTimerRef.current = window.setInterval(() => {
      if (!isActiveRun()) return;
      step += 2.5;
      const nextProgress = Math.min(100, step);
      const completed = Math.round(total * nextProgress / 100);
      setConcurrencyProgress(nextProgress);
      setConcurrencySuccessful(completed);
      if (nextProgress >= 100) {
        clearTimer(concurrencyTimerRef);
        concurrencyRunningRef.current = false;
        setConcurrencyStatus('COMPLETED');
        addOperation('LOAD', `${concurrencyClients} virtual clients`, 'system', 'CONCURRENCY PASS · UI 测试', '非实测');
      }
    }, 95);
  };

  const stopConcurrency = async () => {
    if (concurrencyStopping) return;
    const stopEpoch = lifecycleEpochRef.current;
    clearTimer(concurrencyTimerRef);
    concurrencyRunRef.current += 1;
    concurrencyRunningRef.current = false;
    setConcurrencyStatus('STOPPED');
    setConcurrencyStopping(backendMode);
    addOperation('STOP', `${concurrencyClients} ${backendMode ? 'clients' : 'virtual clients'}`, 'system', '实验已停止 · 未判定', '—');
    if (backendMode) {
      try {
        const response = await stopRemoteConcurrency();
        if (stopEpoch === lifecycleEpochRef.current && !response.stopped) addOperation('STOP', 'concurrency', 'error', '后端未确认停止', '—');
      } catch {
        if (stopEpoch === lifecycleEpochRef.current) addOperation('STOP', 'concurrency', 'error', '后端停止请求失败', '—');
      } finally {
        if (stopEpoch === lifecycleEpochRef.current) setConcurrencyStopping(false);
      }
    }
  };

  const updateBenchmarkSeries = (updater: (current: BenchmarkSeries[]) => BenchmarkSeries[]) => {
    const next = updater(benchmarkSeriesRef.current);
    benchmarkSeriesRef.current = next;
    setBenchmarkSeries(next);
  };

  const clearBenchmarkResultsForConfigChange = () => {
    clearTimer(benchmarkTimerRef);
    benchmarkRunRef.current += 1;
    benchmarkRunningRef.current = false;
    benchmarkQueueRef.current = [];
    benchmarkSeriesRef.current = [];
    benchmarkCurrentJobRef.current = null;
    benchmarkCompletedRef.current = 0;
    benchmarkTotalRef.current = 0;
    setBenchmarkSeries([]);
    setBenchmarkCurrentJob(null);
    setBenchmarkStatus('IDLE');
    setBenchmarkProgress(0);
    setBenchmarkEnvironmentResets(0);
    setBenchmarkStage('实验条件已更新，等待运行');
  };

  const interruptBenchmarkRun = (message: string) => {
    clearTimer(benchmarkTimerRef);
    benchmarkRunRef.current += 1;
    benchmarkRunningRef.current = false;
    const job = benchmarkCurrentJobRef.current;
    if (job) {
      updateBenchmarkSeries((current) => current.map((item) => item.id === job.seriesId ? { ...item, scaleStatus: { ...item.scaleStatus, [job.clients]: 'FAILED' } } : item));
    }
    benchmarkCurrentJobRef.current = null;
    setBenchmarkCurrentJob(null);
    setBenchmarkStatus('INTERRUPTED');
    setBenchmarkStage(message);
  };

  const seedRecoveryData = async () => {
    if (serverState !== 'ONLINE' || recoveryActionPending !== null) return;
    const epoch = lifecycleEpochRef.current;
    const runId = ++recoveryRunRef.current;
    const isActiveRun = () => epoch === lifecycleEpochRef.current && runId === recoveryRunRef.current;
    if (backendMode) {
      setRecoveryActionPending('prepare');
      try {
        const state = await prepareRemoteRecovery(recoverySeedCount);
        if (!isActiveRun()) return;
        if (state.phase !== 'PREPARED' || !state.before) {
          throw new RustKvApiError('INVALID_RESPONSE', '控制器未返回 PREPARED 与 Before 证据');
        }
        applyRemoteRecoveryState(state);
        try {
          const loaded = await refreshBackendEntries();
          if (isActiveRun()) setEntries(loaded);
        } catch {
          if (isActiveRun()) setEntries([]);
        }
        addOperation('SEED', `${recoverySeedCount} keys`, 'write', '控制器已返回 Before 证据', '实测');
      } catch (error) {
        if (!isActiveRun()) return;
        const apiError = error instanceof RustKvApiError ? error : new RustKvApiError('UNKNOWN', '恢复实验准备失败');
        setRecoveryVerified(false);
        setRecoveryPhase('ERROR');
        setRecoveryLogs([`[CLIENT ERROR] ${apiError.code} · ${apiError.message}`]);
        setServerState(apiError.code === 'BACKEND_UNREACHABLE' || apiError.code === 'TIMEOUT' ? 'OFFLINE' : 'ERROR');
        addOperation('SEED', `${recoverySeedCount} keys`, 'error', apiError.code, '—');
      } finally {
        if (isActiveRun()) setRecoveryActionPending(null);
      }
      return;
    }

    const seeded = Array.from({ length: recoverySeedCount }, (_, index) => ({
      key: `crash_test_${String(index + 1).padStart(4, '0')}`,
      value: `durable-value-${String((index * 17 + 11) % 997).padStart(3, '0')}`,
    }));
    const withoutOldSeed = entries.filter((entry) => !entry.key.startsWith('crash_test_'));
    const next = [...seeded, ...withoutOldSeed];
    if (!isActiveRun()) return;
    setEntries(next);
    recoverySnapshotRef.current = next.map((entry) => ({ ...entry }));
    recoveryWalRef.current = next.map((entry) => ({ ...entry }));
    setRecoveryBeforeCount(next.length);
    setBeforeFingerprint(fingerprintFor(next));
    setAfterFingerprint('');
    setRecoverySamples(seeded.slice(0, 3));
    setRecoveryVerificationEntries([]);
    setRecoveredCount(0);
    setRecoveryLost(0);
    setRecoveryVerified(false);
    setRecoveryProgress(0);
    setWalReplayCount(0);
    setRecoveryTime(0);
    setRecoveryPhase('PREPARED');
    setRecoveryLogs([
      `[SEED] 前端写入 ${recoverySeedCount} 个测试键`,
      `[WAL] 独立模拟 WAL 已记录 · ${next.length} keys`,
      `[SNAPSHOT] Before 已冻结，仅用于校验 · ${fingerprintFor(next)}`,
    ]);
    addOperation('SEED', `${recoverySeedCount} keys`, 'write', '本地 WAL + Before Snapshot', '非实测');
  };

  const killServer = async () => {
    if (recoveryPhase !== 'PREPARED' || serverState !== 'ONLINE' || recoveryActionPending !== null) return;
    const concurrencyWasRunning = concurrencyRunningRef.current;
    const benchmarkWasRunning = benchmarkRunningRef.current;
    lifecycleEpochRef.current += 1;
    const epoch = lifecycleEpochRef.current;
    concurrencyRunRef.current += 1;
    concurrencyRunningRef.current = false;
    const runId = ++recoveryRunRef.current;
    const isActiveRun = () => epoch === lifecycleEpochRef.current && runId === recoveryRunRef.current;
    setConcurrencyStopping(false);
    setBenchmarkStopping(false);
    if (backendMode) setBackendProbeEpoch((value) => value + 1);
    suspendHealthProbeRef.current = true;
    if (concurrencyWasRunning) {
      clearTimer(concurrencyTimerRef);
      setConcurrencyStatus('INTERRUPTED');
      addOperation('STOP', `${concurrencyClients} clients`, 'error', 'Crash Recovery 接管 · 未判定', '—');
    }
    if (benchmarkWasRunning) interruptBenchmarkRun('当前 Scale 因 Crash Recovery 中断；已完成数据保留');
    if (backendMode) {
      setRecoveryActionPending('kill');
      try {
        const state = await killRemoteRecovery();
        if (!isActiveRun()) return;
        if (state.phase !== 'CRASHED') {
          throw new RustKvApiError('INVALID_RESPONSE', `控制器返回 ${state.phase}，未确认进程已终止`);
        }
        applyRemoteRecoveryState(state);
        addOperation('KILL', 'managed server process', 'error', '控制器确认后端已终止', '实测');
      } catch (error) {
        if (!isActiveRun()) return;
        const apiError = error instanceof RustKvApiError ? error : new RustKvApiError('UNKNOWN', '强制终止失败');
        suspendHealthProbeRef.current = false;
        setRecoveryVerified(false);
        setRecoveryPhase('ERROR');
        appendRecoveryLog(`[CLIENT ERROR] ${apiError.code} · ${apiError.message}`);
        setServerState(apiError.code === 'BACKEND_UNREACHABLE' || apiError.code === 'TIMEOUT' ? 'OFFLINE' : 'ERROR');
        addOperation('KILL', 'managed server process', 'error', apiError.code, '—');
      } finally {
        if (isActiveRun()) setRecoveryActionPending(null);
      }
      return;
    }
    if (!isActiveRun()) return;
    setEntries([]);
    setServerState('OFFLINE');
    setRecoveryPhase('CRASHED');
    setRecoveredCount(0);
    setRecoveryLogs((previous) => [...previous, `[KILL] ${backendMode ? '后端进程已强制终止' : '前端服务动画切换为离线'}`, '[MEMORY] Memory Store 消失 · 0 Keys', '[WAL] WAL 保留，等待 Restart 后重放', '[SNAPSHOT] Frontend Snapshot 保留，但禁止作为恢复来源']);
    setLastUpdate(formatClock().slice(0, 8));
    addOperation('KILL', backendMode ? 'managed server process' : 'frontend memory state', 'error', backendMode ? '后端已终止' : '纯前端动画', '—');
  };

  const finalizeSimulatedRecovery = (memoryEntries: KvEntry[], replayCount: number, elapsedSeconds: number) => {
    const before = recoverySnapshotRef.current;
    const verificationEntries = memoryEntries;
    const verificationMap = new Map(verificationEntries.map((entry) => [entry.key, entry.value]));
    const lost = before.filter((entry) => verificationMap.get(entry.key) !== entry.value).length;
    const afterHash = fingerprintFor(verificationEntries);
    setEntries(memoryEntries);
    setRecoveryVerificationEntries(verificationEntries);
    setRecoveredCount(memoryEntries.length);
    setRecoveryLost(lost);
    setRecoveryVerified(lost === 0 && fingerprintFor(before) === afterHash);
    setAfterFingerprint(afterHash);
    setWalReplayCount(replayCount);
    setRecoveryTime(elapsedSeconds);
    setRecoveryProgress(100);
    suspendHealthProbeRef.current = false;
    setServerState('ONLINE');
    setRecoveryPhase('VERIFIED');
    setRecoveryLogs((previous) => [...previous, `[VERIFY] WAL 重建 Memory ${memoryEntries.length} Keys`, `[HASH] Before ${fingerprintFor(before)} · After ${afterHash}`, lost ? `[FAIL] 验证层检测到 ${lost} 个键不一致` : '[PASS] 数量、抽样值与 Hash 全部一致']);
    setLastUpdate('刚刚');
    addOperation('RESTART', 'simulated WAL replay', lost ? 'error' : 'system', lost ? 'CONSISTENCY FAIL' : 'CONSISTENCY PASS · 非实测', `${elapsedSeconds.toFixed(2)}s`);
  };

  const restartServer = async () => {
    if (recoveryPhase !== 'CRASHED' || recoveryActionPending !== null) return;
    clearTimer(recoveryTimerRef);
    const epoch = lifecycleEpochRef.current;
    const runId = ++recoveryRunRef.current;
    const isActiveRun = () => epoch === lifecycleEpochRef.current && runId === recoveryRunRef.current;
    const startedAt = performance.now();

    if (backendMode) {
      setRecoveryActionPending('restart');
      try {
        const initialState = await restartRemoteRecovery();
        if (!isActiveRun()) return;
        if (initialState.phase !== 'RECOVERING' && initialState.phase !== 'VERIFIED') {
          throw new RustKvApiError('INVALID_RESPONSE', `控制器返回 ${initialState.phase}，未确认恢复已开始`);
        }
        applyRemoteRecoveryState(initialState);
      } catch (error) {
        if (!isActiveRun()) return;
        const apiError = error instanceof RustKvApiError ? error : new RustKvApiError('UNKNOWN', '重启恢复失败');
        suspendHealthProbeRef.current = false;
        setRecoveryVerified(false);
        setRecoveryPhase('ERROR');
        appendRecoveryLog(`[CLIENT ERROR] ${apiError.code} · ${apiError.message}`);
        setServerState(apiError.code === 'BACKEND_UNREACHABLE' || apiError.code === 'TIMEOUT' ? 'OFFLINE' : 'ERROR');
        addOperation('RESTART', 'backend WAL replay', 'error', apiError.code, '—');
        return;
      } finally {
        if (isActiveRun()) setRecoveryActionPending(null);
      }
      if (!isActiveRun()) return;
      let attempts = 0;
      const poll = async () => {
        if (!isActiveRun()) return;
        attempts += 1;
        try {
          const remoteState = await readRemoteRecovery();
          if (!isActiveRun()) return;
          applyRemoteRecoveryState(remoteState);
          if (remoteState.phase === 'VERIFIED') {
            clearTimer(recoveryTimerRef);
            suspendHealthProbeRef.current = false;
            setBackendProbeEpoch((value) => value + 1);
            try {
              const restored = await refreshBackendEntries();
              if (isActiveRun()) setEntries(restored);
            } catch {
              if (isActiveRun()) setEntries([]);
            }
            if (!isActiveRun()) return;
            addOperation('RESTART', 'backend WAL replay', remoteState.verified ? 'system' : 'error', remoteState.verified ? 'CONSISTENCY PASS' : 'CONSISTENCY FAIL', `${(remoteState.recoveryTimeMs / 1000).toFixed(2)}s`);
            return;
          }
          if (remoteState.phase === 'ERROR') {
            clearTimer(recoveryTimerRef);
            suspendHealthProbeRef.current = false;
            setServerState('ERROR');
            addOperation('RESTART', 'backend WAL replay', 'error', 'RECOVERY ERROR', '—');
            return;
          }
          if (attempts >= 1800) {
            clearTimer(recoveryTimerRef);
            suspendHealthProbeRef.current = false;
            setRecoveryVerified(false);
            setRecoveryPhase('ERROR');
            setServerState('ERROR');
            appendRecoveryLog('[CLIENT ERROR] 恢复状态轮询超过 15 分钟，实验中断');
            return;
          }
          recoveryTimerRef.current = window.setTimeout(() => void poll(), 500);
        } catch (error) {
          if (!isActiveRun()) return;
          clearTimer(recoveryTimerRef);
          suspendHealthProbeRef.current = false;
          const apiError = error instanceof RustKvApiError ? error : new RustKvApiError('UNKNOWN', '恢复状态查询失败');
          setRecoveryVerified(false);
          setRecoveryPhase('ERROR');
          setServerState(apiError.code === 'BACKEND_UNREACHABLE' || apiError.code === 'TIMEOUT' ? 'OFFLINE' : 'ERROR');
          appendRecoveryLog(`[CLIENT ERROR] ${apiError.code} · ${apiError.message}`);
        }
      };
      void poll();
      return;
    }

    setServerState('RECOVERING');
    setRecoveryPhase('RECOVERING');
    setRecoveryVerified(false);
    setRecoveryProgress(0);
    setWalReplayCount(0);
    setRecoveryLogs((previous) => [...previous, '[BOOT] 纯前端启动恢复动画', '[SOURCE] 恢复来源 = 模拟 WAL；Frontend Snapshot 未参与', '[REPLAY] 开始模拟顺序重放']);
    const wal = recoveryWalRef.current.map((entry) => ({ ...entry }));
    let step = 0;
    const announced = new Set<number>();
    recoveryTimerRef.current = window.setInterval(() => {
      if (!isActiveRun()) return;
      step += 4;
      const nextProgress = Math.min(100, step);
      const nextCount = Math.round(wal.length * nextProgress / 100);
      setRecoveryProgress(nextProgress);
      setRecoveredCount(nextCount);
      setWalReplayCount(nextCount);
      [25, 50, 75].forEach((threshold) => {
        if (nextProgress >= threshold && !announced.has(threshold)) {
          announced.add(threshold);
          setRecoveryLogs((previous) => [...previous, `[REPLAY] ${threshold}% · 已从 WAL 重建 ${Math.round(wal.length * threshold / 100)} 个键`]);
        }
      });
      if (nextProgress >= 100) {
        clearTimer(recoveryTimerRef);
        finalizeSimulatedRecovery(wal, wal.length, (performance.now() - startedAt) / 1000);
      }
    }, 90);
  };

  const failBenchmarkJob = (job: BenchmarkJob, message: string, runId: number, epoch: number) => {
    if (runId !== benchmarkRunRef.current || epoch !== lifecycleEpochRef.current) return;
    clearTimer(benchmarkTimerRef);
    updateBenchmarkSeries((current) => current.map((item) => item.id === job.seriesId ? { ...item, scaleStatus: { ...item.scaleStatus, [job.clients]: 'FAILED' } } : item));
    benchmarkCurrentJobRef.current = null;
    setBenchmarkCurrentJob(null);
    setBenchmarkStatus('INTERRUPTED');
    benchmarkRunningRef.current = false;
    setBenchmarkStage(message);
    addOperation('BENCH', `${job.seriesId}:${job.clients}`, 'error', 'Scale FAILED · 可重试', '—');
    benchmarkRunRef.current += 1;
  };

  const resetBenchmarkEnvironmentBefore = (job: BenchmarkJob, runId: number, epoch: number, countReset: boolean, readyMessage: string) => {
    void (async () => {
      try {
        const response = await resetRemoteBenchmarkEnvironment();
        if (runId !== benchmarkRunRef.current || epoch !== lifecycleEpochRef.current) return;
        if (!response.reset) {
          failBenchmarkJob(job, '后端未确认实验环境复位；后续 Scale 未运行', runId, epoch);
          return;
        }
        if (countReset) setBenchmarkEnvironmentResets((value) => value + 1);
        setBenchmarkStage(readyMessage);
        benchmarkTimerRef.current = window.setTimeout(() => runNextBenchmarkJob(runId, epoch), 120);
      } catch (error) {
        failBenchmarkJob(job, `实验环境复位失败 · ${error instanceof Error ? error.message : '未知错误'}`, runId, epoch);
      }
    })();
  };

  const completeBenchmarkJob = (job: BenchmarkJob, point: BenchmarkPoint, runId: number, epoch: number) => {
    if (runId !== benchmarkRunRef.current || epoch !== lifecycleEpochRef.current) return;
    if (!isValidBenchmarkPoint(point)) {
      failBenchmarkJob(job, `${job.clients} Clients 返回无效数据；已完成点保留`, runId, epoch);
      return;
    }
    updateBenchmarkSeries((current) => current.map((item) => {
      if (item.id !== job.seriesId) return item;
      const points = [...item.points.filter((candidate) => candidate.clients !== job.clients), point].sort((a, b) => a.clients - b.clients);
      return { ...item, points, scaleStatus: { ...item.scaleStatus, [job.clients]: 'DONE' } };
    }));
    benchmarkCompletedRef.current += 1;
    setBenchmarkProgress(benchmarkTotalRef.current ? benchmarkCompletedRef.current / benchmarkTotalRef.current * 100 : 100);
    benchmarkCurrentJobRef.current = null;
    setBenchmarkCurrentJob(null);

    const next = benchmarkQueueRef.current[0];
    if (!next) {
      benchmarkRunningRef.current = false;
      setBenchmarkStatus('COMPLETED');
      setBenchmarkProgress(100);
      setBenchmarkStage('全部 Scale 完成 · 已生成对照结论');
      addOperation('BENCH', `${benchmarkTotalRef.current} steps`, 'system', backendMode ? '后端实测完成' : '本地执行器完成 · 非实测', '—');
      return;
    }
    if (next.seriesId !== job.seriesId) {
      setBenchmarkStage(backendMode ? '请求后端恢复相同数据集与持久化条件' : '恢复相同数据集与持久化条件，准备下一组');
      if (!backendMode) {
        setBenchmarkEnvironmentResets((value) => value + 1);
        benchmarkTimerRef.current = window.setTimeout(() => runNextBenchmarkJob(runId, epoch), 420);
        return;
      }
      resetBenchmarkEnvironmentBefore(next, runId, epoch, true, '后端环境已复位，准备运行下一组');
    } else {
      benchmarkTimerRef.current = window.setTimeout(() => runNextBenchmarkJob(runId, epoch), 120);
    }
  };

  function runNextBenchmarkJob(runId: number, epoch: number) {
    if (runId !== benchmarkRunRef.current || epoch !== lifecycleEpochRef.current) return;
    const job = benchmarkQueueRef.current.shift();
    if (!job) {
      benchmarkRunningRef.current = false;
      setBenchmarkStatus('COMPLETED');
      setBenchmarkProgress(100);
      setBenchmarkStage('全部 Scale 完成');
      return;
    }
    const targetSeries = benchmarkSeriesRef.current.find((item) => item.id === job.seriesId);
    if (!targetSeries) {
      failBenchmarkJob(job, '实验配置丢失，无法继续', runId, epoch);
      return;
    }
    benchmarkCurrentJobRef.current = job;
    setBenchmarkCurrentJob(job);
    updateBenchmarkSeries((current) => current.map((item) => item.id === job.seriesId ? { ...item, scaleStatus: { ...item.scaleStatus, [job.clients]: 'RUNNING' } } : item));
    setBenchmarkStage(`运行 ${targetSeries.label} · ${job.clients} Clients · ${formatNumber(targetSeries.config.requests)} Requests`);

    if (!backendMode) {
      benchmarkTimerRef.current = window.setTimeout(() => completeBenchmarkJob(job, makeBenchmarkPoint(targetSeries.config, job.clients), runId, epoch), 620);
      return;
    }

    void (async () => {
      try {
        const startResponse = await startRemoteBenchmark({
          clients: job.clients,
          requests: targetSeries.config.requests,
          runtime: targetSeries.config.runtime.toLowerCase() as 'sync' | 'async',
          lock: targetSeries.config.lock.toLowerCase() as 'mutex' | 'rwlock',
          workload: workloadForApi(targetSeries.config.workload),
        });
        if (runId !== benchmarkRunRef.current || epoch !== lifecycleEpochRef.current) return;
        if (!startResponse.accepted) {
          failBenchmarkJob(job, `${job.clients} Clients 未被后端接受`, runId, epoch);
          return;
        }
        let attempts = 0;
        const poll = async () => {
          if (runId !== benchmarkRunRef.current || epoch !== lifecycleEpochRef.current) return;
          try {
            const state = await readRemoteBenchmark();
            if (runId !== benchmarkRunRef.current || epoch !== lifecycleEpochRef.current) return;
            attempts += 1;
            const remotePoint = state.points.find((point) => point.clients === job.clients);
            if (state.status === 'COMPLETED' && remotePoint) {
              completeBenchmarkJob(job, remotePoint, runId, epoch);
              return;
            }
            if (state.status === 'COMPLETED' && !remotePoint) {
              failBenchmarkJob(job, `${job.clients} Clients 完成但缺少结果点`, runId, epoch);
              return;
            }
            if (state.status === 'INTERRUPTED' || state.status === 'STOPPED' || state.error) {
              failBenchmarkJob(job, state.error ?? `${job.clients} Clients 测试被中断`, runId, epoch);
              return;
            }
            if (attempts >= 1800) {
              failBenchmarkJob(job, `${job.clients} Clients 状态轮询超过 15 分钟`, runId, epoch);
              return;
            }
            benchmarkTimerRef.current = window.setTimeout(() => void poll(), 500);
          } catch (error) {
            failBenchmarkJob(job, `${job.clients} Clients 请求失败 · ${error instanceof Error ? error.message : '未知错误'}`, runId, epoch);
          }
        };
        void poll();
      } catch (error) {
        failBenchmarkJob(job, `${job.clients} Clients 无法启动 · ${error instanceof Error ? error.message : '未知错误'}`, runId, epoch);
      }
    })();
  }

  const startBenchmark = () => {
    if (serverState !== 'ONLINE' || !benchmarkScales.length || benchmarkStopping) return;
    clearTimer(benchmarkTimerRef);
    const epoch = lifecycleEpochRef.current;
    const runId = ++benchmarkRunRef.current;
    const config: BenchmarkConfig = { runtime: benchmarkRuntime, lock: benchmarkLock, workload: benchmarkWorkload, requests: BENCHMARK_REQUESTS };
    const nextSeries = buildBenchmarkSeries(benchmarkMode, benchmarkResearchVariable, config, benchmarkScales);
    const jobs = nextSeries.flatMap((item) => benchmarkScales.map((clients) => ({ seriesId: item.id, clients })));
    benchmarkSeriesRef.current = nextSeries;
    benchmarkQueueRef.current = jobs;
    benchmarkCompletedRef.current = 0;
    benchmarkTotalRef.current = jobs.length;
    benchmarkCurrentJobRef.current = null;
    setBenchmarkSeries(nextSeries);
    setBenchmarkCurrentJob(null);
    benchmarkRunningRef.current = true;
    setBenchmarkStatus('RUNNING');
    setBenchmarkProgress(0);
    setBenchmarkEnvironmentResets(0);
    setBenchmarkStage(backendMode ? '请求后端准备统一数据集与固定持久化条件' : '准备统一数据集与固定持久化条件');
    if (backendMode) {
      resetBenchmarkEnvironmentBefore(jobs[0], runId, epoch, false, '后端初始环境已准备，开始第一组');
    } else {
      benchmarkTimerRef.current = window.setTimeout(() => runNextBenchmarkJob(runId, epoch), 260);
    }
  };

  const stopBenchmark = async () => {
    if (benchmarkStopping) return;
    const stopEpoch = lifecycleEpochRef.current;
    clearTimer(benchmarkTimerRef);
    benchmarkRunRef.current += 1;
    benchmarkRunningRef.current = false;
    const job = benchmarkCurrentJobRef.current;
    if (job) updateBenchmarkSeries((current) => current.map((item) => item.id === job.seriesId ? { ...item, scaleStatus: { ...item.scaleStatus, [job.clients]: 'WAITING' } } : item));
    benchmarkCurrentJobRef.current = null;
    setBenchmarkCurrentJob(null);
    setBenchmarkStatus('STOPPED');
    setBenchmarkStage('实验已停止；已完成数据点保留');
    setBenchmarkStopping(backendMode);
    addOperation('STOP', 'performance lab', 'system', '已停止并保留完成点', '—');
    if (backendMode) {
      try {
        const response = await stopRemoteBenchmark();
        if (stopEpoch === lifecycleEpochRef.current && !response.stopped) {
          setBenchmarkStage('实验数据已保留，但后端未确认停止');
          addOperation('STOP', 'performance lab', 'error', '后端未确认停止', '—');
        }
      } catch {
        if (stopEpoch === lifecycleEpochRef.current) {
          setBenchmarkStage('实验数据已保留，但后端停止请求失败');
          addOperation('STOP', 'performance lab', 'error', '后端停止请求失败', '—');
        }
      } finally {
        if (stopEpoch === lifecycleEpochRef.current) setBenchmarkStopping(false);
      }
    }
  };

  const retryFailedBenchmark = () => {
    if (serverState !== 'ONLINE' || benchmarkStopping) return;
    clearTimer(benchmarkTimerRef);
    const epoch = lifecycleEpochRef.current;
    const runId = ++benchmarkRunRef.current;
    updateBenchmarkSeries((current) => current.map((item) => ({
      ...item,
      scaleStatus: Object.fromEntries(Object.entries(item.scaleStatus).map(([scale, value]) => [Number(scale), value === 'FAILED' ? 'WAITING' : value])) as Record<number, ScaleStatus>,
    })));
    const retryJobs: BenchmarkJob[] = [];
    for (const item of benchmarkSeriesRef.current) {
      const orderedScales = Object.entries(item.scaleStatus)
        .filter(([, value]) => value === 'WAITING' || value === 'FAILED')
        .map(([clients]) => Number(clients))
        .sort((a, b) => a - b);
      for (const clients of orderedScales) retryJobs.push({ seriesId: item.id, clients });
    }
    benchmarkQueueRef.current = retryJobs;
    benchmarkCompletedRef.current = benchmarkSeriesRef.current.reduce((count, item) => count + Object.values(item.scaleStatus).filter((value) => value === 'DONE').length, 0);
    benchmarkTotalRef.current = benchmarkSeriesRef.current.reduce((count, item) => count + Object.keys(item.scaleStatus).length, 0);
    benchmarkRunningRef.current = true;
    setBenchmarkStatus('RUNNING');
    setBenchmarkStage(backendMode ? '重试前请求后端恢复统一环境' : '重试前恢复统一环境');
    if (!retryJobs.length) {
      benchmarkRunningRef.current = false;
      setBenchmarkStatus('COMPLETED');
      setBenchmarkProgress(100);
      setBenchmarkStage('没有待重试 Scale');
    } else if (backendMode) {
      resetBenchmarkEnvironmentBefore(retryJobs[0], runId, epoch, true, '后端环境已复位，继续未完成 Scale');
    } else {
      setBenchmarkEnvironmentResets((value) => value + 1);
      benchmarkTimerRef.current = window.setTimeout(() => runNextBenchmarkJob(runId, epoch), 260);
    }
  };

  const applyBenchmarkPreset = (value: Exclude<BenchmarkPreset, null>) => {
    clearBenchmarkResultsForConfigChange();
    setBenchmarkPreset(value);
    setBenchmarkMode('COMPARE');
    setBenchmarkScales([...BENCHMARK_SCALES]);
    if (value === 'A') {
      setBenchmarkResearchVariable('RUNTIME');
      setBenchmarkRuntime('Sync');
      setBenchmarkLock('Mutex');
      setBenchmarkWorkload('MIXED');
    } else if (value === 'B') {
      setBenchmarkResearchVariable('LOCK');
      setBenchmarkRuntime('Async');
      setBenchmarkLock('Mutex');
      setBenchmarkWorkload('READ_HEAVY');
    } else {
      setBenchmarkResearchVariable('WORKLOAD');
      setBenchmarkRuntime('Async');
      setBenchmarkLock('RwLock');
      setBenchmarkWorkload('READ_HEAVY');
    }
  };

  const resetLab = (stopRemoteRuns = true) => {
    const stopConcurrencyOnBackend = stopRemoteRuns && backendMode && (concurrencyStatus === 'RUNNING' || concurrencyStopping);
    const stopBenchmarkOnBackend = stopRemoteRuns && backendMode && (benchmarkStatus === 'RUNNING' || benchmarkStopping);
    lifecycleEpochRef.current += 1;
    const resetEpoch = lifecycleEpochRef.current;
    concurrencyRunRef.current += 1;
    concurrencyRunningRef.current = false;
    recoveryRunRef.current += 1;
    benchmarkRunRef.current += 1;
    benchmarkRunningRef.current = false;
    suspendHealthProbeRef.current = false;
    if (backendMode) setBackendProbeEpoch((value) => value + 1);
    clearTimer(concurrencyTimerRef);
    clearTimer(recoveryTimerRef);
    clearTimer(benchmarkTimerRef);
    setConcurrencyStopping(stopConcurrencyOnBackend);
    setBenchmarkStopping(stopBenchmarkOnBackend);
    if (stopConcurrencyOnBackend) {
      void stopRemoteConcurrency()
        .catch(() => undefined)
        .finally(() => {
          if (resetEpoch === lifecycleEpochRef.current) setConcurrencyStopping(false);
        });
    }
    if (stopBenchmarkOnBackend) {
      void stopRemoteBenchmark()
        .catch(() => undefined)
        .finally(() => {
          if (resetEpoch === lifecycleEpochRef.current) setBenchmarkStopping(false);
        });
    }
    setActiveTab('overview');
    if (!backendMode) {
      setServerState('ONLINE');
      setEntries(makeInitialEntries());
    }
    setLastUpdate('刚刚');
    setOperations(initialOperations);
    setConcurrencyClients(100);
    setRequestsPerClient(100);
    setConcurrencyWorkload('MIXED');
    setConcurrencyStatus('IDLE');
    setConcurrencyProgress(0);
    setConcurrencySuccessful(0);
    setConcurrencyFailed(0);
    setRecoveryPhase('IDLE');
    setRecoverySeedCount(100);
    setRecoveryActionPending(null);
    setRecoveryVerified(false);
    setRecoveryBeforeCount(0);
    setRecoveredCount(0);
    setRecoveryLost(0);
    setRecoveryProgress(0);
    setRecoveryLogs([]);
    setBeforeFingerprint('');
    setAfterFingerprint('');
    setRecoverySamples([]);
    setRecoveryVerificationEntries([]);
    setWalReplayCount(backendMode ? null : 0);
    setRecoveryTime(0);
    setBenchmarkMode('COMPARE');
    setBenchmarkResearchVariable('LOCK');
    setBenchmarkRuntime('Async');
    setBenchmarkLock('Mutex');
    setBenchmarkScales([...BENCHMARK_SCALES]);
    setBenchmarkWorkload('READ_HEAVY');
    setBenchmarkPreset('B');
    setBenchmarkStatus('IDLE');
    setBenchmarkProgress(0);
    setBenchmarkSeries([]);
    setBenchmarkCurrentJob(null);
    setBenchmarkStage('等待运行实验');
    setBenchmarkEnvironmentResets(0);
    recoverySnapshotRef.current = [];
    recoveryWalRef.current = [];
    benchmarkQueueRef.current = [];
    benchmarkSeriesRef.current = [];
    benchmarkCurrentJobRef.current = null;
    benchmarkCompletedRef.current = 0;
    benchmarkTotalRef.current = 0;
    logIdRef.current = 0;
    setResetOpen(false);
  };

  const switchExecutionMode = () => {
    const nextBackendMode = !backendMode;
    resetLab(false);
    setBackendMode(nextBackendMode);
    setServerState(nextBackendMode ? 'STARTING' : 'ONLINE');
    setEntries(nextBackendMode ? [] : makeInitialEntries());
    setLastUpdate(nextBackendMode ? formatClock().slice(0, 8) : '刚刚');
  };

  const displayedKeyCount = backendMode && recoveryPhase !== 'IDLE' && recoveryPhase !== 'ERROR'
    ? recoveryPhase === 'PREPARED'
      ? recoveryBeforeCount
      : recoveryPhase === 'CRASHED'
        ? 0
        : recoveredCount
    : serverState === 'RECOVERING'
      ? recoveredCount
      : entries.length;
  const metricStrip: LabMetricItem[] = [
    { label: backendMode ? '后端键总数' : '本地键总数', value: formatNumber(displayedKeyCount), suffix: backendMode ? '来自接入层' : '纯前端内存', accent: 'cyan' },
  ];
  const activeNavigation = navigation.find((item) => item.id === activeTab)!;

  return (
    <main className={`lab-shell ${backendMode ? 'debate-mode' : 'frontend-mode'} server-${serverState.toLowerCase()}`}>
      <header className="topbar">
        <div className="brand-block"><div className="brand-mark"><Database size={20} /></div><div><strong>RustKV <span>实验室</span></strong><small>{backendMode ? '答辩模式 · 接入后端实测' : '纯前端模式 · UI 与动画测试'}</small></div></div>
        <div className={`connection-pill state-${serverState.toLowerCase()} ${backendMode ? 'backend' : 'frontend'}`}><LabStatusOrb offline={serverState !== 'ONLINE'} /><b>{backendMode ? serverLabels[serverState].label : serverState === 'ONLINE' ? 'UI 测试就绪' : serverLabels[serverState].label}</b><code>{backendMode ? 'BACKEND LAB · 答辩实测' : 'FRONTEND ONLY · 非实测'}</code></div>
        <div className="top-actions">
        <LabButton variant="ghost" className="shell-button" onClick={switchExecutionMode} aria-pressed={backendMode}><Presentation /> {backendMode ? '退出答辩模式' : '进入答辩模式'}</LabButton>
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

      <LabMetricStrip metrics={metricStrip} aria-label={backendMode ? '后端数据概览' : '本地数据概览'} />

      <div className="content-area">
        <div className="page-title-row"><div><p><Activity size={14} /> RUSTKV SYSTEMS LAB / {activeNavigation.hint.toUpperCase()}</p><h1>{activeNavigation.label}</h1></div><div className={`last-update ${serverState !== 'ONLINE' ? 'frozen' : ''}`}><LabStatusOrb offline={serverState !== 'ONLINE'} /> {serverState === 'ONLINE' ? (backendMode ? '后端状态 · 刚刚' : '纯前端状态 · 刚刚') : `${serverLabels[serverState].label} · 最后更新 ${lastUpdate}`}</div></div>

        {activeTab === 'overview' && <Overview backendMode={backendMode} serverState={serverState} operations={operations} lastUpdate={lastUpdate} concurrencyStatus={concurrencyStatus} concurrencyFailed={concurrencyFailed} recoveryPhase={recoveryPhase} recoveryLost={recoveryLost} benchmarkStatus={benchmarkStatus} benchmarkSeries={benchmarkSeries} />}
        {activeTab === 'kv' && <KvOperations backendMode={backendMode} online={serverState === 'ONLINE'} entries={entries} operations={operations} onAction={performKvAction} />}
        {activeTab === 'concurrency' && <ConcurrencyPage backendMode={backendMode} online={serverState === 'ONLINE'} clients={concurrencyClients} setClients={setConcurrencyClients} requestsPerClient={requestsPerClient} setRequestsPerClient={setRequestsPerClient} workload={concurrencyWorkload} setWorkload={setConcurrencyWorkload} status={concurrencyStatus} progress={concurrencyProgress} successful={concurrencySuccessful} failed={concurrencyFailed} stopping={concurrencyStopping} onStart={startConcurrency} onStop={stopConcurrency} />}
        {activeTab === 'recovery' && <RecoveryPage backendMode={backendMode} serverState={serverState} phase={recoveryPhase} seedCount={recoverySeedCount} setSeedCount={setRecoverySeedCount} actionPending={recoveryActionPending} verifiedBySource={recoveryVerified} beforeCount={recoveryBeforeCount} recoveredCount={recoveredCount} recoveryLost={recoveryLost} progress={recoveryProgress} logs={recoveryLogs} beforeFingerprint={beforeFingerprint} afterFingerprint={afterFingerprint} sampleBefore={recoverySamples} currentEntries={entries} verificationEntries={recoveryVerificationEntries} walReplayCount={walReplayCount} recoveryTime={recoveryTime} onSeed={seedRecoveryData} onKill={killServer} onRestart={restartServer} />}
        {activeTab === 'performance' && <PerformancePage backendMode={backendMode} online={serverState === 'ONLINE'} mode={benchmarkMode} setMode={(value) => { clearBenchmarkResultsForConfigChange(); setBenchmarkMode(value); setBenchmarkPreset(null); }} researchVariable={benchmarkResearchVariable} setResearchVariable={(value) => { clearBenchmarkResultsForConfigChange(); setBenchmarkResearchVariable(value); setBenchmarkPreset(null); }} runtime={benchmarkRuntime} setRuntime={(value) => { clearBenchmarkResultsForConfigChange(); setBenchmarkRuntime(value); setBenchmarkPreset(null); }} lock={benchmarkLock} setLock={(value) => { clearBenchmarkResultsForConfigChange(); setBenchmarkLock(value); setBenchmarkPreset(null); }} scales={benchmarkScales} setScales={(value) => { clearBenchmarkResultsForConfigChange(); setBenchmarkScales(value); setBenchmarkPreset(null); }} workload={benchmarkWorkload} setWorkload={(value) => { clearBenchmarkResultsForConfigChange(); setBenchmarkWorkload(value); setBenchmarkPreset(null); }} preset={benchmarkPreset} onApplyPreset={applyBenchmarkPreset} status={benchmarkStatus} progress={benchmarkProgress} series={benchmarkSeries} currentJob={benchmarkCurrentJob} stage={benchmarkStage} environmentResets={benchmarkEnvironmentResets} stopping={benchmarkStopping} onStart={startBenchmark} onStop={stopBenchmark} onRetry={retryFailedBenchmark} />}
      </div>

      <AlertDialog open={resetOpen} onOpenChange={setResetOpen}>
        <AlertDialogContent className="reset-dialog">
          <AlertDialogHeader><AlertDialogTitle>重置当前实验室界面？</AlertDialogTitle><AlertDialogDescription>{backendMode ? '只清空前端实验进度与图表，不删除后端数据、不修改 WAL，也不会切换出答辩模式。' : '清空纯前端实验进度，恢复初始键值与 UI 状态；不会发送任何后端请求。'}</AlertDialogDescription></AlertDialogHeader>
          <AlertDialogFooter><AlertDialogCancel>取消</AlertDialogCancel><AlertDialogAction onClick={() => resetLab()}><RotateCcw /> 确认重置</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </main>
  );
}
