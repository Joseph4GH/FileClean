/** 内联 SVG 图标集：不引入图标库、不依赖字体，保证打包体积与主题一致性。 */

import type { FileGlyph } from '../format';

type IconName =
  | 'image'
  | 'audio'
  | 'video'
  | 'archive'
  | 'code'
  | 'doc'
  | 'folder'
  | 'check'
  | 'cross'
  | 'play'
  | 'trash'
  | 'link'
  | 'filter'
  | 'search'
  | 'shield'
  | 'disk'
  | 'clock'
  | 'chevron'
  | 'close'
  | 'plus'
  | 'minus'
  | 'up'
  | 'down'
  | 'export'
  | 'save'
  | 'eye'
  | 'undo'
  | 'settings'
  | 'sliders'
  | 'gauge'
  | 'refresh';

const PATHS: Record<IconName, string> = {
  image: 'M4 5h16v14H4zM4 15l4-4 4 4 3-3 5 5M9 9.5a1 1 0 1 0 0-.01',
  audio: 'M9 18V6l10-2v12M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0Zm10-2a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z',
  video: 'M3 6h12v12H3zM15 10l6-3v10l-6-3z',
  archive: 'M4 4h16v5H4zM5 9h14v11H5zM10 13h4',
  code: 'M9 8l-4 4 4 4M15 8l4 4-4 4',
  doc: 'M6 3h8l4 4v14H6zM14 3v4h4M9 13h6M9 17h6',
  folder: 'M3 6h6l2 2h10v11H3z',
  check: 'M4 12.5l5 5L20 6.5',
  cross: 'M6 6l12 12M18 6L6 18',
  play: 'M7 5l12 7-12 7z',
  trash: 'M5 7h14M10 7V5h4v2M7 7l1 13h8l1-13M11 11v6M14 11v6',
  link: 'M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1',
  filter: 'M4 5h16l-6 7v6l-4 2v-8z',
  search: 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM16.5 16.5L21 21',
  shield: 'M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z',
  disk: 'M4 6a8 3 0 1 0 16 0A8 3 0 0 0 4 6zM4 6v12a8 3 0 0 0 16 0V6M4 12a8 3 0 0 0 16 0',
  clock: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 8v5l3 2',
  chevron: 'M9 6l6 6-6 6',
  close: 'M6 6l12 12M18 6L6 18',
  plus: 'M12 5v14M5 12h14',
  minus: 'M5 12h14',
  up: 'M12 19V5M6 11l6-6 6 6',
  down: 'M12 5v14M6 13l6 6 6-6',
  export: 'M12 16V4M7 9l5-5 5 5M4 17v3h16v-3',
  save: 'M5 4h11l3 3v13H5zM8 4v5h7V4M8 15h8',
  eye: 'M2 12s3.6-6 10-6 10 6 10 6-3.6 6-10 6-10-6-10-6zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
  undo: 'M9 10H4V5M4 10a8 8 0 1 1 2 8',
  settings:
    'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.6 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.6a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z',
  sliders: 'M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0M14 4v4M8 10v4M16 16v4',
  gauge: 'M12 14l4-4M3.5 17a10 10 0 1 1 17 0M12 17h.01',
  refresh: 'M20 11a8 8 0 1 0-2.3 6M20 5v6h-6',
};

export function Icon({
  name,
  size = 16,
  className,
}: {
  name: IconName | FileGlyph;
  size?: number;
  className?: string;
}) {
  const d = PATHS[name as IconName] ?? PATHS.doc;
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      style={{ flexShrink: 0 }}
    >
      <path d={d} />
    </svg>
  );
}

export type { IconName };
