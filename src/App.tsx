import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { save } from '@tauri-apps/plugin-dialog';

import { api, humanize, onProgress, onScanDone, onScanError } from './api';
import { formatDuration, humanBytes, num, shortPath } from './format';
import { I18nProvider, useI18n, getLocale } from './i18n';
import { ThemeProvider } from './theme';
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

function AppBody() {
  const { toasts, dismiss, info, success, error, t } = useToastsWithI18n();

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

  /** 主界面 = 扫描工作台；设置页承载全部扫描规则与外观偏好。 */
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
        error(t('msg.initFailed', { detail: humanize(e) }));
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
            info(t('empty.noResult'));
          } else {
            success(
              t('msg.scanDone', {
                groups: num(full.stats.groups),
                size: humanBytes(full.stats.wastedBytes),
              }),
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
  }, [info, success, error, t]);

  // ---------------- 操作 ----------------
  const startScan = useCallback(async () => {
    if (!options || !markConfig) return;
    if (options.roots.length === 0) {
      error(t('btn.scanDisabledHint'));
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
      info(t('btn.scanTitle'));
    } catch (e) {
      setStatus('failed');
      error(humanize(e));
    }
  }, [options, markConfig, error, info, t]);

  const cancelScan = useCallback(async () => {
    if (!scanId) return;
    try {
      await api.cancelScan(scanId);
      setStatus('cancelled');
      info(t('progress.cancelled'));
    } catch (e) {
      error(humanize(e));
    }
  }, [scanId, error, info, t]);

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
        info(t('btn.invert'));
      } catch (e) {
        error(humanize(e));
      }
    },
    [scanId, applyResult, error, info, t],
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
        title: t('btn.exportReport'),
        defaultPath: `fileclean-report-${stamp}.${exportFormat}`,
        filters: [{ name: exportFormat.toUpperCase(), extensions: [exportFormat] }],
      });
      // 用户在系统弹窗里取消：静默返回，不当成错误。
      if (!target) return;

      const summary = await api.exportReport(scanId, exportFormat, target);
      success(t('msg.reportExported', { path: summary.path }));
      await api.revealInFinder(summary.path);
    } catch (e) {
      error(humanize(e));
    }
  }, [scanId, exportFormat, success, error, t]);

  const removeArchiveEntries = useCallback(
    async (group: DuplicateGroup) => {
      const paths = group.files
        .filter((f) => f.decision === 'remove' && f.archiveInnerPath)
        .map((f) => f.archiveInnerPath as string);
      if (paths.length === 0) {
        info(t('detail.archiveNote'));
        return;
      }
      const archivePath = group.files[0]?.entry.path;
      if (!archivePath) return;
      try {
        const removed = await api.removeArchiveEntries(archivePath, paths, true);
        success(t('msg.archiveEntriesRemoved', { n: removed }));
      } catch (e) {
        error(humanize(e));
      }
    },
    [info, success, error, t],
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
      success(t('msg.cacheCleared', { n: num(removed) }));
      setSystemInfo(await api.systemInfo());
    } catch (e) {
      error(humanize(e));
    }
  }, [success, error, t]);

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
    else sorted.sort((a, b) => a.title.localeCompare(b.title, getLocale() === 'zh' ? 'zh' : 'en'));
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
          <small>{t('app.subtitle')}</small>
        </div>

        <nav className="seg">
          <button
            className={`seg-item${view === 'scan' ? ' active' : ''}`}
            onClick={() => setView('scan')}
          >
            <Icon name="search" size={13} /> {t('nav.scan')}
          </button>
          <button
            className={`seg-item${view === 'settings' ? ' active' : ''}`}
            onClick={() => setView('settings')}
          >
            <Icon name="settings" size={13} /> {t('nav.settings')}
          </button>
        </nav>

        <div className="spacer" />

        {systemInfo && view === 'scan' && (
          <span className="hint">
            {t('topbar.envHint', {
              cores: systemInfo.logicalCores,
              n: num(systemInfo.cacheEntries),
            })}
          </span>
        )}

        {view === 'scan' && lastJournal && (
          <button className="btn sm" onClick={undo}>
            <Icon name="undo" size={13} /> {t('btn.undo')}
          </button>
        )}

        {view === 'scan' && (
          <>
            <button
              className="btn sm"
              onClick={exportReport}
              disabled={!result || result.groups.length === 0}
              title={t('btn.exportReport')}
            >
              <Icon name="export" size={13} /> {t('btn.exportReport')}
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
              title={dirtyGroups.length === 0 ? t('btn.cleanEmptyHint') : t('btn.cleanTitle')}
            >
              <Icon name="trash" size={13} /> {t('btn.clean')}
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
            <div className="sidebar hint">{t('settings.loadingConfig')}</div>
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
            <div className="sidebar hint">{t('settings.loadingConfig')}</div>
          )}

          <div className="main">
            {stats && (
              <div className="statsbar">
                <div className="stat">
                  <div className="k">{t('stat.kWaste')}</div>
                  <div className="v danger">{humanBytes(stats.wastedBytes)}</div>
                </div>
                <div className="stat">
                  <div className="k">{t('stat.kGroups')}</div>
                  <div className="v">{num(stats.groups)}</div>
                </div>
                <div className="stat">
                  <div className="k">{t('stat.kDupFiles')}</div>
                  <div className="v">{num(stats.duplicateFiles)}</div>
                </div>
                <div className="stat">
                  <div className="k">{t('stat.kProtected')}</div>
                  <div className="v ok">{num(stats.protectedFiles)}</div>
                </div>
                <div className="stat">
                  <div className="k">{t('stat.kFilesSeen')}</div>
                  <div className="v">{num(stats.filesSeen)}</div>
                </div>
                <div className="stat">
                  <div className="k">{t('stat.kFilesScanned')}</div>
                  <div className="v">{num(stats.filesScanned)}</div>
                </div>
                <div className="stat">
                  <div className="k">{t('stat.kCacheHits')}</div>
                  <div className="v">{num(stats.cacheHits)}</div>
                </div>
                <div className="stat">
                  <div className="k">{t('stat.kDuration')}</div>
                  <div className="v">{formatDuration(stats.durationMs)}</div>
                </div>
                {stats.protectedDirsSkipped > 0 && (
                  <div className="stat">
                    <div className="k">{t('stat.kProtectedDirs')}</div>
                    <div className="v">{num(stats.protectedDirsSkipped)}</div>
                  </div>
                )}
              </div>
            )}

            <div className="toolbar">
              <Icon name="search" size={14} />
              <input
                type="text"
                placeholder={t('toolbar.searchPlaceholder')}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
              <select
                value={kindFilter}
                onChange={(e) => setKindFilter(e.target.value as MatchKind | '')}
              >
                <option value="">{t('filter.allTypes')}</option>
                <option value="exact">{t('kind.exact')}</option>
                <option value="perceptual">{t('kind.perceptual')}</option>
                <option value="audio">{t('kind.audio')}</option>
                <option value="archiveInternal">{t('kind.archiveInternal')}</option>
                <option value="rawJpegPair">{t('kind.rawJpegPair')}</option>
              </select>
              <select
                value={decisionFilter}
                onChange={(e) => setDecisionFilter(e.target.value as typeof decisionFilter)}
              >
                <option value="all">{t('filter.allDecisions')}</option>
                <option value="remove">{t('filter.onlyRemove')}</option>
                <option value="protected">{t('filter.onlyProtected')}</option>
              </select>
              <select value={sortKey} onChange={(e) => setSortKey(e.target.value as SortKey)}>
                <option value="waste">{t('sort.waste')}</option>
                <option value="count">{t('sort.count')}</option>
                <option value="name">{t('sort.name')}</option>
              </select>
              <div className="spacer" />
              <span className="hint">
                {result
                  ? t('toolbar.groupCount', {
                      shown: num(filteredGroups.length),
                      total: num(result.groups.length),
                    })
                  : t('toolbar.noScan')}
              </span>
              <button
                className="btn sm"
                onClick={() => invert(null)}
                disabled={!result || running}
                title={t('btn.invertTitle')}
              >
                {t('btn.invert')}
              </button>
              {result && (
                <button
                  className="btn sm"
                  onClick={async () => {
                    if (!scanId || !markConfig) return;
                    try {
                      applyResult(await api.applyAutoMark(scanId, markConfig));
                      success(t('msg.reapplied'));
                    } catch (e) {
                      error(humanize(e));
                    }
                  }}
                  disabled={running}
                >
                  {t('btn.reapply')}
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
                    <div>{result ? t('empty.selectGroup') : t('empty.noResult')}</div>
                    <div className="hint">
                      {result
                        ? t('empty.detailHint')
                        : t('empty.noScanHint')}
                    </div>
                    {result?.groups.length === 0 && (
                      <div className="hint">{t('empty.noDupHint')}</div>
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
              ? t('progress.done')
              : status === 'cancelled'
                ? t('progress.cancelled')
                : status === 'failed'
                  ? t('progress.failed')
                  : running
                    ? (progress?.phaseLabel ?? t('progress.preparing'))
                    : t('progress.ready')}
          </span>
          <span className="msg">
            {progress?.message ?? (running ? '' : t('progress.readyMsg'))}
          </span>
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

export default function App() {
  return (
    <I18nProvider>
      <ThemeProvider>
        <AppBody />
      </ThemeProvider>
    </I18nProvider>
  );
}

// 把 i18n 的 t 接入 useToasts 的返回，避免每个 toast 文案在调用点重复拼接。
function useToastsWithI18n() {
  const base = useToasts();
  const { t } = useI18n();
  return { ...base, t };
}
