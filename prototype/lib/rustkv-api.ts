export type KvCommand =
  | { cmd: 'set'; key: string; value: string }
  | { cmd: 'get'; key: string }
  | { cmd: 'delete'; key: string }
  | { cmd: 'keys' }
  | { cmd: 'status' }
  | { cmd: 'ping' };

export type KvResponseData =
  | { kind: 'set'; replaced: boolean }
  | { kind: 'get'; value: string }
  | { kind: 'delete'; deleted: boolean }
  | { kind: 'keys'; keys: string[]; count: number }
  | { kind: 'status'; count: number }
  | { kind: 'ping' };

export type KvResponse =
  | { ok: true; data: KvResponseData }
  | { ok: false; error: { code: string; message: string } };

export type RemoteExperimentState = {
  status: 'IDLE' | 'RUNNING' | 'COMPLETED' | 'STOPPED' | 'INTERRUPTED';
  progress: number;
  successful: number;
  failed: number;
};

export type RemoteBenchmarkPoint = {
  clients: number;
  qps: number;
  p50: number;
  p95: number;
  p99: number;
  success: number;
  failed: number;
  elapsedMs: number;
};

export type RemoteBenchmarkState = {
  status: 'IDLE' | 'RUNNING' | 'COMPLETED' | 'STOPPED' | 'INTERRUPTED';
  progress: number;
  points: RemoteBenchmarkPoint[];
  error?: string;
  mode?: 'quick' | 'full';
  sampling?: 'fixed_duration' | 'fixed_requests';
  sampleDurationMs?: number;
  requestsPerScale?: number;
  resetEpoch: number;
  startedAtUnixMs?: number;
  completedAtUnixMs?: number;
};

export type RemoteRecoveryEvidence = {
  count: number;
  fingerprint: string;
  samples: Array<{ key: string; value: string }>;
};

export type RemoteRecoveryState = {
  phase: 'IDLE' | 'PREPARED' | 'CRASHED' | 'RECOVERING' | 'VERIFIED' | 'ERROR';
  progress: number;
  before: RemoteRecoveryEvidence | null;
  after: RemoteRecoveryEvidence | null;
  lost: number;
  verified: boolean;
  walReplayCount: number;
  recoveryTimeMs: number;
  logs: string[];
};

export type RemoteStorageState = {
  engine: string;
  entries: number;
  walRecords: number;
  walBytes: number;
  snapshotBytes: number;
  totalBytes: number;
  lastSequence: number;
  writable: boolean;
};

export type RemoteStorageCompactResult = {
  compacted: boolean;
  compactMs: number;
  before: RemoteStorageStats;
  after: RemoteStorageStats;
};

export type RemoteStorageStats = Omit<RemoteStorageState, 'engine' | 'writable'>;

export class RustKvApiError extends Error {
  code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = 'RustKvApiError';
    this.code = code;
  }
}

async function requestJson<T>(path: string, init?: RequestInit, timeoutMs = 3_000): Promise<T> {
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), timeoutMs);
  const headers = new Headers(init?.headers);
  if (init?.body) headers.set('Content-Type', 'application/json');
  try {
    const response = await fetch(path, {
      ...init,
      headers,
      signal: controller.signal,
    });
    const payload = await response.json() as T & { error?: { code?: string; message?: string } };
    if (!response.ok) {
      throw new RustKvApiError(
        payload.error?.code ?? 'BACKEND_UNREACHABLE',
        payload.error?.message ?? `HTTP ${response.status}`,
      );
    }
    return payload;
  } catch (error) {
    if (error instanceof RustKvApiError) throw error;
    if (error instanceof DOMException && error.name === 'AbortError') {
      throw new RustKvApiError('TIMEOUT', `请求超过 ${Math.round(timeoutMs / 1000)} 秒未完成`);
    }
    throw new RustKvApiError('BACKEND_UNREACHABLE', error instanceof Error ? error.message : '后端不可达');
  } finally {
    window.clearTimeout(timeout);
  }
}

export async function sendKvCommand(command: KvCommand) {
  const response = await requestJson<KvResponse>('/api/kv', {
    method: 'POST',
    body: JSON.stringify(command),
  });
  if (!response.ok) throw new RustKvApiError(response.error.code, response.error.message);
  return response.data;
}

