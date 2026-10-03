import { useEffect, useMemo, useState } from 'react';

import { api, humanize } from '../api';
import {
  AUDIO_EXT,
  DECISION_META,
  IMAGE_EXT,
  KIND_META,
  baseName,
  formatTime,
  humanBytes,
  percent,
} from '../format';
import type { AudioPreview, DuplicateFile, DuplicateGroup, MarkDecision, TextDiffPreview, ThumbnailPreview } from '../types';
import { Icon } from './Icon';

interface Props {
  group: DuplicateGroup;
  selectedPath: string | null;
  onSelectPath: (path: string) => void;
  onSetFileDecision: (path: string, decision: MarkDecision) => void;
  onSetGroupDecision: (decision: MarkDecision) => void;
  onInvert: () => void;
  onReveal: (path: string) => void;
  onRemoveArchiveEntries: (group: DuplicateGroup) => void;
  onNotify: (kind: 'info' | 'success' | 'error', message: string) => void;
}

function useThumbnail(path: string | null, maxEdge: number) {
  const [state, setState] = useState<ThumbnailPreview | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let alive = true;
    if (!path) {
      setState(null);
      return;
    }
    setLoading(true);
    api
      .previewThumbnail(path, maxEdge)
      .then((result) => {
        if (alive) setState(result);
      })
      .catch(() => {
        if (alive) setState(null);
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [path, maxEdge]);

  return { preview: state, loading };
}

function useDiff(left: string | null, right: string | null) {
  const [state, setState] = useState<TextDiffPreview | null>(null);

  useEffect(() => {
    let alive = true;
    if (!left || !right || left === right) {
      setState(null);
      return;
    }
    api
      .previewText(left, right)
      .then((result) => {
        if (alive) setState(result);
      })
      .catch(() => {
        if (alive) setState(null);
      });
    return () => {
      alive = false;
    };
  }, [left, right]);

  return state;
}

function useAudio(path: string | null) {
  const [state, setState] = useState<AudioPreview | null>(null);

  useEffect(() => {
    let alive = true;
    if (!path) {
      setState(null);
      return;
    }
    api
      .previewAudio(path)
      .then((result) => {
        if (alive) setState(result);
      })
      .catch(() => {
        if (alive) setState(null);
      });
    return () => {
      alive = false;
    };
  }, [path]);

  return state;
}

function Spectrum({ preview, title }: { preview: AudioPreview | null; title: string }) {
  if (!preview) return null;
  if (preview.error) {
    return (
      <div className="preview-box">
        <div className="cap">{title}</div>
        <div className="hint" style={{ padding: 8 }}>
          {preview.error}
        </div>
      </div>
    );
  }
  const max = Math.max(...preview.bands, 1e-6);
  return (
    <div className="preview-box">
      <div className="cap">
        <span>{title}</span>
        <span>{preview.durationSecs.toFixed(1)} 秒 · 频谱轮廓</span>
      </div>
      <div className="spectrum">
        {preview.bands.map((v, i) => (
          <i key={i} style={{ height: `${Math.max(3, (v / max) * 100)}%` }} />
        ))}
      </div>
    </div>
  );
}

function PreviewArea({
  group,
  selected,
  reference,
}: {
  group: DuplicateGroup;
  selected: DuplicateFile;
  reference: DuplicateFile | null;
}) {
  const ext = selected.entry.extension.toLowerCase();
  const otherExt = reference?.entry.extension.toLowerCase() ?? '';
  const isImage = IMAGE_EXT.has(ext) && IMAGE_EXT.has(otherExt);
  const isAudio = AUDIO_EXT.has(ext) && AUDIO_EXT.has(otherExt);

  const leftPath = reference && reference.entry.path !== selected.entry.path ? reference.entry.path : null;
  const rightPath = selected.entry.path;

  const imageA = useThumbnail(isImage ? leftPath : null, 640);
  const imageB = useThumbnail(isImage ? rightPath : null, 640);
  const audioA = useAudio(isAudio ? leftPath : null);
  const audioB = useAudio(isAudio ? rightPath : null);
  const diff = useDiff(!isImage && !isAudio ? leftPath : null, !isImage && !isAudio ? rightPath : null);

  if (isImage) {
    return (
      <div className="preview-box">
        <div className="cap">
          <span>图像对比</span>
          <span>
            {imageA.preview ? `${imageA.preview.width}×${imageA.preview.height}` : '—'} vs{' '}
            {imageB.preview ? `${imageB.preview.width}×${imageB.preview.height}` : '—'}
          </span>
        </div>
        <div className="diff" style={{ gridTemplateColumns: '1fr 1fr' }}>
          {[imageA, imageB].map((side, idx) => (
            <div key={idx} style={{ background: 'var(--panel)', padding: 6, textAlign: 'center' }}>
              {side.preview?.error ? (
                <div className="hint">{side.preview.error}</div>
              ) : side.preview?.dataUrl ? (
                <img src={side.preview.dataUrl} alt="" style={{ maxHeight: 210 }} />
              ) : (
                <div className="hint" style={{ padding: 30 }}>
                  {idx === 0 ? '（无对照文件）' : '加载中…'}
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    );
  }

  if (isAudio) {
    return (
      <div style={{ display: 'grid', gap: 8 }}>
        <Spectrum preview={audioA} title={leftPath ? baseName(leftPath) : '对照'} />
        <Spectrum preview={audioB} title={baseName(rightPath)} />
      </div>
    );
  }

  if (!leftPath) {
    return (
      <div className="preview-box">
        <div className="cap">内容预览</div>
        <div className="hint" style={{ padding: 8 }}>
          该分组没有保留项作为对照，无法做差异对比。
        </div>
      </div>
    );
  }

  if (!diff) {
    return (
      <div className="preview-box">
        <div className="cap">内容对比</div>
        <div className="hint" style={{ padding: 8 }}>
          正在比较…
        </div>
      </div>
    );
  }

  return (
    <div className="preview-box">
      <div className="cap">
        <span>{diff.binary ? '二进制文件对比' : '文本差异对比'}</span>
        <span>
          {diff.identical
            ? '内容完全一致'
            : diff.firstDifferenceOffset != null
              ? `首个差异位于第 ${diff.firstDifferenceOffset} 字节`
              : '长度不同'}
        </span>
      </div>
      {diff.binary ? (
        <div className="hint" style={{ padding: 10 }}>
          这两个文件是二进制内容，无法逐行展示差异。
          {diff.identicalPrefixBytes > 0 &&
            ` 前 ${humanBytes(diff.identicalPrefixBytes)} 完全一致。`}
          {group.kind === 'exact' && ' 精确哈希已判定内容一致，可放心按规则清理。'}
        </div>
      ) : (
        <div className="diff">
          <pre>{diff.leftSnippet || '（空）'}</pre>
          <pre>{diff.rightSnippet || '（空）'}</pre>
        </div>
      )}
    </div>
  );
}

export function GroupDetail({
  group,
  selectedPath,
  onSelectPath,
  onSetFileDecision,
  onSetGroupDecision,
  onInvert,
  onReveal,
  onRemoveArchiveEntries,
  onNotify,
}: Props) {
  const selected = useMemo(
    () => group.files.find((f) => f.entry.path === selectedPath) ?? group.files[0],
    [group, selectedPath],
  );

  const reference = useMemo(
    () => group.files.find((f) => f.decision === 'keep' && f.entry.path !== selected.entry.path) ?? null,
    [group, selected],
  );

  const kind = KIND_META[group.kind];
  const removable = group.files.filter((f) => f.decision === 'remove');
  const totalWaste = removable.reduce((sum, f) => sum + f.entry.size, 0);

  return (
    <div className="detail-pane">
      <div className="detail-head">
        <h2 title={group.title}>{group.title || baseName(selected.entry.path)}</h2>
        <div className="row wrap" style={{ gap: 6 }}>
          <span className={`tag ${kind.tone}`}>{kind.label}</span>
          <span className="tag">{group.files.length} 个文件</span>
          {group.similarity != null && <span className="tag">相似度 {percent(group.similarity)}</span>}
          {totalWaste > 0 && <span className="tag danger">可回收 {humanBytes(totalWaste)}</span>}
        </div>

        {!group.informational && (
          <div className="btn-row" style={{ marginTop: 10 }}>
            <button
              className="btn sm"
              onClick={() => onSetGroupDecision('remove')}
              title="保留每组首选，其余标记为待清理"
            >
              <Icon name="minus" size={13} /> 只留一份
            </button>
            <button className="btn sm" onClick={onInvert} title="反向选择：首选改为清理，其余保留">
              <Icon name="undo" size={13} /> 反向选择
            </button>
            <button className="btn sm" onClick={() => onSetGroupDecision('keep')}>
              <Icon name="check" size={13} /> 全部保留
            </button>
          </div>
        )}

        {group.kind === 'archiveInternal' && (
          <div className="alert info" style={{ marginTop: 10 }}>
            这些条目位于压缩包内部。清理需要重写压缩包（会生成新的包体并原子替换原文件）。
            <div style={{ marginTop: 8 }}>
              <button className="btn sm" onClick={() => onRemoveArchiveEntries(group)}>
                <Icon name="archive" size={13} /> 移除选中条目
              </button>
            </div>
          </div>
        )}

        {group.informational && (
          <div className="alert info" style={{ marginTop: 10 }}>
            RAW 与 JPEG 是同一张照片的两种载体，已作为整体保留，不参与清理。
          </div>
        )}
      </div>

      <PreviewArea group={group} selected={selected} reference={reference} />

      <div style={{ padding: '8px 16px 4px' }} className="hint">
        点击文件名可切换预览与对照；「清理 / 保留」按钮会即时更新标记。
      </div>

      {group.files.map((file) => {
        const meta = DECISION_META[file.decision];
        const isSelected = file.entry.path === selected.entry.path;
        return (
          <div
            key={file.entry.path}
            className={`file-item${isSelected ? ' selected' : ''}${
              file.decision === 'remove' ? ' removing' : file.decision === 'keep' ? ' keeping' : ''
            }`}
            onClick={() => onSelectPath(file.entry.path)}
          >
            <div className="file-top">
              <span className="name" title={file.entry.path}>
                {baseName(file.entry.path)}
              </span>
              {file.isPreferred && <span className="tag accent">首选</span>}
              <span className={`tag ${meta.tone}`}>{meta.label}</span>
            </div>

            <div className="file-path">
              {file.archiveInnerPath ? `${file.entry.path} › ${file.archiveInnerPath}` : file.entry.path}
            </div>

            <div className="file-facts">
              <span>{humanBytes(file.entry.size)}</span>
              <span>修改 {formatTime(file.entry.modified)}</span>
              {file.entry.hardLinks > 1 && <span>硬链接 ×{file.entry.hardLinks}</span>}
              {file.hash && (
                <span title={file.hash} style={{ fontFamily: 'var(--mono)' }}>
                  {file.hash.slice(0, 12)}…
                </span>
              )}
            </div>

            {file.protected && (
              <div className="reasons">
                <span className="tag warn">
                  <Icon name="shield" size={11} /> {file.protected}
                </span>
              </div>
            )}

            {file.reasons.length > 0 && !file.protected && (
              <div className="reasons">
                <ul>
                  {file.reasons.slice(0, 3).map((r, i) => (
                    <li key={i}>{r}</li>
                  ))}
                </ul>
              </div>
            )}

            <div className="btn-row" style={{ marginTop: 7, gap: 6 }}>
              <button
                className="btn sm"
                style={{ flex: '0 0 auto' }}
                disabled={file.decision === 'keep'}
                onClick={(e) => {
                  e.stopPropagation();
                  onSetFileDecision(file.entry.path, 'keep');
                }}
              >
                保留
              </button>
              <button
                className="btn sm"
                style={{ flex: '0 0 auto' }}
                disabled={file.decision === 'remove' || !!file.protected || file.isSymlink}
                title={file.protected ? `受保护：${file.protected}` : '标记为待清理'}
                onClick={(e) => {
                  e.stopPropagation();
                  onSetFileDecision(file.entry.path, 'remove');
                }}
              >
                清理
              </button>
              <button
                className="btn ghost sm"
                onClick={(e) => {
                  e.stopPropagation();
                  onReveal(file.entry.path);
                }}
                title="在文件管理器中显示"
              >
                <Icon name="folder" size={13} />
              </button>
            </div>
          </div>
        );
      })}

      <div style={{ padding: 16 }}>
        <button
          className="btn block sm"
          onClick={() => {
            navigator.clipboard
              .writeText(group.files.map((f) => f.entry.path).join('\n'))
              .then(() => onNotify('success', `已复制 ${group.files.length} 条路径`))
              .catch((e) => onNotify('error', humanize(e)));
          }}
        >
          复制本组全部路径
        </button>
      </div>
    </div>
  );
}
