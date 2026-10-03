import { ALGORITHM_OPTIONS } from '../../format';
import type { ScanOptions } from '../../types';
import { Icon } from '../Icon';

interface Props {
  options: ScanOptions;
  patch: (value: Partial<ScanOptions>) => void;
  disabled: boolean;
}

export function AlgorithmSettings({ options, patch, disabled }: Props) {
  const selected = ALGORITHM_OPTIONS.find((o) => o.value === options.algorithm);

  return (
    <div className="settings-form">
      <label className="field">
        <span>哈希算法</span>
        <select
          value={options.algorithm}
          onChange={(e) => patch({ algorithm: e.target.value as ScanOptions['algorithm'] })}
          disabled={disabled}
        >
          {ALGORITHM_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
        {selected && <small className="hint">{selected.hint}</small>}
      </label>

      <div className="alert info">
        <div className="row" style={{ gap: 6, marginBottom: 4 }}>
          <Icon name="sliders" size={13} />
          <span>三级筛选策略（自动生效，无需配置）</span>
        </div>
        <ol className="steps">
          <li>
            <b>体积预筛</b>：大小唯一的文件直接排除，不读磁盘
          </li>
          <li>
            <b>分块指纹</b>：读首尾各 64 KiB + 体积算 xxHash3，不同者不进入下一轮
          </li>
          <li>
            <b>全量哈希</b>：只剩同体积且分块指纹一致的文件才做完整比对
          </li>
        </ol>
        <small className="hint">
          大文件走内存映射，BLAKE3 可在单个文件内部并行，4K 视频与 ISO 不会占满内存
        </small>
      </div>

      <div className="divider" />

      <div className="field">
        <span>内容匹配（默认开启）</span>
        <label className="check">
          <input
            type="checkbox"
            checked={options.rawJpegPairing}
            onChange={(e) => patch({ rawJpegPairing: e.target.checked })}
            disabled={disabled}
          />
          <span>
            RAW + JPEG 配对识别
            <small>同名 RAW/JPEG 视为同一张照片整体保留，不当作重复清理</small>
          </span>
        </label>
      </div>

      <div className="divider" />

      <div className="field">
        <span>相似匹配（默认关闭，按需开启）</span>

        <label className="check">
          <input
            type="checkbox"
            checked={options.detectPerceptual}
            onChange={(e) => patch({ detectPerceptual: e.target.checked })}
            disabled={disabled}
          />
          <span>
            图片感知哈希
            <small>pHash / dHash 识别裁剪、调色、压缩、改尺寸后的同一张图</small>
          </span>
        </label>
        {options.detectPerceptual && (
          <div className="slider-row">
            <span className="slider-label">相似度阈值</span>
            <input
              type="range"
              min={70}
              max={100}
              value={Math.round(options.perceptualThreshold * 100)}
              onChange={(e) => patch({ perceptualThreshold: Number(e.target.value) / 100 })}
              disabled={disabled}
            />
            <span className="slider-value">{(options.perceptualThreshold * 100).toFixed(0)}%</span>
          </div>
        )}
        {options.detectPerceptual && (
          <small className="hint">
            越低越宽松（误报更多）。经验值 90%；低于 85% 容易把同版式不同配色的图合并
          </small>
        )}

        <label className="check" style={{ marginTop: 6 }}>
          <input
            type="checkbox"
            checked={options.audioFingerprint}
            onChange={(e) => patch({ audioFingerprint: e.target.checked })}
            disabled={disabled}
          />
          <span>
            音频指纹
            <small>频谱轮廓比对，识别不同码率 / 不同标签但听感相同的音乐</small>
          </span>
        </label>
        {options.audioFingerprint && (
          <div className="slider-row">
            <span className="slider-label">相似度阈值</span>
            <input
              type="range"
              min={70}
              max={100}
              value={Math.round(options.audioThreshold * 100)}
              onChange={(e) => patch({ audioThreshold: Number(e.target.value) / 100 })}
              disabled={disabled}
            />
            <span className="slider-value">{(options.audioThreshold * 100).toFixed(0)}%</span>
          </div>
        )}
        {options.audioFingerprint && (
          <small className="hint">
            同一首歌的不同压制品实测相似度约 0.98，不同曲目约 0.001，区分度很高
          </small>
        )}

        <label className="check" style={{ marginTop: 6 }}>
          <input
            type="checkbox"
            checked={options.scanArchives}
            onChange={(e) => patch({ scanArchives: e.target.checked })}
            disabled={disabled}
          />
          <span>
            扫描压缩包内部
            <small>
              不解压即可检索 ZIP / 7Z 内的重复条目；RAR 需系统安装 7z 命令行工具
            </small>
          </span>
        </label>
      </div>
    </div>
  );
}
