import { useEffect, useMemo, useRef, useState } from 'react';

import { cachedThumbnail, getThumbnail } from '../thumbnails';
import { IMAGE_EXT, KIND_META, baseName, humanBytes, num, percent, parentDir, shortPath } from '../format';
import type { DuplicateGroup } from '../types';
import { Icon } from './Icon';

/** 固定行高是虚拟滚动的前提；改这里必须同步改 .group-row 的高度。 */
const ROW_HEIGHT = 62;
const OVERSCAN = 6;

function Thumb({ path, extension }: { path: string; extension: string }) {
  const [src, setSrc] = useState<string | null | undefined>(() => cachedThumbnail(path));

  useEffect(() => {
    let alive = true;
    if (!IMAGE_EXT.has(extension.toLowerCase())) {
      setSrc(null);
      return;
    }
    const known = cachedThumbnail(path);
    if (known !== undefined) {
      setSrc(known);
      return;
    }
    getThumbnail(path, 96).then((value) => {
      if (alive) setSrc(value);
    });
    return () => {
      alive = false;
    };
  }, [path, extension]);

  if (src) return <img src={src} alt="" loading="lazy" />;
  const glyph = IMAGE_EXT.has(extension.toLowerCase()) ? 'image' : undefined;
  return <Icon name={glyph ?? 'doc'} size={18} />;
}

function GroupRow({
  group,
  selected,
  onSelect,
}: {
  group: DuplicateGroup;
  selected: boolean;
  onSelect: () => void;
}) {
  const kind = KIND_META[group.kind];
  const removable = group.files.filter((f) => f.decision === 'remove').length;
  const thumbFile = group.files[0];

  return (
    <div
      className={`group-row${selected ? ' selected' : ''}`}
      style={{ height: ROW_HEIGHT }}
      onClick={onSelect}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') onSelect();
      }}
    >
      <div className="group-thumb">
        {thumbFile ? <Thumb path={thumbFile.entry.path} extension={thumbFile.entry.extension} /> : null}
      </div>

      <div className="group-main">
        <div className="group-title">{group.title || baseName(thumbFile?.entry.path ?? '')}</div>
        <div className="group-meta">
          <span className={`tag ${kind.tone}`}>{kind.label}</span>
          <span>{group.files.length} 个文件</span>
          {group.similarity != null && <span>相似度 {percent(group.similarity)}</span>}
          {group.files.some((f) => f.protected) && (
            <span className="tag warn">
              <Icon name="shield" size={11} /> 含保护
            </span>
          )}
          {removable > 0 && <span className="tag danger">{removable} 待清理</span>}
        </div>
        <div className="hint" title={thumbFile?.entry.path} style={{ marginTop: 1 }}>
          {shortPath(parentDir(thumbFile?.entry.path ?? ''), 68)}
        </div>
      </div>

      <div className="group-right">
        <div className="waste">{group.wastedBytes > 0 ? humanBytes(group.wastedBytes) : '—'}</div>
        <div className="n">共 {humanBytes(group.totalBytes)}</div>
      </div>
    </div>
  );
}

export function GroupList({
  groups,
  selectedId,
  onSelect,
}: {
  groups: DuplicateGroup[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewport, setViewport] = useState(600);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setViewport(el.clientHeight));
    observer.observe(el);
    setViewport(el.clientHeight);
    return () => observer.disconnect();
  }, []);

  const { start, end } = useMemo(() => {
    const first = Math.floor(scrollTop / ROW_HEIGHT);
    return {
      start: Math.max(0, first - OVERSCAN),
      end: Math.min(groups.length, Math.ceil((scrollTop + viewport) / ROW_HEIGHT) + OVERSCAN),
    };
  }, [scrollTop, viewport, groups.length]);

  const visible = groups.slice(start, end);

  if (groups.length === 0) {
    return (
      <div className="vlist">
        <div className="empty">
          <span className="big">
            <Icon name="search" size={30} />
          </span>
          <div>没有符合当前条件的重复分组</div>
          <div className="hint">试着放宽过滤条件，或开启「图片感知哈希」查找相似文件</div>
        </div>
      </div>
    );
  }

  return (
    <div
      className="vlist"
      ref={containerRef}
      onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
    >
      <div className="vlist-inner" style={{ height: groups.length * ROW_HEIGHT }}>
        <div
          style={{
            position: 'absolute',
            top: start * ROW_HEIGHT,
            left: 0,
            right: 0,
          }}
        >
          {visible.map((group) => (
            <GroupRow
              key={group.id}
              group={group}
              selected={group.id === selectedId}
              onSelect={() => onSelect(group.id)}
            />
          ))}
        </div>
      </div>
      <div className="hint" style={{ padding: '6px 16px 12px' }}>
        共 {num(groups.length)} 个分组
        {groups.length > 0 && (
          <>
            {' · '}
            可回收 {humanBytes(groups.reduce((sum, g) => sum + g.wastedBytes, 0))}
          </>
        )}
      </div>
    </div>
  );
}
