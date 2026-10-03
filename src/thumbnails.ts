/** 缩略图按需加载：带并发限制与结果缓存，避免虚拟列表滚动时打爆 IPC。 */

import { api } from './api';

const cache = new Map<string, string | null>();
const inflight = new Map<string, Promise<string | null>>();
const MAX_CONCURRENCY = 4;
let active = 0;
const waiters: Array<() => void> = [];

function release(): void {
  active -= 1;
  const next = waiters.shift();
  if (next) next();
}

async function acquire(): Promise<void> {
  if (active < MAX_CONCURRENCY) {
    active += 1;
    return;
  }
  await new Promise<void>((resolve) => waiters.push(resolve));
  active += 1;
}

/** 取缩略图 data URL；失败或非图片返回 null（调用方退化到图标）。 */
export function getThumbnail(path: string, maxEdge = 96): Promise<string | null> {
  if (cache.has(path)) return Promise.resolve(cache.get(path) ?? null);

  const existing = inflight.get(path);
  if (existing) return existing;

  const task = (async () => {
    await acquire();
    try {
      const result = await api.previewThumbnail(path, maxEdge);
      const value = result.error ? null : result.dataUrl;
      cache.set(path, value);
      return value;
    } catch {
      cache.set(path, null);
      return null;
    } finally {
      release();
      inflight.delete(path);
    }
  })();

  inflight.set(path, task);
  return task;
}

export function cachedThumbnail(path: string): string | null | undefined {
  return cache.get(path);
}

export function clearThumbnailCache(): void {
  cache.clear();
}
