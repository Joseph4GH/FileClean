import { useCallback, useEffect, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';

import { api, humanize } from '../api';
import { formatTime, humanBytes, num } from '../format';
import { useI18n } from '../i18n';
import type { ScanResult, SessionMeta } from '../types';
import { Icon } from './Icon';

interface Props {
  scanId: string | null;
  hasResult: boolean;
  onLoaded: (result: ScanResult) => void;
  onNotify: (kind: 'info' | 'success' | 'error', message: string) => void;
}

export function SessionPanel({ scanId, hasResult, onLoaded, onNotify }: Props) {
  const { t } = useI18n();
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
      onNotify('success', t('msg.sessionSaved', { name: meta.name }));
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
        title: t('session.exportTitle'),
        defaultPath: `fileclean-session-${Date.now()}.json`,
        filters: [{ name: t('session.filterName'), extensions: ['json'] }],
      });
      if (!target) return;
      const path = await api.exportSessionFile(scanId, target);
      onNotify('success', t('msg.sessionExported', { path }));
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
        title: t('session.importTitle'),
        multiple: false,
        filters: [{ name: t('session.filterName'), extensions: ['json'] }],
      });
      if (typeof picked !== 'string') return;
      const result = await api.importSessionFile(picked);
      onLoaded(result);
      onNotify('success', t('msg.sessionImported'));
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
      onNotify('success', t('msg.sessionLoaded'));
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
          <span>{t('session.info')}</span>
        </div>
        <small className="hint">{t('session.infoHint')}</small>
      </div>

      <div className="btn-row" style={{ marginBottom: 8 }}>
        <button className="btn sm" onClick={saveCurrent} disabled={busy || !hasResult}>
          <Icon name="save" size={13} /> {t('session.save')}
        </button>
        <button className="btn sm" onClick={exportFile} disabled={busy || !hasResult}>
          <Icon name="export" size={13} /> {t('session.export')}
        </button>
        <button className="btn sm" onClick={importFile} disabled={busy}>
          {t('session.import')}
        </button>
        <button className="btn ghost sm" onClick={refresh} disabled={busy} title={t('session.refreshTitle')}>
          <Icon name="refresh" size={13} /> {t('common.refresh')}
        </button>
      </div>

      <div className="divider" />

      <div className="field">
        <span>{t('session.listTitle', { n: sessions.length })}</span>
        {sessions.length === 0 ? (
          <div className="hint">{t('session.empty')}</div>
        ) : (
          <div className="sessions-list">
            {sessions.map((s) => (
              <div className="session-item" key={s.id}>
                <div className="info">
                  <div title={s.name}>{s.name}</div>
                  <div>
                    {t('session.row', {
                      groups: num(s.groupCount),
                      size: humanBytes(s.wastedBytes),
                      time: formatTime(s.updatedAt),
                    })}
                  </div>
                </div>
                <button
                  className="btn ghost sm"
                  onClick={() => load(s.id)}
                  disabled={busy}
                  title={t('common.load')}
                >
                  <Icon name="eye" size={13} />
                </button>
                <button
                  className="btn ghost sm"
                  onClick={() => remove(s.id)}
                  disabled={busy}
                  title={t('common.delete')}
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
