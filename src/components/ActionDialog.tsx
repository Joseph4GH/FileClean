import { useEffect, useMemo, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

import { api, humanize } from '../api';
import { humanBytes } from '../format';
import { useI18n, type DictKey } from '../i18n';
import type { ActionResult, DeleteMode, DuplicateGroup, PlanPreview } from '../types';
import { Icon } from './Icon';

interface Props {
  scanId: string;
  groups: DuplicateGroup[];
  onClose: () => void;
  onDone: (result: ActionResult) => void;
  onNotify: (kind: 'info' | 'success' | 'error', message: string) => void;
}

const MODE_VALUES: DeleteMode[] = ['trash', 'hardlink', 'moveTo'];

export function ActionDialog({ scanId, groups, onClose, onDone, onNotify }: Props) {
  const { t } = useI18n();
  const [mode, setMode] = useState<DeleteMode>('trash');
  const [moveDir, setMoveDir] = useState<string>('');
  const [dryRun, setDryRun] = useState(true);
  const [preview, setPreview] = useState<PlanPreview | null>(null);
  const [executing, setExecuting] = useState(false);
  // 真实执行前的显式确认。后端的 `confirmed` 是硬门槛，必须由用户的一个
  // 具体动作产生，不能由「演练开关被关掉」这件事隐含推导出来。
  const [acknowledged, setAcknowledged] = useState(false);

  const targets = useMemo(
    () =>
      groups.flatMap((group) => {
        const keep = group.files.find((f) => f.decision === 'keep');
        return group.files
          .filter((f) => f.decision === 'remove' && !f.protected && !f.isSymlink)
          .map((f) => ({
            path: f.entry.path,
            keepPath: keep?.entry.path ?? null,
            groupId: group.id,
            size: f.entry.size,
            archiveInnerPath: f.archiveInnerPath,
          }));
      }),
    [groups],
  );

  const archiveTargets = targets.filter((t) => t.archiveInnerPath);
  const fileTargets = targets.filter((t) => !t.archiveInnerPath);
  const totalBytes = fileTargets.reduce((sum, t) => sum + t.size, 0);

  useEffect(() => {
    if (fileTargets.length === 0) {
      setPreview(null);
      return;
    }
    api
      .previewPlan({ mode, moveDir: moveDir || null, dryRun, targets: fileTargets, confirmed: false })
      .then(setPreview)
      .catch(() => setPreview(null));
  }, [mode, moveDir, dryRun, fileTargets.length]);

  const pickMoveDir = async () => {
    const picked = await open({ directory: true, multiple: false, title: t('dialog.pickQuarantine') });
    if (typeof picked === 'string') setMoveDir(picked);
  };

  /**
   * 执行清理。
   *
   * `dryRun` 与 `confirmed` 是**两个独立语义**，别混用：
   * - `dryRun`：要不要真的落盘；
   * - `confirmed`：用户是否已明确确认（后端的硬门槛）。
   */
  const executePlan = async (opts: { dryRun: boolean; confirmed: boolean }) => {
    if (fileTargets.length === 0) {
      onNotify('info', t('msg.noFileTargets'));
      return;
    }
    if (opts.confirmed && !acknowledged) return;
    setExecuting(true);
    try {
      const result = await api.executeActions(scanId, {
        mode,
        moveDir: mode === 'moveTo' ? moveDir || null : null,
        dryRun: opts.dryRun,
        targets: fileTargets,
        confirmed: opts.confirmed,
      });
      onDone(result);
      onNotify(
        result.dryRun ? 'info' : result.failed > 0 ? 'error' : 'success',
        result.dryRun
          ? t('msg.dryDone', { n: result.succeeded, size: humanBytes(result.freedBytes) })
          : t('msg.realDone', {
              n: result.succeeded,
              m: result.failed,
              size: humanBytes(result.freedBytes),
            }),
      );
      if (!result.dryRun) onClose();
    } catch (e) {
      onNotify('error', humanize(e));
    } finally {
      setExecuting(false);
    }
  };

  // 两个分支的标志位都写全，调用点一眼能看出传了什么。
  const run = () =>
    dryRun
      ? executePlan({ dryRun: true, confirmed: false })
      : executePlan({ dryRun: false, confirmed: true });

  const modeLabelKey = (m: DeleteMode) => `mode.${m}.label` as DictKey;
  const modeDescKey = (m: DeleteMode) => `mode.${m}.desc` as DictKey;
  const confirmModeKey: DictKey =
    mode === 'trash'
      ? 'dialog.confirmModeTrash'
      : mode === 'hardlink'
        ? 'dialog.confirmModeHardlink'
        : 'dialog.confirmModeMoveTo';

  return (
    <div className="modal-mask" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal">
        <header>{t('dialog.title')}</header>
        <div className="content">
          <div className="alert danger">
            <strong>{t('dialog.warn')}</strong>
            <div style={{ marginTop: 4 }}>
              {t('dialog.warnDetail', { n: fileTargets.length, size: humanBytes(totalBytes) })}
            </div>
          </div>

          <div className="section" style={{ marginTop: 14 }}>
            <h3>{t('dialog.method')}</h3>
            {MODE_VALUES.map((m) => (
              <label className="check" key={m}>
                <input
                  type="radio"
                  name="mode"
                  checked={mode === m}
                  onChange={() => setMode(m)}
                />
                <span>
                  {t(modeLabelKey(m))}
                  <small>{t(modeDescKey(m))}</small>
                </span>
              </label>
            ))}
          </div>

          {mode === 'moveTo' && (
            <label className="field">
              <span>{t('dialog.quarantineDir')}</span>
              <div className="row">
                <input
                  type="text"
                  value={moveDir}
                  onChange={(e) => setMoveDir(e.target.value)}
                  placeholder={t('dialog.quarantinePh')}
                />
                <button className="btn" onClick={pickMoveDir}>
                  <Icon name="folder" size={13} /> {t('dialog.pick')}
                </button>
              </div>
            </label>
          )}

          {mode === 'hardlink' && (
            <div className="alert warn">
              {t('dialog.hardlinkNote')}
            </div>
          )}

          <label className="check" style={{ marginTop: 10 }}>
            <input
              type="checkbox"
              checked={dryRun}
              onChange={(e) => {
                setDryRun(e.target.checked);
                // 切回演练时撤销已确认状态，避免下次真实执行时「仍然处于已确认」。
                if (e.target.checked) setAcknowledged(false);
              }}
            />
            <span>
              {t('dialog.dryRun')}
              <small>{t('dialog.dryRunHint')}</small>
            </span>
          </label>

          {preview && preview.blocked.length > 0 && (
            <div className="alert warn" style={{ marginTop: 10 }}>
              <div>{t('dialog.blocked', { n: preview.blocked.length })}</div>
              <ul style={{ margin: '6px 0 0', paddingLeft: 18 }}>
                {preview.blocked.slice(0, 5).map((b, i) => (
                  <li key={i}>{b}</li>
                ))}
              </ul>
              {preview.blocked.length > 5 && (
                <div className="hint">{t('dialog.blockedMore', { n: preview.blocked.length - 5 })}</div>
              )}
            </div>
          )}

          <div className="alert info" style={{ marginTop: 10 }}>
            <div className="kv">
              <span className="k">{t('dialog.method')}</span>
              <span className="v">{t(modeLabelKey(mode))}</span>
              <span className="k">{t('kv.releasable')}</span>
              <span className="v">{preview ? preview.human : humanBytes(totalBytes)}</span>
              <span className="k">{t('kv.executable')}</span>
              <span className="v">
                {preview ? preview.count : fileTargets.length} / {fileTargets.length}
              </span>
              {archiveTargets.length > 0 && (
                <>
                  <span className="k">{t('kv.archiveEntries')}</span>
                  <span className="v">{t('dialog.archiveEntriesVal', { n: archiveTargets.length })}</span>
                </>
              )}
            </div>
          </div>

          {mode === 'trash' && (
            <div className="hint" style={{ marginTop: 8 }}>
              {t('dialog.trashHint')}
            </div>
          )}

          {/* 确认区放在最后：用户必须先看过上面的清单与汇总，再确认。 */}
          {!dryRun && (
            <div className="alert danger" style={{ marginTop: 10 }}>
              <strong>{t('dialog.confirm', { n: preview ? preview.count : fileTargets.length })}</strong>
              <div style={{ marginTop: 4 }}>
                {t(confirmModeKey, { dir: moveDir || t('dialog.defaultQuarantine') })}
              </div>
              <label className="check" style={{ marginTop: 8, marginBottom: 0 }}>
                <input
                  type="checkbox"
                  checked={acknowledged}
                  onChange={(e) => setAcknowledged(e.target.checked)}
                />
                <span>
                  {t('dialog.ack')}
                  <small>{t('dialog.ackHint')}</small>
                </span>
              </label>
            </div>
          )}
        </div>

        <footer>
          <button className="btn" onClick={onClose} disabled={executing}>
            {t('common.cancel')}
          </button>
          <button
            className={dryRun ? 'btn primary' : 'btn danger'}
            onClick={run}
            disabled={executing || fileTargets.length === 0 || (!dryRun && !acknowledged)}
            title={!dryRun && !acknowledged ? t('dialog.ackTitle') : undefined}
          >
            {executing ? t('common.processing') : dryRun ? t('dialog.runDry') : t('dialog.runReal')}
          </button>
        </footer>
      </div>
    </div>
  );
}
