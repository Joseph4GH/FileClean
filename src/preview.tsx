/**
 * 无后端 UI 预览入口（`npm run preview:ui`）。
 *
 * 为什么需要它：本项目的界面是 Tauri WebView 里的前端产物，要看真实版式
 * 通常得先编译 Rust（分钟级）。改版式时这个循环太慢，所以这里用官方
 * `@tauri-apps/api/mocks` 顶替 IPC，返回固定的假数据，让 `vite` 直接跑起来。
 *
 * 用法：
 *   npm run preview:ui            → http://localhost:4175/preview.html
 *   npm run preview:ui            → 再加 ?demo=1 可自动载入一份演示扫描结果
 *
 * 这里的数据只服务于「看布局」，与 Rust 侧的真实返回无关；
 * 契约一旦变化，`tsc` 会在这里先报错，正好当一道防漂移的哨兵。
 */

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { mockIPC } from '@tauri-apps/api/mocks';

import App from './App';
import './styles.css';
import type {
  AutoMarkConfig,
  DuplicateFile,
  DuplicateGroup,
  FileEntry,
  MarkDecision,
  ScanOptions,
  ScanResult,
  SessionMeta,
  SystemInfo,
} from './types';

const NOW = Math.floor(Date.now() / 1000);
const ROOT = '/Volumes/Archive';
const DEMO_SCAN_ID = 'demo-scan-0001';

// --------------------------------------------------------------------------- //
// 假数据
// --------------------------------------------------------------------------- //

const SYSTEM_INFO: SystemInfo = {
  platform: 'macos',
  arch: 'aarch64',
  logicalCores: 16,
  effectiveThreads: 16,
  dataDir: '~/Library/Application Support/FileClean',
  dbPath: '~/Library/Application Support/FileClean/cache.db',
  quarantineDir: '~/Library/Application Support/FileClean/quarantine',
  exportDir: '~/Library/Application Support/FileClean/exports',
  protectSystemDirs: true,
  cacheEntries: 1284,
  cacheTtlDays: 30,
};

const DEFAULT_OPTIONS: ScanOptions = {
  roots: [`${ROOT}/photos/2024`, `${ROOT}/docs`],
  filter: {
    includeExtensions: [],
    excludeExtensions: ['iso', 'dmg'],
    minSize: 1024,
    maxSize: null,
    modifiedAfter: null,
    modifiedBefore: null,
    includeHidden: false,
    includeSystem: false,
    followSymlinks: false,
    excludeGlobs: [
      '**/node_modules/**',
      '**/.git/**',
      '**/target/debug/**',
      '**/Library/Caches/**',
      '**/.Trash/**',
      '**/.Trashes/**',
      '**/$RECYCLE.BIN/**',
      '**/System Volume Information/**',
    ],
    minDepth: 0,
  },
  algorithm: 'blake3',
  useCache: true,
  detectPerceptual: true,
  perceptualThreshold: 0.9,
  audioFingerprint: false,
  audioThreshold: 0.92,
  scanArchives: false,
  archiveMaxEntryBytes: 512 * 1024 * 1024,
  rawJpegPairing: true,
  checkReferences: true,
  protectSystemDirs: true,
  minGroupSize: 2,
  threads: 0,
};

const MARK_CONFIG: AutoMarkConfig = {
  rules: [
    { rule: { type: 'keepInPath', pattern: '原片' }, enabled: true },
    {
      rule: {
        type: 'preferExtension',
        extensions: ['cr2', 'cr3', 'nef', 'arw', 'dng', 'raf', 'orf', 'rw2', 'pef', 'srw'],
      },
      enabled: true,
    },
    { rule: { type: 'keepOldestCreated' }, enabled: true },
    { rule: { type: 'keepShortestPath' }, enabled: true },
    { rule: { type: 'keepLargest' }, enabled: false },
  ],
  keepPerGroup: 1,
  neverDeleteProtected: true,
  alsoMarkSimilar: false,
};

const SESSIONS: SessionMeta[] = [
  {
    id: 'sess-1',
    name: '照片盘清理 2026-09-28',
    createdAt: NOW - 86_400 * 5,
    updatedAt: NOW - 86_400 * 5,
    roots: [`${ROOT}/photos`],
    groupCount: 143,
    wastedBytes: 12_884_901_888,
    filePath: null,
  },
  {
    id: 'sess-2',
    name: '文档目录审计',
    createdAt: NOW - 86_400 * 21,
    updatedAt: NOW - 86_400 * 20,
    roots: [`${ROOT}/docs`],
    groupCount: 18,
    wastedBytes: 24_117_248,
    filePath: null,
  },
];

