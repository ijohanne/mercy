const PROXY_BASE = '/api/proxy';

export async function proxyGet(path: string): Promise<Response> {
  return fetch(`${PROXY_BASE}/${path}`, { cache: 'no-store' });
}

export async function proxyPost(path: string): Promise<Response> {
  return fetch(`${PROXY_BASE}/${path}`, { method: 'POST', cache: 'no-store' });
}

export async function proxyPostJson(path: string, body: unknown): Promise<Response> {
  return fetch(`${PROXY_BASE}/${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
    cache: 'no-store',
  });
}

export interface StatusResponse {
  phase: 'idle' | 'preparing' | 'ready' | 'scanning' | 'paused';
  running: boolean;
  paused: boolean;
  current_kingdom: number | null;
  exchanges_found: number;
  manual_scan_kingdom: number | null;
}

export interface Exchange {
  kingdom: number;
  x: number;
  y: number;
  found_at: string;
  scan_duration_secs: number | null;
  confirmed: boolean;
}
