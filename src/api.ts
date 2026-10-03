/**
 * IPC 客户端：所有后端调用唯一入口，Base 概念不适用（Tauri IPC 无 URL），
 * 但统一在这里做三件事：
 *   1. 参数与返回值类型标注（跨端类型安全的落点）；
 *   2. 把后端的 `{ code, message }` 规范化错误转成 `ApiError`；
 *   3. 事件通道的订阅/解绑统一封装，杜绝内存泄漏。
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  ActionPlan,
  ActionResult,
  AudioPreview,
  AutoMarkConfig,
  CacheStats,
  ExportFormat,
  ExportSummary,
  MarkDecision,
  ScanOptions,
  ScanProgress,
  ScanResult,
  ScanStatusPayload,
  SessionMeta,
  SystemInfo,
  TextDiffPreview,
  ThumbnailPreview,
  UndoResult,
  PlanPreview,
} from './types';

export class ApiError extends Error {
  code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = 'ApiError';
    this.code = code;
  }

  /** 4xx 语义（业务拒绝 / 输入非法）不应重试；只有真正的 IO/DB 抖动才值得重试。 */
  get retryable(): boolean {
    return ['IO', 'DB', 'INTERNAL'].includes(this.code);
  }
}

function normalize(error: unknown): ApiError {
  if (error && typeof error === 'object' && 'code' in error && 'message' in error) {
    const e = error as { code: string; message: string };
    return new ApiError(e.code, e.message);
  }
  if (typeof error === 'string') {
    return new ApiError('UNKNOWN', error);
  }
  return new ApiError('UNKNOWN', '发生未知错误，请查看日志');
}

/** 带退避重试的调用：仅对可重试错误生效，最多 3 次。 */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  let lastError: ApiError | null = null;

  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      return await invoke<T>(command, args);
    } catch (raw) {
      const error = normalize(raw);
      lastError = error;
      if (!error.retryable) break;
      await new Promise((resolve) => setTimeout(resolve, 150 * 2 ** attempt));
    }
  }
  throw lastError ?? new ApiError('UNKNOWN', '调用失败');
}

// ============================ 扫描 ============================

export const api = {
  startScan: (options: ScanOptions, markConfig: AutoMarkConfig) =>
    call<string>('start_scan', { options, markConfig }),

  cancelScan: (scanId: string) => call<void>('cancel_scan', { scanId }),

  getScanStatus: (scanId: string) => call<ScanStatusPayload>('get_scan_status', { scanId }),

  getScanResult: (scanId: string) => call<ScanResult>('get_scan_result', { scanId }),

  listScans: () => call<ScanStatusPayload[]>('list_scans'),

  defaultOptions: () => call<ScanOptions>('default_options'),

  defaultMarkConfig: () => call<AutoMarkConfig>('default_mark_config'),

  systemInfo: () => call<SystemInfo>('system_info'),

  cacheStats: () => call<CacheStats>('cache_stats'),

  clearCache: () => call<number>('clear_cache'),

  // ---- 结果操作 ----

  applyAutoMark: (scanId: string, markConfig: AutoMarkConfig) =>
    call<ScanResult>('apply_auto_mark', { scanId, markConfig }),

  setFileDecision: (scanId: string, groupId: string, path: string, decision: MarkDecision) =>
    call<ScanResult>('set_file_decision', { scanId, groupId, path, decision }),

  setGroupDecision: (scanId: string, groupId: string, decision: MarkDecision) =>
    call<ScanResult>('set_group_decision', { scanId, groupId, decision }),

  invertGroup: (scanId: string, groupId: string | null) =>
    call<ScanResult>('invert_group', { scanId, groupId }),

  previewText: (left: string, right: string) =>
    call<TextDiffPreview>('preview_text', { left, right }),

  previewThumbnail: (path: string, maxEdge?: number) =>
    call<ThumbnailPreview>('preview_thumbnail', { path, maxEdge: maxEdge ?? null }),

  previewAudio: (path: string) => call<AudioPreview>('preview_audio', { path }),

  exportReport: (scanId: string, format: ExportFormat, outPath?: string) =>
    call<ExportSummary>('export_report', { scanId, format, outPath: outPath ?? null }),

  // ---- 动作 ----

  previewPlan: (plan: ActionPlan) => call<PlanPreview>('preview_plan', { plan }),

  executeActions: (scanId: string, plan: ActionPlan) =>
    call<ActionResult>('execute_actions', { scanId, plan }),

  undoAction: (journalId: string) => call<UndoResult>('undo_action', { journalId }),

  removeArchiveEntries: (archivePath: string, innerPaths: string[], confirmed: boolean) =>
    call<number>('remove_archive_entries', { archivePath, innerPaths, confirmed }),

  revealInFinder: (path: string) => call<void>('reveal_in_finder', { path }),

  // ---- 会话 ----

  saveSession: (scanId: string, name?: string) =>
    call<SessionMeta>('save_session', { scanId, name: name ?? null }),

  updateSession: (sessionId: string, scanId: string, name?: string) =>
    call<SessionMeta>('update_session', { sessionId, scanId, name: name ?? null }),

  listSessions: () => call<SessionMeta[]>('list_sessions'),

  deleteSession: (sessionId: string) => call<void>('delete_session', { sessionId }),

  loadSession: (sessionId: string) => call<ScanResult>('load_session', { sessionId }),

  exportSessionFile: (scanId: string, outPath?: string) =>
    call<string>('export_session_file', { scanId, outPath: outPath ?? null }),

  importSessionFile: (path: string) => call<ScanResult>('import_session_file', { path }),
};

// ============================ 事件通道 ============================

export const EVENTS = {
  progress: 'scan://progress',
  done: 'scan://done',
  error: 'scan://error',
} as const;

export interface ScanDoneEvent {
  scanId: string;
  groups: number;
  wastedBytes: number;
  durationMs: number;
}

export interface ScanErrorEvent {
  scanId: string;
  code: string;
  message: string;
}

export function onProgress(handler: (p: ScanProgress) => void): Promise<UnlistenFn> {
  return listen<ScanProgress>(EVENTS.progress, (event) => handler(event.payload));
}

export function onScanDone(handler: (p: ScanDoneEvent) => void): Promise<UnlistenFn> {
  return listen<ScanDoneEvent>(EVENTS.done, (event) => handler(event.payload));
}

export function onScanError(handler: (p: ScanErrorEvent) => void): Promise<UnlistenFn> {
  return listen<ScanErrorEvent>(EVENTS.error, (event) => handler(event.payload));
}

/** 把后端错误映射成用户可读文案（前端边界职责）。 */
export function humanize(error: unknown): string {
  if (error instanceof ApiError) {
    switch (error.code) {
      case 'CANCELLED':
        return '操作已取消';
      case 'FORBIDDEN':
        return error.message;
      case 'NOT_FOUND':
        return `找不到目标：${error.message}`;
      case 'UNSUPPORTED':
        return error.message;
      case 'CONFIG':
        return error.message;
      case 'IO':
        return `读写文件失败：${error.message}`;
      case 'DB':
        return `本地数据库异常：${error.message}`;
      default:
        return error.message;
    }
  }
  if (error instanceof Error) return error.message;
  return String(error);
}