function entry(rel: string, size: number, extension: string): FileEntry {
  const segments = rel.split('/');
  return {
    path: `${ROOT}/${rel}`,
    root: `${ROOT}/${segments[0]}`,
    size,
    modified: NOW - 86_400 * 30,
    created: NOW - 86_400 * 120,
    extension,
    inode: 900_000 + rel.length * 37,
    device: 16_777_220,
    hardLinks: 1,
    isSymlink: false,
    depth: segments.length,
    relativePath: rel,
  };
}

function dup(
  rel: string,
  size: number,
  extension: string,
  decision: MarkDecision,
  extra: Partial<DuplicateFile> = {},
): DuplicateFile {
  return {
    entry: entry(rel, size, extension),
    hash: '3a7bd3e2',
    fingerprint: null,
    decision,
    reasons: [],
    protected: null,
    isPreferred: decision === 'keep',
    archiveInnerPath: null,
    isSymlink: false,
    ...extra,
  };
}

const GROUPS: DuplicateGroup[] = [
  {
    id: 'g1',
    kind: 'exact',
    kindLabel: '完全重复',
    signature: '3a7bd3e2',
    title: 'report.txt',
    files: [
      dup('docs/report.txt', 1_160, 'txt', 'keep'),
      dup('docs/backup/report-copy.txt', 1_160, 'txt', 'remove'),
      dup('docs/backup/renamed.txt', 1_160, 'txt', 'remove'),
      dup('docs/backup/hardlink.txt', 1_160, 'txt', 'keep', {
        protected: '与组内其它条目指向同一物理文件（硬链接）',
      }),
    ],
    wastedBytes: 2_320,
    totalBytes: 4_640,
    similarity: null,
    informational: false,
  },
  {
    id: 'g2',
    kind: 'exact',
    kindLabel: '完全重复',
    signature: 'c1f9a04b',
    title: 'beach.png',
    files: [
      dup('photos/2024/beach.png', 45_310, 'png', 'keep'),
      dup('photos/2024/beach-copy.png', 45_310, 'png', 'remove'),
    ],
    wastedBytes: 45_310,
    totalBytes: 90_620,
    similarity: null,
    informational: false,
  },
  {
    id: 'g3',
    kind: 'perceptual',
    kindLabel: '图片相似',
    signature: '726a5650bba4650f',
    title: 'sunset.png',
    files: [
      dup('photos/2024/sunset.png', 1_284_300, 'png', 'undecided', {
        fingerprint: '726a5650bba4650f/b0f0d0c8a4e2c1a0',
        isPreferred: true,
      }),
      dup('photos/2024/sunset-hd.png', 4_812_960, 'png', 'undecided', {
        fingerprint: '726a5650bba4650f/b0f0d0c8a4e2c1a0',
      }),
    ],
    wastedBytes: 4_812_960,
    totalBytes: 6_097_260,
    similarity: 1,
    informational: false,
  },
  {
    id: 'g4',
    kind: 'rawJpegPair',
    kindLabel: 'RAW/JPEG 配对',
    signature: 'IMG_0001',
    title: 'IMG_0001',
    files: [
      dup('photos/2024/IMG_0001.CR2', 26_214_400, 'cr2', 'keep', {
        protected: '属于 RAW/JPEG 配对，不参与重复清理',
      }),
      dup('photos/2024/IMG_0001.JPG', 4_194_304, 'jpg', 'keep', {
        protected: '属于 RAW/JPEG 配对，不参与重复清理',
      }),
    ],
    wastedBytes: 0,
    totalBytes: 30_408_704,
    similarity: null,
    informational: true,
  },
];

const DEMO_RESULT: ScanResult = {
  scanId: DEMO_SCAN_ID,
  roots: DEFAULT_OPTIONS.roots,
  algorithm: 'blake3',
  options: DEFAULT_OPTIONS,
  groups: GROUPS,
  stats: {
    filesSeen: 14_203,
    filesScanned: 11_874,
    filesSkipped: 2_329,
    bytesScanned: 486_539_182_080,
    cacheHits: 11_502,
    cacheMisses: 372,
    partialHashed: 2_914,
    fullHashed: 1_046,
    groups: GROUPS.length,
    duplicateFiles: 7,
    wastedBytes: 4_860_590,
    protectedFiles: 4,
    archiveEntries: 3,
    referencedFiles: 1,
    durationMs: 8_420,
    protectedDirsSkipped: 12,
    inaccessiblePaths: 0,
  },
  createdAt: NOW,
};

// --------------------------------------------------------------------------- //
// IPC 桩
// --------------------------------------------------------------------------- //

