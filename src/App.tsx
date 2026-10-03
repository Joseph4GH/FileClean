import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { save } from '@tauri-apps/plugin-dialog';

import { api, humanize, onProgress, onScanDone, onScanError } from './api';
import { formatDuration, humanBytes, num, shortPath } from './format';
import type {
  ActionResult,
  AutoMarkConfig,
  DuplicateGroup,
  ExportFormat,
  MarkDecision,
  MatchKind,
  ScanOptions,
  ScanProgress,
  ScanResult,
  ScanStatus,
  SystemInfo,
} from './types';
import { ActionDialog } from './components/ActionDialog';
import { GroupDetail } from './components/GroupDetail';
import { GroupList } from './components/GroupList';
import { Icon } from './components/Icon';
import { SessionPanel } from './components/SessionPanel';
import { SettingsPage } from './components/SettingsPage';
import { SetupPanel } from './components/SetupPanel';
import { Toasts, useToasts } from './components/Toasts';

type SortKey = 'waste' | 'count' | 'name';
type View = 'scan' | 'settings';

export default function App() {
  const { toasts, dismiss, info, success, error } = useToasts();

  const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null);
  const [options, setOptions] = useState<ScanOptions | null>(null);
  const [markConfig, setMarkConfig] = useState<AutoMarkConfig | null>(null);

  const [scanId, setScanId] = useState<string | null>(null);
  const [status, setStatus] = useState<ScanStatus | null>(null);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [result, setResult] = useState<ScanResult | null>(null);

  const [selectedGroupId, setSelectedGroupId] = useState<string | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);

  const [query, setQuery] = useState('');
  const [kindFilter, setKindFilter] = useState<MatchKind | ''>('');
  const [decisionFilter, setDecisionFilter] = useState<'all' | 'remove' | 'protected'>('all');
  const [sortKey, setSortKey] = useState<SortKey>('waste');

  const [showAction, setShowAction] = useState(false);
  const [exportFormat, setExportFormat] = useState<ExportFormat>('html');
  const [lastJournal, setLastJournal] = useState<string | null>(null);

  /** 主界面 = 扫描工作台；设置页承载全部扫描规则。 */
  const [view, setView] = useState<View>('scan');

  const scanIdRef = useRef<string | null>(null);
  scanIdRef.current = scanId;

  // ---------------- 初始化 ----------------
  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const [info_, opts, marks] = await Promise.all([
          api.systemInfo(),
          api.defaultOptions(),
          api.defaultMarkConfig(),
        ]);
        if (!alive) return;
        setSystemInfo(info_);
        setOptions(opts);
        setMarkConfig(marks);
      } catch (e) {
        error(`初始化失败：${humanize(e)}`);
      }
    })();
    return () => {
      alive = false;
    };
    // 只在首次挂载执行。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------------- 事件订阅 ----------------
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let disposed = false;

    (async () => {
      const offProgress = await onProgress((p) => {
        if (p.scanId !== scanIdRef.current) return;
        setProgress(p);
      });
      const offDone = await onScanDone(async (payload) => {
        if (payload.scanId !== scanIdRef.current) return;
        setStatus('done');
        try {
          const full = await api.getScanResult(payload.scanId);
          setResult(full);
          setSelectedGroupId(full.groups[0]?.id ?? null);
          setSelectedPath(full.groups[0]?.files[0]?.entry.path ?? null);
          if (full.groups.length === 0) {
            info('扫描完成：没有发现重复文件');
          } else {
            success(
              `扫描完成：${num(full.stats.groups)} 个分组，可回收 ${humanBytes(
                full.stats.wastedBytes,
              )}`,
            );
          }
        } catch (e) {
          error(humanize(e));
        }
      });
      const offError = await onScanError((payload) => {
        if (payload.scanId !== scanIdRef.current) return;
        setStatus('failed');
        error(payload.message);
      });

      if (disposed) {
        offProgress();
        offDone();
        offError();
      } else {
        unlisteners.push(offProgress, offDone, offError);
      }
    })();

    return () => {
      disposed = true;
      unlisteners.forEach((off) => off());
    };
  }, [info, success, error]);

  // ---------------- 操作 ----------------
  const startScan = useCallback(async () => {
    if (!options || !markConfig) return;
    if (options.roots.length === 0) {
      error('请先添加至少一个扫描目录');
      return;
    }
    try {
      setResult(null);
      setSelectedGroupId(null);
      setSelectedPath(null);
      setStatus('running');
      setProgress(null);
      const id = await api.startScan(options, markConfig);
      setScanId(id);
      scanIdRef.current = id;
      info('扫描已启动');
    } catch (e) {
      setStatus('failed');
      error(humanize(e));
    }
  }, [options, markConfig, error, info]);

  const cancelScan = useCallback(async () => {
    if (!scanId) return;
    try {
      await api.cancelScan(scanId);
      setStatus('cancelled');
      info('已请求取消扫描');
    } catch (e) {
      error(humanize(e));
    }
  }, [scanId, error, info]);

  const applyResult = useCallback((next: ScanResult) => {
    setResult(next);
    setSelectedGroupId((current) => {
      if (current && next.groups.some((g) => g.id === current)) return current;
      return next.groups[0]?.id ?? null;
    });
  }, []);

  const setFileDecision = useCallback(
    async (groupId: string, path: string, decision: MarkDecision) => {
      if (!scanId) return;
      try {
        applyResult(await api.setFileDecision(scanId, groupId, path, decision));
      } catch (e) {
        error(humanize(e));
      }
    },
    [scanId, applyResult, error],
  );

  const setGroupDecision = useCallback(
    async (groupId: string, decision: MarkDecision) => {
      if (!scanId) return;
      try {
        applyResult(await api.setGroupDecision(scanId, groupId, decision));
      } catch (e) {
        error(humanize(e));
      }
    },
    [scanId, applyResult, error],
  );

  const invert = useCallback(
    async (groupId: string | null) => {
      if (!scanId) return;
      try {
        applyResult(await api.invertGroup(scanId, groupId));
        info('已执行反向选择');
      } catch (e) {
        error(humanize(e));
      }
    },
    [scanId, applyResult, error, info],
  );

  const reveal = useCallback(
    async (path: string) => {
      try {
        await api.revealInFinder(path);
      } catch (e) {
        error(humanize(e));
      }
    },
    [error],
  );

  const exportReport = useCallback(async () => {
    if (!scanId) return;
    try {
      const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-');
      const target = await save({
        title: '导出重复文件报告',
        defaultPath: `fileclean-report-${stamp}.${exportFormat}`,
        filters: [{ name: exportFormat.toUpperCase(), extensions: [exportFormat] }],
      });
      // 用户在系统弹窗里取消：静默返回，不当成错误。
      if (!target) return;

      const summary = await api.exportReport(scanId, exportFormat, target);
      success(`报告已导出：${summary.path}`);
      await api.revealInFinder(summary.path);
    } catch (e) {
      error(humanize(e));
    }
  }, [scanId, exportFormat, success, error]);

  const removeArchiveEntries = useCallback(
    async (group: DuplicateGroup) => {
      const paths = group.files
        .filter((f) => f.decision === 'remove' && f.archiveInnerPath)
        .map((f) => f.archiveInnerPath as string);
      if (paths.length === 0) {
        info('没有标记为待清理的压缩包条目');
        return;
      }
      const archivePath = group.files[0]?.entry.path;
      if (!archivePath) return;
      try {
        const removed = await api.removeArchiveEntries(archivePath, paths, true);
        success(`已从压缩包中移除 ${removed} 个条目`);
      } catch (e) {
        error(humanize(e));
      }
    },
    [info, success, error],
  );

  const undo = useCallback(async () => {
    if (!lastJournal) return;
    try {
      const result = await api.undoAction(lastJournal);
      success(result.hint);
      if (result.errors.length > 0) info(result.errors[0]);
      setLastJournal(null);
    } catch (e) {
      error(humanize(e));
    }
  }, [lastJournal, success, info, error]);

  const clearCache = useCallback(async () => {
    try {
      const removed = await api.clearCache();
      success(`已清空 ${num(removed)} 条增量缓存`);
      setSystemInfo(await api.systemInfo());
    } catch (e) {
      error(humanize(e));
    }
  }, [success, error]);

  const openExportDir = useCallback(async () => {
    if (!systemInfo) return;
    await reveal(systemInfo.exportDir);
  }, [systemInfo, reveal]);

  /** 子组件统一走这个回调上报，避免每个调用点重复写三路分支。 */
  const notify = useCallback(
    (kind: 'info' | 'success' | 'error', message: string) => {
      if (kind === 'success') success(message);
      else if (kind === 'error') error(message);
      else info(message);
    },
    [success, error, info],
  );

  // ---------------- 派生数据 ----------------
  const filteredGroups = useMemo(() => {
    if (!result) return [];
    const needle = query.trim().toLowerCase();

    const list = result.groups.filter((group) => {
      if (kindFilter && group.kind !== kindFilter) return false;

      if (decisionFilter === 'remove' && !group.files.some((f) => f.decision === 'remove')) {
        return false;
      }
      if (decisionFilter === 'protected' && !group.files.some((f) => f.protected)) return false;

      if (!needle) return true;
      return group.files.some(
        (f) =>
          f.entry.path.toLowerCase().includes(needle) ||
          f.entry.relativePath.toLowerCase().includes(needle) ||
          (f.archiveInnerPath ?? '').toLowerCase().includes(needle),
      );
    });

    const sorted = [...list];
    if (sortKey === 'waste') sorted.sort((a, b) => b.wastedBytes - a.wastedBytes);
    else if (sortKey === 'count') sorted.sort((a, b) => b.files.length - a.files.length);
    else sorted.sort((a, b) => a.title.localeCompare(b.title, 'zh'));
    return sorted;
  }, [result, query, kindFilter, decisionFilter, sortKey]);

  const selectedGroup = useMemo(
    () => filteredGroups.find((g) => g.id === selectedGroupId) ?? null,
    [filteredGroups, selectedGroupId],
  );

  const dirtyGroups = useMemo(
    () => (result ? result.groups.filter((g) => g.files.some((f) => f.decision === 'remove')) : []),
    [result],
  );

  const running = status === 'running';
  const stats = result?.stats;

  const onActionResult = (actionResult: ActionResult) => {
    setLastJournal(actionResult.journalId);
  };

  return (
    <div className="app">
      <div className="topbar">
        <div className="brand">
          <span className="dot" />
          FileClean
          <small>重复文件扫描与安全清理</small>
        </div>

        <nav className="seg">
          <button
            className={`seg-item${view === 'scan' ? ' active' : ''}`}
            onClick={() => setView('scan')}
          >
            <Icon name="search" size={13} /> 扫描
          </button>
          <button
            className={`seg-item${view === 'settings' ? ' active' : ''}`}
            onClick={() => setView('settings')}
          >
            <Icon name="settings" size={13} /> 扫描规则
          </button>
        </nav>

        <div className="spacer" />

        {systemInfo && view === 'scan' && (
          <span className="hint">
            {systemInfo.logicalCores} 核 · 缓存 {num(systemInfo.cacheEntries)} 条
          </span>
        )}

        {view === 'scan' && lastJournal && (
          <button className="btn sm" onClick={undo}>
            <Icon name="undo" size={13} /> 撤销上次隔离
          </button>
        )}

        {view === 'scan' && (
          <>
            <button
              className="btn sm"
              onClick={exportReport}
              disabled={!result || result.groups.length === 0}
              title="导出 CSV / HTML / XML / JSON 报告"
            >
              <Icon name="export" size={13} /> 导出报告
            </button>
            <select
              value={exportFormat}
              onChange={(e) => setExportFormat(e.target.value as ExportFormat)}
              style={{ width: 82 }}
              disabled={!result}
            >
              <option value="html">HTML</option>
              <option value="csv">CSV</option>
              <option value="xml">XML</option>
              <option value="json">JSON</option>
            </select>

            <button
              className="btn danger sm"
              onClick={() => setShowAction(true)}
              disabled={!result || dirtyGroups.length === 0 || running}
              title={dirtyGroups.length === 0 ? '没有待清理的文件' : '执行清理'}
            >
              <Icon name="trash" size={13} /> 执行清理
            </button>
          </>
        )}
      </div>

      {view === 'settings' ? (
        <div className="body">
          {options && markConfig ? (
            <SettingsPage
              options={options}
              onOptions={setOptions}
              markConfig={markConfig}
              onMarkConfig={setMarkConfig}
              systemInfo={systemInfo}
              running={running}
              onPickSaveDir={openExportDir}
              onClearCache={clearCache}
              sessionSlot={
                <SessionPanel
                  scanId={scanId}
                  hasResult={!!result}
                  onLoaded={applyResult}
                  onNotify={notify}
                />
              }
            />
          ) : (
            <div className="sidebar hint">正在加载配置…</div>
          )}
        </div>
      ) : (
        <div className="body">
          {options && markConfig ? (
            <SetupPanel
              options={options}
              onOptions={setOptions}
              markConfig={markConfig}
              running={running}
              systemInfo={systemInfo}
              onStart={startScan}
              onCancel={cancelScan}
              onOpenSettings={() => setView('settings')}
            />
          ) : (
            <div className="sidebar hint">正在加载配置…</div>
          )}

          <div className="main">
            {stats && (
              <div className="statsbar">
                <div className="stat">
                  <div className="k">可回收空间</div>
                  <div className="v danger">{humanBytes(stats.wastedBytes)}</div>
                </div>
                <div className="stat">
                  <div className="k">重复分组</div>
                  <div className="v">{num(stats.groups)}</div>
                </div>
                <div className="stat">
                  <div className="k">待清理文件</div>
                  <div className="v">{num(stats.duplicateFiles)}</div>
                </div>
                <div className="stat">
                  <div className="k">受保护</div>
                  <div className="v ok">{num(stats.protectedFiles)}</div>
                </div>
                <div className="stat">
                  <div className="k">扫描文件</div>
                  <div className="v">{num(stats.filesSeen)}</div>
                </div>
                <div className="stat">
                  <div className="k">参与哈希</div>
                  <div className="v">{num(stats.filesScanned)}</div>
                </div>
                <div className="stat">
                  <div className="k">缓存命中</div>
                  <div className="v">{num(stats.cacheHits)}</div>
                </div>
                <div className="stat">
                  <div className="k">耗时</div>
                  <div className="v">{formatDuration(stats.durationMs)}</div>
                </div>
                {stats.protectedDirsSkipped > 0 && (
                  <div className="stat">
                    <div className="k">保护目录跳过</div>
                    <div className="v">{num(stats.protectedDirsSkipped)}</div>
                  </div>
                )}
              </div>
            )}

            <div className="toolbar">
              <Icon name="search" size={14} />
              <input
                type="text"
                placeholder="按路径 / 文件名过滤…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
              <select
                value={kindFilter}
                onChange={(e) => setKindFilter(e.target.value as MatchKind | '')}
              >
                <option value="">全部类型</option>
                <option value="exact">完全重复</option>
                <option value="perceptual">图片相似</option>
                <option value="audio">音频相似</option>
                <option value="archiveInternal">压缩包内</option>
                <option value="rawJpegPair">RAW/JPEG 配对</option>
              </select>
              <select
                value={decisionFilter}
                onChange={(e) => setDecisionFilter(e.target.value as typeof decisionFilter)}
              >
                <option value="all">全部决策</option>
                <option value="remove">仅待清理</option>
                <option value="protected">仅受保护</option>
              </select>
              <select value={sortKey} onChange={(e) => setSortKey(e.target.value as SortKey)}>
                <option value="waste">按可回收空间</option>
                <option value="count">按文件数</option>
                <option value="name">按文件名</option>
              </select>
              <div className="spacer" />
              <span className="hint">
                {result ? `${num(filteredGroups.length)} / ${num(result.groups.length)} 个分组` : '尚未扫描'}
              </span>
              <button
                className="btn sm"
                onClick={() => invert(null)}
                disabled={!result || running}
                title="全局反向选择"
              >
                反向选择
              </button>
              {result && (
                <button
                  className="btn sm"
                  onClick={async () => {
                    if (!scanId || !markConfig) return;
                    try {
                      applyResult(await api.applyAutoMark(scanId, markConfig));
                      success('已按当前保留策略重新标记');
                    } catch (e) {
                      error(humanize(e));
                    }
                  }}
                  disabled={running}
                >
                  重新应用策略
                </button>
              )}
            </div>

            <div className="split">
              <div className="list-pane">
                <GroupList
                  groups={filteredGroups}
                  selectedId={selectedGroupId}
                  onSelect={(id) => {
                    setSelectedGroupId(id);
                    const g = filteredGroups.find((x) => x.id === id);
                    setSelectedPath(g?.files[0]?.entry.path ?? null);
                  }}
                />
              </div>

              {selectedGroup ? (
                <GroupDetail
                  group={selectedGroup}
                  selectedPath={selectedPath}
                  onSelectPath={setSelectedPath}
                  onSetFileDecision={(path, decision) =>
                    setFileDecision(selectedGroup.id, path, decision)
                  }
                  onSetGroupDecision={(decision) => setGroupDecision(selectedGroup.id, decision)}
                  onInvert={() => invert(selectedGroup.id)}
                  onReveal={reveal}
                  onRemoveArchiveEntries={removeArchiveEntries}
                  onNotify={notify}
                />
              ) : (
                <div className="detail-pane">
                  <div className="empty">
                    <span className="big">
                      <Icon name="filter" size={30} />
                    </span>
                    <div>{result ? '从左侧选择一个分组查看详情' : '还没有扫描结果'}</div>
                    <div className="hint">
                      {result
                        ? '详情面板支持图片对比、文本差异对比与音频频谱预览'
                        : '在左侧选择目录并点击「开始扫描」'}
                    </div>
                    {result?.groups.length === 0 && (
                      <div className="hint">本次扫描未发现重复文件，可以尝试开启感知哈希或放宽过滤条件。</div>
                    )}
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      <div className="progress-wrap">
        <div className="progress-head">
          <span className="phase">
            {status === 'done'
              ? '已完成'
              : status === 'cancelled'
                ? '已取消'
                : status === 'failed'
                  ? '失败'
                  : running
                    ? (progress?.phaseLabel ?? '准备中')
                    : '就绪'}
          </span>
          <span className="msg">{progress?.message ?? '选择目录后开始扫描'}</span>
          <span className="pct">{progress ? `${(progress.percent * 100).toFixed(0)}%` : ''}</span>
        </div>
        <div className="progress-track">
          <div
            className="progress-fill"
            style={{ width: `${Math.round((progress?.percent ?? 0) * 100)}%` }}
          />
        </div>
        <div className="progress-sub">
          {progress?.currentPath ? shortPath(progress.currentPath, 120) : ''}
        </div>
      </div>

      {showAction && result && scanId && (
        <ActionDialog
          scanId={scanId}
          groups={dirtyGroups}
          onClose={() => setShowAction(false)}
          onDone={onActionResult}
          onNotify={notify}
        />
      )}

      <Toasts items={toasts} onDismiss={dismiss} />
    </div>
  );
}
