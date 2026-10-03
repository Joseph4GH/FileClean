import type { DuplicateGroup, MarkDecision, MatchKind } from './types';

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

/** 大数字加千分位。 */
export function num(value: number): string {
  return value.toLocaleString('zh-CN');
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
  if (ms < 1000) return `${ms} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)} s`;
  const m = Math.floor(s / 60);
  return `${m} 分 ${Math.round(s - m * 60)} 秒`;
}

export function percent(value: number): string {
  return `${(value * 100).toFixed(1)}%`;
}

export const KIND_META: Record<MatchKind, { label: string; tone: 'accent' | 'warn' | 'ok' | '' }> = {
  exact: { label: '完全重复', tone: 'accent' },
  perceptual: { label: '图片相似', tone: 'warn' },
  audio: { label: '音频相似', tone: 'warn' },
  archiveInternal: { label: '压缩包内', tone: '' },
  rawJpegPair: { label: 'RAW/JPEG 配对', tone: 'ok' },
};

export const DECISION_META: Record<MarkDecision, { label: string; tone: 'ok' | 'danger' | '' }> = {
  keep: { label: '保留', tone: 'ok' },
  remove: { label: '待清理', tone: 'danger' },
  undecided: { label: '未决策', tone: '' },
};

export const ALGORITHM_OPTIONS = [
  {
    value: 'blake3',
    label: 'BLAKE3（默认，最快）',
    hint: '密码学强度且支持单文件内多核并行，硬盘与固态盘上都是最快的稳妥选择',
  },
  {
    value: 'xxhash3',
    label: 'xxHash3（最快，非密码学）',
    hint: '速度略高于 BLAKE3，但没有抗碰撞保证；仅用于本机去重时足够',
  },
  {
    value: 'sha256',
    label: 'SHA-256（最稳）',
    hint: '广泛认可的密码学哈希，速度慢于前两者，适合需要归档留痕的合规场景',
  },
  { value: 'sha1', label: 'SHA-1', hint: '已被证明存在碰撞，仅在与旧工具互认指纹时使用' },
  { value: 'md5', label: 'MD5（兼容旧工具）', hint: '速度尚可但碰撞已知，只建议用于与旧清单比对' },
] as const;

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
  const parts = [`${group.files.length} 个文件`];
  if (group.similarity != null) parts.push(`相似度 ${percent(group.similarity)}`);
  if (group.files.some((f) => f.protected)) parts.push('含受保护文件');
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

export function ruleLabel(rule: { type: string; pattern?: string; extensions?: string[] }): string {
  switch (rule.type) {
    case 'keepNewest':
      return '保留最新修改的文件';
    case 'keepOldest':
      return '保留最早修改的文件';
    case 'keepOldestCreated':
      return '保留最早创建（原片优先）';
    case 'keepShortestPath':
      return '保留路径最短的';
    case 'keepLongestName':
      return '保留文件名最长的';
    case 'keepShallowest':
      return '保留目录层级最浅的';
    case 'keepLargest':
      return '保留体积最大的';
    case 'keepMostLinked':
      return '保留硬链接数最多的';
    case 'keepInPath':
      return `优先保留位于「${rule.pattern ?? ''}」的文件`;
    case 'preferExtension':
      return `扩展名优先：${(rule.extensions ?? []).join(' > ') || '（未设置）'}`;
    default:
      return rule.type;
  }
}
