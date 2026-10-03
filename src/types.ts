/**
 * 与 Rust 侧 `domain::models` 一一对应的类型契约。
 * Rust 端统一 `#[serde(rename_all = "camelCase")]`，因此这里全部用驼峰。
 * 任何一端改了字段，另一端会在这里得到编译期/运行期的明确反馈，而不是静默错位。
 */

export type HashAlgorithm = 'md5' | 'sha1' | 'sha256' | 'xxhash3' | 'blake3';

export type ScanPhase =
  | 'idle'
  | 'walking'
  | 'filtering'
  | 'partialHash'
  | 'fullHash'
  | 'perceptual'
  | 'audio'
  | 'archives'
  | 'references'
  | 'marking'
  | 'done'
  | 'cancelled'
  | 'failed';

export type ScanStatus = 'running' | 'done' | 'cancelled' | 'failed';
export type MarkDecision = 'undecided' | 'keep' | 'remove';
export type MatchKind = 'exact' | 'perceptual' | 'audio' | 'archiveInternal' | 'rawJpegPair';
export type DeleteMode = 'trash' | 'hardlink' | 'moveTo';
export type ExportFormat = 'csv' | 'html' | 'xml' | 'json';

export interface ScanFilter {
  includeExtensions: string[];
  excludeExtensions: string[];
  minSize: number;
  maxSize: number | null;
  modifiedAfter: number | null;
  modifiedBefore: number | null;
  includeHidden: boolean;
  includeSystem: boolean;
  followSymlinks: boolean;
  excludeGlobs: string[];
  minDepth: number;
}

export interface ScanOptions {
  roots: string[];
  filter: ScanFilter;
  algorithm: HashAlgorithm;
  useCache: boolean;
  detectPerceptual: boolean;
  perceptualThreshold: number;
  audioFingerprint: boolean;
  audioThreshold: number;
  scanArchives: boolean;
  archiveMaxEntryBytes: number;
  rawJpegPairing: boolean;
  checkReferences: boolean;
  protectSystemDirs: boolean;
  minGroupSize: number;
  threads: number;
}

export interface FileEntry {
  path: string;
  root: string;
  size: number;
  modified: number;
  created: number;
  extension: string;
  inode: number;
  device: number;
  hardLinks: number;
  isSymlink: boolean;
  depth: number;
  relativePath: string;
}

export interface DuplicateFile {
  entry: FileEntry;
  hash: string | null;
  fingerprint: string | null;
  decision: MarkDecision;
  reasons: string[];
  protected: string | null;
  isPreferred: boolean;
  archiveInnerPath: string | null;
  isSymlink: boolean;
}

export interface DuplicateGroup {
  id: string;
  kind: MatchKind;
  kindLabel: string;
  signature: string;
  title: string;
  files: DuplicateFile[];
  wastedBytes: number;
  totalBytes: number;
  similarity: number | null;
  informational: boolean;
}

export interface ScanStats {
  filesSeen: number;
  filesScanned: number;
  filesSkipped: number;
  bytesScanned: number;
  cacheHits: number;
  cacheMisses: number;
  partialHashed: number;
  fullHashed: number;
  groups: number;
  duplicateFiles: number;
  wastedBytes: number;
  protectedFiles: number;
  archiveEntries: number;
  referencedFiles: number;
  durationMs: number;
  protectedDirsSkipped: number;
  inaccessiblePaths: number;
}

export interface ScanResult {
  scanId: string;
  roots: string[];
  algorithm: HashAlgorithm;
  options: ScanOptions;
  groups: DuplicateGroup[];
  stats: ScanStats;
  createdAt: number;
}

export interface ScanProgress {
  scanId: string;
  phase: ScanPhase;
  phaseLabel: string;
  filesSeen: number;
  filesTotal: number;
  bytesProcessed: number;
  groupsFound: number;
  cacheHits: number;
  currentPath: string;
  elapsedMs: number;
  percent: number;
  message: string;
}

export interface ScanStatusPayload {
  scanId: string;
  status: ScanStatus;
  progress: ScanProgress;
  error: string | null;
  hasResult: boolean;
}

export interface ActionTarget {
  path: string;
  keepPath: string | null;
  groupId: string;
  size: number;
  archiveInnerPath: string | null;
}

export interface ActionPlan {
  mode: DeleteMode;
  moveDir: string | null;
  dryRun: boolean;
  targets: ActionTarget[];
  confirmed: boolean;
}

export interface ActionOutcome {
  path: string;
  success: boolean;
  error: string | null;
  freedBytes: number;
  movedTo: string | null;
}

export interface ActionResult {
  journalId: string | null;
  mode: DeleteMode;
  dryRun: boolean;
  outcomes: ActionOutcome[];
  succeeded: number;
  failed: number;
  freedBytes: number;
}

export interface PlanPreview {
  count: number;
  bytes: number;
  human: string;
  modeLabel: string;
  modeDescription: string;
  blocked: string[];
}

export type MarkRule =
  | { type: 'keepNewest' }
  | { type: 'keepOldest' }
  | { type: 'keepOldestCreated' }
  | { type: 'keepShortestPath' }
  | { type: 'keepLongestName' }
  | { type: 'keepShallowest' }
  | { type: 'keepLargest' }
  | { type: 'keepMostLinked' }
  | { type: 'keepInPath'; pattern: string }
  | { type: 'preferExtension'; extensions: string[] };

export interface MarkRuleEntry {
  rule: MarkRule;
  enabled: boolean;
}

export interface AutoMarkConfig {
  rules: MarkRuleEntry[];
  keepPerGroup: number;
  neverDeleteProtected: boolean;
  alsoMarkSimilar: boolean;
}

export interface SystemInfo {
  platform: string;
  arch: string;
  logicalCores: number;
  effectiveThreads: number;
  dataDir: string;
  dbPath: string;
  quarantineDir: string;
  exportDir: string;
  protectSystemDirs: boolean;
  cacheEntries: number;
  cacheTtlDays: number;
}

export interface CacheStats {
  entries: number;
  bytes: number;
}

export interface TextDiffPreview {
  leftPath: string;
  rightPath: string;
  identical: boolean;
  identicalPrefixBytes: number;
  firstDifferenceOffset: number | null;
  leftSnippet: string;
  rightSnippet: string;
  leftTruncated: boolean;
  rightTruncated: boolean;
  binary: boolean;
}

export interface ThumbnailPreview {
  path: string;
  dataUrl: string;
  width: number;
  height: number;
  error: string | null;
}

export interface AudioPreview {
  path: string;
  durationSecs: number;
  bands: number[];
  error: string | null;
}

export interface ExportSummary {
  path: string;
  format: ExportFormat;
  rows: number;
  bytes: number;
}

export interface SessionMeta {
  id: string;
  name: string;
  createdAt: number;
  updatedAt: number;
  roots: string[];
  groupCount: number;
  wastedBytes: number;
  filePath: string | null;
}

export interface UndoResult {
  restored: number;
  errors: string[];
  hint: string;
}

export interface ApiError {
  code: string;
  message: string;
}
