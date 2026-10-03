import { useEffect, useMemo, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

import { api, humanize } from '../api';
import { humanBytes } from '../format';
import type { ActionResult, DeleteMode, DuplicateGroup, PlanPreview } from '../types';
import { Icon } from './Icon';

interface Props {
  scanId: string;
  groups: DuplicateGroup[];
  onClose: () => void;
  onDone: (result: ActionResult) => void;
  onNotify: (kind: 'info' | 'success' | 'error', message: string) => void;
}

const MODES: Array<{ value: DeleteMode; label: string; desc: string; safe: boolean }> = [
  {
    value: 'trash',
    label: '移入回收站',
    desc: '默认方式。文件进入系统废纸篓，可从废纸篓手动还原。',
    safe: true,
  },
  {
    value: 'hardlink',
    label: '硬链接替换',
    desc: '保留文件不动，把副本位置替换为指向它的硬链接 —— 省下空间，且原路径引用依然有效（Lightroom / 工程文件不会失联）。',
    safe: true,
  },
  {
    value: 'moveTo',
    label: '移入隔离区',
    desc: '把文件移动到隔离目录（保留原目录结构），方便人工二次审核，且支持一键撤销还原。',
    safe: true,
  },
];

export function ActionDialog({ scanId, groups, onClose, onDone, onNotify }: Props) {
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
    const picked = await open({ directory: true, multiple: false, title: '选择隔离目录' });
    if (typeof picked === 'string') setMoveDir(picked);
  };

  /**
   * 执行清理。
   *
   * `dryRun` 与 `confirmed` 是**两个独立语义**，别混用：
   * - `dryRun`：要不要真的落盘；
   * - `confirmed`：用户是否已明确确认（后端的硬门槛）。
   *
   * 这里刻意收「具名对象」而不是两个位置布尔参数。早先按钮写成
   * `onClick={() => run(dryRun)}`，把 dryRun 当成了 confirmed，而 TypeScript 对
   * 两个 `boolean` 位置参数完全无从区分 —— 演练时 `run(true)` 恰好蒙对（演练本就
   * 不需要确认），真实执行时 `run(false)` 就被后端闸门拒绝，报「缺少显式确认」。
   */
  const executePlan = async (opts: { dryRun: boolean; confirmed: boolean }) => {
    if (fileTargets.length === 0) {
      onNotify('info', '没有可执行的文件目标（压缩包内条目请使用专门的移除操作）');
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
          ? `演练完成：将处理 ${result.succeeded} 个文件，预计释放 ${humanBytes(result.freedBytes)}`
          : `完成：成功 ${result.succeeded} 个，失败 ${result.failed} 个，释放 ${humanBytes(result.freedBytes)}`,
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

  const selectedMode = MODES.find((m) => m.value === mode)!;

  return (
    <div className="modal-mask" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal">
        <header>执行清理</header>
        <div className="content">
          <div className="alert danger">
            <strong>⚠️ 此操作可能改变磁盘上的真实文件。</strong>
            <div style={{ marginTop: 4 }}>
              本次将处理 <strong>{fileTargets.length}</strong> 个文件，预计释放{' '}
              <strong>{humanBytes(totalBytes)}</strong>。
              建议先用「演练模式」确认清单，再执行真实操作。
            </div>
          </div>

          <div className="section" style={{ marginTop: 14 }}>
            <h3>处理方式</h3>
            {MODES.map((m) => (
              <label className="check" key={m.value}>
                <input
                  type="radio"
                  name="mode"
                  checked={mode === m.value}
                  onChange={() => setMode(m.value)}
                />
                <span>
                  {m.label}
                  <small>{m.desc}</small>
                </span>
              </label>
            ))}
          </div>

          {mode === 'moveTo' && (
            <label className="field">
              <span>隔离目录（留空使用默认）</span>
              <div className="row">
                <input
                  type="text"
                  value={moveDir}
                  onChange={(e) => setMoveDir(e.target.value)}
                  placeholder="默认：应用数据目录 / quarantine"
                />
                <button className="btn" onClick={pickMoveDir}>
                  <Icon name="folder" size={13} /> 选择
                </button>
              </div>
            </label>
          )}

          {mode === 'hardlink' && (
            <div className="alert warn">
              硬链接要求副本与保留文件位于<strong>同一文件系统</strong>。跨卷的条目会被跳过并如实报错。
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
              演练模式（不落盘）
              <small>只校验路径与权限，输出将执行的动作清单</small>
            </span>
          </label>

          {preview && preview.blocked.length > 0 && (
            <div className="alert warn" style={{ marginTop: 10 }}>
              <div>{preview.blocked.length} 个目标会被跳过：</div>
              <ul style={{ margin: '6px 0 0', paddingLeft: 18 }}>
                {preview.blocked.slice(0, 5).map((b, i) => (
                  <li key={i}>{b}</li>
                ))}
              </ul>
              {preview.blocked.length > 5 && (
                <div className="hint">…以及另外 {preview.blocked.length - 5} 条</div>
              )}
            </div>
          )}

          <div className="alert info" style={{ marginTop: 10 }}>
            <div className="kv">
              <span className="k">处理方式</span>
              <span className="v">{selectedMode.label}</span>
              <span className="k">可释放</span>
              <span className="v">{preview ? preview.human : humanBytes(totalBytes)}</span>
              <span className="k">实际可执行</span>
              <span className="v">
                {preview ? preview.count : fileTargets.length} / {fileTargets.length}
              </span>
              {archiveTargets.length > 0 && (
                <>
                  <span className="k">压缩包内条目</span>
                  <span className="v">{archiveTargets.length} 个（需单独移除）</span>
                </>
              )}
            </div>
          </div>

          {mode === 'trash' && (
            <div className="hint" style={{ marginTop: 8 }}>
              回收站模式由系统接管，撤销需从废纸篓手动还原；隔离区模式支持应用内一键撤销。
            </div>
          )}

          {/* 确认区放在最后：用户必须先看过上面的清单与汇总，再确认。 */}
          {!dryRun && (
            <div className="alert danger" style={{ marginTop: 10 }}>
              <strong>
                即将真实修改磁盘上的文件，共 {preview ? preview.count : fileTargets.length} 个。
              </strong>
              <div style={{ marginTop: 4 }}>
                处理方式：<strong>{selectedMode.label}</strong>
                {mode === 'trash' && ' —— 撤销需从系统废纸篓手动还原'}
                {mode === 'hardlink' && ' —— 不可自动撤销'}
                {mode === 'moveTo' && (
                  <>
                    {' '}
                    —— 隔离到 <code>{moveDir || '默认隔离目录'}</code>，可一键撤销
                  </>
                )}
              </div>
              <label className="check" style={{ marginTop: 8, marginBottom: 0 }}>
                <input
                  type="checkbox"
                  checked={acknowledged}
                  onChange={(e) => setAcknowledged(e.target.checked)}
                />
                <span>
                  我已核对以上清单，确认执行
                  <small>勾选后按钮才会启用；这一步是后端「无确认不执行」闸门的确认来源</small>
                </span>
              </label>
            </div>
          )}
        </div>

        <footer>
          <button className="btn" onClick={onClose} disabled={executing}>
            取消
          </button>
          <button
            className={dryRun ? 'btn primary' : 'btn danger'}
            onClick={run}
            disabled={executing || fileTargets.length === 0 || (!dryRun && !acknowledged)}
            title={!dryRun && !acknowledged ? '请先勾选「我已核对以上清单，确认执行」' : undefined}
          >
            {executing ? '处理中…' : dryRun ? '执行演练' : '确认并真实执行'}
          </button>
        </footer>
      </div>
    </div>
  );
}
