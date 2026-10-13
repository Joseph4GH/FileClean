import type { DuplicateGroup } from './types';
import { getLocale, t } from './i18n';

/** 字节数 → 人类可读。与 Rust 侧 pipeline::human_bytes 保持同一套规则。 */
export function humanBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${value.toFixed(2)} ${units[unit]}`;
}

/** 大数字加千分位（随界面语言切换分组符）。 */
export function num(value: number): string {
  const locale = getLocale() === 'zh' ? 'zh-CN' : 'en-US';
  return value.toLocaleString(locale);
}

export function formatTime(unix: number): string {
  if (!unix || unix <= 0) return '—';
  const d = new Date(unix * 1000);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(
    d.getMinutes(),
  )}`;
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return t('dur.ms', { ms });
  const s = ms / 1000;
  if (s < 60) return t('dur.sec', { s: s.toFixed(1) });
  const m = Math.floor(s / 60);
  return t('dur.min', { m, s: Math.round(s - m * 60) });
}

export function percent(value: number): string {
  return `${(value * 100).toFixed(1)}%`;
}

export const IMAGE_EXT = new Set([
  'jpg', 'jpeg', 'jpe', 'jfif', 'png', 'gif', 'bmp', 'tif', 'tiff', 'webp', 'heic', 'heif', 'avif',
]);

export const AUDIO_EXT = new Set([
  'mp3', 'm4a', 'm4b', 'aac', 'flac', 'wav', 'wave', 'ogg', 'oga', 'aiff', 'aif', 'aifc', 'alac', 'caf',
]);

export const VIDEO_EXT = new Set(['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v', 'wmv', 'flv']);

export const ARCHIVE_EXT = new Set(['zip', '7z', 'rar', 'jar', 'apk', 'cbz', 'war', 'ear']);

export type FileGlyph = 'image' | 'audio' | 'video' | 'archive' | 'code' | 'doc';

export function glyphFor(extension: string): FileGlyph {
  const ext = extension.toLowerCase();
  if (IMAGE_EXT.has(ext)) return 'image';
  if (AUDIO_EXT.has(ext)) return 'audio';
  if (VIDEO_EXT.has(ext)) return 'video';
  if (ARCHIVE_EXT.has(ext)) return 'archive';
  if (['js', 'ts', 'tsx', 'jsx', 'rs', 'py', 'go', 'java', 'c', 'cpp', 'h', 'json', 'toml', 'yaml', 'yml', 'sh'].includes(ext))
    return 'code';
  return 'doc';
}

/** 分组摘要文案，列表与详情共用。 */
export function groupSummary(group: DuplicateGroup): string {
  const parts = [t('common.fileCount', { n: group.files.length })];
  if (group.similarity != null) parts.push(t('common.similarity', { pct: percent(group.similarity) }));
  if (group.files.some((f) => f.protected)) parts.push(t('groupSummary.protected'));
  return parts.join(' · ');
}

export function shortPath(path: string, max = 72): string {
  if (path.length <= max) return path;
  const head = path.slice(0, Math.floor(max / 2) - 2);
  const tail = path.slice(path.length - Math.floor(max / 2) + 1);
  return `${head}…${tail}`;
}

export function baseName(path: string): string {
  const idx = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return idx >= 0 ? path.slice(idx + 1) : path;
}

export function parentDir(path: string): string {
  const idx = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return idx > 0 ? path.slice(0, idx) : '';
}
