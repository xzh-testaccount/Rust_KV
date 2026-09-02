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
};

export type RemoteBenchmarkState = {
  status: 'IDLE' | 'RUNNING' | 'COMPLETED' | 'STOPPED' | 'INTERRUPTED';
  progress: number;
  points: RemoteBenchmarkPoint[];
  error?: string;
};

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
  requests: number;
  runtime: 'sync' | 'async';
  lock: 'mutex' | 'rwlock';
  workload: 'read' | 'mixed' | 'write';
}) {
  return requestJson<{ accepted: boolean }>('/api/benchmark/start', {
    method: 'POST',
    body: JSON.stringify({
      scales: [input.clients],
      requestsPerScale: input.requests,
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

export async function resetRemoteBenchmarkEnvironment() {
  return requestJson<{ reset: boolean }>('/api/benchmark/reset', {
    method: 'POST',
    body: JSON.stringify({
      dataset: { size: 10_000, valueSize: 128 },
      requestsPerScale: 10_000,
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