export async function startRemoteConcurrency(input: {
  clients: number;
  requestsPerClient: number;
  workload: 'read' | 'mixed' | 'write';
}) {
  return requestJson<{ accepted: boolean }>('/api/experiment/start', {
    method: 'POST',
    body: JSON.stringify({ type: 'concurrency', ...input }),
  });
}

export async function stopRemoteConcurrency() {
  return requestJson<{ stopped: boolean }>('/api/experiment/stop', {
    method: 'POST',
    body: JSON.stringify({ type: 'concurrency' }),
  });
}

export async function readRemoteConcurrency() {
  return requestJson<RemoteExperimentState>('/api/experiment/state');
}

export async function startRemoteBenchmark(input: {
  clients: number;
  benchmarkProfile: 'quick' | 'full';
  requests?: number;
  runtime: 'sync' | 'async';
  lock: 'mutex' | 'rwlock';
  workload: 'read' | 'mixed' | 'write';
}) {
  return requestJson<{ accepted: boolean }>('/api/benchmark/start', {
    method: 'POST',
    body: JSON.stringify({
      scales: [input.clients],
      benchmarkProfile: input.benchmarkProfile,
      ...(input.benchmarkProfile === 'full'
        ? { requestsPerScale: input.requests ?? 10_000 }
        : {}),
      runtime: input.runtime,
      lock: input.lock,
      workload: input.workload,
      persistence: { wal: true, syncData: true },
    }),
  });
}

export async function readRemoteBenchmark() {
  return requestJson<RemoteBenchmarkState>('/api/benchmark/state');
}

export async function stopRemoteBenchmark() {
  return requestJson<{ stopped: boolean }>('/api/benchmark/stop', { method: 'POST' });
}

export async function resetRemoteBenchmarkEnvironment(benchmarkProfile: 'quick' | 'full') {
  return requestJson<{
    reset: boolean;
    ready: boolean;
    resetEpoch: number;
    environmentStrategy: string;
  }>('/api/benchmark/reset', {
    method: 'POST',
    body: JSON.stringify({
      benchmarkProfile,
      dataset: { size: 10_000, valueSize: 128 },
      sampling: benchmarkProfile === 'quick'
        ? { kind: 'fixed-duration', durationMs: 3_000 }
        : { kind: 'fixed-requests', requestsPerScale: 10_000 },
      persistence: { wal: true, syncData: true },
      protocol: 'json-lines',
      network: 'localhost',
    }),
  }, 30_000);
}

export async function killRemoteServer() {
  return requestJson<{ state?: string }>('/api/server/kill', { method: 'POST' });
}

export async function restartRemoteServer() {
  return requestJson<{ state?: string }>('/api/server/restart', { method: 'POST' });
}

export async function readRemoteServerState() {
  return requestJson<{
    state: 'ONLINE' | 'OFFLINE' | 'STARTING' | 'RECOVERING' | 'ERROR';
    walReplayCount?: number;
  }>('/api/server/state');
}

