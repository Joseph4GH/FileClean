import { useCallback, useEffect, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';

import { api, humanize } from '../api';
import { formatTime, humanBytes, num } from '../format';
import type { ScanResult, SessionMeta } from '../types';
import { Icon } from './Icon';

interface Props {
  scanId: string | null;
  hasResult: boolean;
  onLoaded: (result: ScanResult) => void;
  onNotify: (kind: 'info' | 'success' | 'error', message: string) => void;
}

export function SessionPanel({ scanId, hasResult, onLoaded, onNotify }: Props) {
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setSessions(await api.listSessions());
    } catch (e) {
      onNotify('error', humanize(e));
    }
  }, [onNotify]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const saveCurrent = async () => {
    if (!scanId || !hasResult) return;
    setBusy(true);
    try {
      const meta = await api.saveSession(scanId);
      onNotify('success', `会话已保存：${meta.name}`);
      await refresh();
    } catch (e) {
      onNotify('error', humanize(e));
    } finally {
      setBusy(false);
    }
  };

  const exportFile = async () => {
    if (!scanId || !hasResult) return;
    setBusy(true);
    try {
      const target = await save({
        title: '导出会话文件',
        defaultPath: `fileclean-session-${Date.now()}.json`,
        filters: [{ name: 'FileClean 会话', extensions: ['json'] }],
      });
      if (!target) return;
      const path = await api.exportSessionFile(scanId, target);
      onNotify('success', `会话已导出：${path}`);
    } catch (e) {
      onNotify('error', humanize(e));
    } finally {
      setBusy(false);
    }
  };

  const importFile = async () => {
    setBusy(true);
    try {
      const picked = await open({
        title: '导入会话文件',
        multiple: false,
        filters: [{ name: 'FileClean 会话', extensions: ['json'] }],
      });
      if (typeof picked !== 'string') return;
      const result = await api.importSessionFile(picked);
      onLoaded(result);
      onNotify('success', '会话已导入（清理前需重新确认）');
    } catch (e) {
      onNotify('error', humanize(e));
    } finally {
      setBusy(false);
    }
  };

  const load = async (id: string) => {
    setBusy(true);
    try {
      const result = await api.loadSession(id);
      onLoaded(result);
      onNotify('success', '会话已载入（清理前需重新确认）');
    } catch (e) {
      onNotify('error', humanize(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (id: string) => {
    try {
      await api.deleteSession(id);
      await refresh();
    } catch (e) {
      onNotify('error', humanize(e));
    }
  };

  return (
    <div className="settings-form">
      <div className="alert info">
        <div className="row" style={{ gap: 6, marginBottom: 4 }}>
          <Icon name="save" size={13} />
          <span>会话保存的是扫描结果与决策，不含文件内容</span>
        </div>
        <small className="hint">
          重新载入或导入的会话，所有「待清理」标记会被重置为「未决策」——避免隔了一段时间后
          直接沿用旧判断动手清理。
        </small>
      </div>

      <div className="btn-row" style={{ marginBottom: 8 }}>
        <button className="btn sm" onClick={saveCurrent} disabled={busy || !hasResult}>
          <Icon name="save" size={13} /> 保存当前会话
        </button>
        <button className="btn sm" onClick={exportFile} disabled={busy || !hasResult}>
          <Icon name="export" size={13} /> 导出为文件
        </button>
        <button className="btn sm" onClick={importFile} disabled={busy}>
          导入会话文件
        </button>
        <button className="btn ghost sm" onClick={refresh} disabled={busy} title="刷新列表">
          <Icon name="refresh" size={13} /> 刷新
        </button>
      </div>

      <div className="divider" />

      <div className="field">
        <span>已保存的会话（{sessions.length}）</span>
        {sessions.length === 0 ? (
          <div className="hint">
            暂无保存的会话。扫描完成后点「保存当前会话」，下次打开可以直接恢复本次结果继续操作。
          </div>
        ) : (
          <div className="sessions-list">
            {sessions.map((s) => (
              <div className="session-item" key={s.id}>
                <div className="info">
                  <div title={s.name}>{s.name}</div>
                  <div>
                    {num(s.groupCount)} 组 · 可回收 {humanBytes(s.wastedBytes)} ·{' '}
                    {formatTime(s.updatedAt)}
                  </div>
                </div>
                <button
                  className="btn ghost sm"
                  onClick={() => load(s.id)}
                  disabled={busy}
                  title="载入"
                >
                  <Icon name="eye" size={13} />
                </button>
                <button
                  className="btn ghost sm"
                  onClick={() => remove(s.id)}
                  disabled={busy}
                  title="删除"
                >
                  <Icon name="trash" size={13} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