const TEXT_DIFF = {
  leftPath: `${ROOT}/docs/report.txt`,
  rightPath: `${ROOT}/docs/backup/report-copy.txt`,
  identical: false,
  identicalPrefixBytes: 428,
  firstDifferenceOffset: 428,
  leftSnippet: '季度汇总\n\n收入 1,284,300\n支出   960,120\n结余   324,180\n',
  rightSnippet: '季度汇总\n\n收入 1,284,300\n支出   960,120\n结余   324,100\n',
  leftTruncated: false,
  rightTruncated: false,
  binary: false,
};

/** 演示用的三色渐变缩略图，避免预览依赖真实文件。 */
const DEMO_THUMB =
  'data:image/svg+xml;base64,' +
  btoa(
    '<svg xmlns="http://www.w3.org/2000/svg" width="160" height="120">' +
      '<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">' +
      '<stop offset="0" stop-color="#f59e0b"/><stop offset="0.55" stop-color="#ef4444"/>' +
      '<stop offset="1" stop-color="#6d28d9"/></linearGradient></defs>' +
      '<rect width="160" height="120" fill="url(#g)"/>' +
      '<circle cx="118" cy="34" r="18" fill="#fde68a" opacity="0.9"/></svg>',
  );

const RESPONSES: Record<string, unknown> = {
  system_info: SYSTEM_INFO,
  default_options: DEFAULT_OPTIONS,
  default_mark_config: MARK_CONFIG,
  list_sessions: SESSIONS,
  list_scans: [],
  cache_stats: { entries: 1_284, bytes: 41_943_040 },
  clear_cache: 1_284,
  start_scan: DEMO_SCAN_ID,
  cancel_scan: null,
  get_scan_result: DEMO_RESULT,
  get_scan_status: {
    scanId: DEMO_SCAN_ID,
    status: 'done',
    progress: null,
    error: null,
    startedAt: NOW,
    finishedAt: NOW,
  },
  apply_auto_mark: DEMO_RESULT,
  invert_group: DEMO_RESULT,
  set_file_decision: DEMO_RESULT,
  set_group_decision: DEMO_RESULT,
  reveal_in_finder: null,
  preview_text: TEXT_DIFF,
  preview_thumbnail: {
    path: '',
    dataUrl: DEMO_THUMB,
    width: 160,
    height: 120,
    error: null,
  },
  preview_audio: { path: '', durationSecs: 0, bands: [], error: '演示模式不加载音频' },
  preview_plan: { count: 0, bytes: 0, human: '0 B', blocked: [] },
  execute_actions: {
    dryRun: true,
    succeeded: 0,
    failed: 0,
    freedBytes: 0,
    journalId: 'demo-journal',
    outcomes: [],
    skipped: [],
  },
  export_report: { path: '/tmp/demo-report.html', format: 'html', rows: 7, bytes: 24_576 },
  save_session: SESSIONS[0],
  delete_session: null,
  load_session: DEMO_RESULT,
  import_session_file: DEMO_RESULT,
  export_session_file: '/tmp/demo-session.json',
  remove_archive_entries: 0,
  undo_action: { restored: 0, errors: [], hint: '演示模式无可撤销的操作' },
};

mockIPC((command) => {
  if (command in RESPONSES) return RESPONSES[command];
  // 未覆盖的命令返回一个明确的失败，而不是 undefined —— 便于发现遗漏。
  throw { code: 'UNSUPPORTED', message: `预览模式未实现命令：${command}` };
}, { shouldMockEvents: true });

// --------------------------------------------------------------------------- //
// 演示流程：自动触发一次「扫描 → 完成」，把结果视图也带出来
// --------------------------------------------------------------------------- //

async function runDemo() {
  const { emit } = await import('@tauri-apps/api/event');

  const startButton = Array.from(document.querySelectorAll('button')).find((b) =>
    b.textContent?.includes('开始扫描'),
  );
  if (!startButton) return;
  startButton.click();

  // 等 start_scan 的 Promise 结算、scanId 落到 state 上，再推完成事件。
  await new Promise((resolve) => setTimeout(resolve, 400));
  await emit('scan://done', {
    scanId: DEMO_SCAN_ID,
    groups: DEMO_RESULT.groups.length,
    wastedBytes: DEMO_RESULT.stats.wastedBytes,
    durationMs: DEMO_RESULT.stats.durationMs,
  });
}

const root = document.getElementById('root');
if (!root) throw new Error('找不到 #root 挂载点');

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

const params = new URLSearchParams(window.location.search);
if (params.has('view')) {
  const target = params.get('view') === 'settings' ? '扫描规则' : '扫描';
  setTimeout(() => {
    Array.from(document.querySelectorAll('button'))
      .find((b) => b.textContent?.trim() === target)
      ?.click();
  }, 250);
}
if (params.has('demo')) {
  setTimeout(() => void runDemo(), 700);
}