const recoveryPhases = new Set<RemoteRecoveryState['phase']>([
  'IDLE',
  'PREPARED',
  'CRASHED',
  'RECOVERING',
  'VERIFIED',
  'ERROR',
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function parseRemoteStorageStats(value: unknown, field: string): RemoteStorageStats {
  if (!isRecord(value)) {
    throw new RustKvApiError('INVALID_RESPONSE', `${field} 存储统计格式无效`);
  }
  const numericFields = [
    'entries',
    'walRecords',
    'walBytes',
    'snapshotBytes',
    'totalBytes',
    'lastSequence',
  ] as const;
  if (!numericFields.every((name) => typeof value[name] === 'number'
    && Number.isSafeInteger(value[name])
    && value[name] >= 0)) {
    throw new RustKvApiError('INVALID_RESPONSE', `${field} 存储统计格式无效`);
  }

  return {
    entries: value.entries as number,
    walRecords: value.walRecords as number,
    walBytes: value.walBytes as number,
    snapshotBytes: value.snapshotBytes as number,
    totalBytes: value.totalBytes as number,
    lastSequence: value.lastSequence as number,
  };
}

function parseRemoteStorageState(value: unknown, field = 'storage'): RemoteStorageState {
  if (!isRecord(value)
    || typeof value.engine !== 'string'
    || typeof value.writable !== 'boolean') {
    throw new RustKvApiError('INVALID_RESPONSE', `${field} 存储状态格式无效`);
  }
  return {
    engine: value.engine,
    writable: value.writable,
    ...parseRemoteStorageStats(value, field),
  };
}

function parseRemoteStorageCompactResult(value: unknown): RemoteStorageCompactResult {
  if (!isRecord(value)
    || typeof value.compacted !== 'boolean'
    || typeof value.compactMs !== 'number'
    || !Number.isFinite(value.compactMs)
    || value.compactMs < 0) {
    throw new RustKvApiError('INVALID_RESPONSE', 'Compact 结果格式无效');
  }
  return {
    compacted: value.compacted,
    compactMs: value.compactMs,
    before: parseRemoteStorageStats(value.before, 'before'),
    after: parseRemoteStorageStats(value.after, 'after'),
  };
}

function parseRecoveryEvidence(value: unknown, field: string): RemoteRecoveryEvidence | null {
  if (value === null) return null;
  if (!isRecord(value)
    || typeof value.count !== 'number'
    || !Number.isInteger(value.count)
    || value.count < 0
    || typeof value.fingerprint !== 'string'
    || !Array.isArray(value.samples)
    || !value.samples.every((sample) => isRecord(sample)
      && typeof sample.key === 'string'
      && typeof sample.value === 'string')) {
    throw new RustKvApiError('INVALID_RESPONSE', `${field} 恢复证据格式无效`);
  }
  return {
    count: value.count as number,
    fingerprint: value.fingerprint,
    samples: value.samples.map((sample) => ({
      key: (sample as Record<string, unknown>).key as string,
      value: (sample as Record<string, unknown>).value as string,
    })),
  };
}

function parseRemoteRecoveryState(value: unknown): RemoteRecoveryState {
  if (!isRecord(value)
    || typeof value.phase !== 'string'
    || !recoveryPhases.has(value.phase as RemoteRecoveryState['phase'])
    || typeof value.progress !== 'number'
    || !Number.isFinite(value.progress)
    || value.progress < 0
    || value.progress > 100
    || typeof value.lost !== 'number'
    || !Number.isInteger(value.lost)
    || value.lost < 0
    || typeof value.verified !== 'boolean'
    || typeof value.walReplayCount !== 'number'
    || !Number.isInteger(value.walReplayCount)
    || value.walReplayCount < 0
    || typeof value.recoveryTimeMs !== 'number'
    || !Number.isFinite(value.recoveryTimeMs)
    || value.recoveryTimeMs < 0
    || !Array.isArray(value.logs)
    || !value.logs.every((log) => typeof log === 'string')) {
    throw new RustKvApiError('INVALID_RESPONSE', '恢复实验状态格式无效');
  }
  return {
    phase: value.phase as RemoteRecoveryState['phase'],
    progress: value.progress,
    before: parseRecoveryEvidence(value.before, 'before'),
    after: parseRecoveryEvidence(value.after, 'after'),
    lost: value.lost as number,
    verified: value.verified,
    walReplayCount: value.walReplayCount as number,
    recoveryTimeMs: value.recoveryTimeMs,
    logs: value.logs as string[],
  };
}

async function requestRecoveryState(path: string, init?: RequestInit, timeoutMs?: number) {
  const response = await requestJson<unknown>(path, init, timeoutMs);
  return parseRemoteRecoveryState(response);
}

export async function prepareRemoteRecovery(count: number) {
  return requestRecoveryState('/api/recovery/prepare', {
    method: 'POST',
    body: JSON.stringify({ count }),
  }, 180_000);
}

export async function killRemoteRecovery() {
  return requestRecoveryState('/api/recovery/kill', { method: 'POST' }, 10_000);
}

export async function restartRemoteRecovery() {
  return requestRecoveryState('/api/recovery/restart', { method: 'POST' }, 30_000);
}

export async function readRemoteRecovery() {
  return requestRecoveryState('/api/recovery/state', undefined, 5_000);
}

export async function readRemoteStorageState() {
  const response = await requestJson<unknown>('/api/storage/state', undefined, 5_000);
  return parseRemoteStorageState(response);
}

export async function compactRemoteStorage() {
  const response = await requestJson<unknown>('/api/storage/compact', {
    method: 'POST',
  }, 120_000);
  return parseRemoteStorageCompactResult(response);
}
