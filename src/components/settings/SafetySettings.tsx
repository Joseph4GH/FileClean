import type { ScanOptions, SystemInfo } from '../../types';
import { Icon } from '../Icon';

interface Props {
  options: ScanOptions;
  patch: (value: Partial<ScanOptions>) => void;
  disabled: boolean;
  systemInfo: SystemInfo | null;
}

const PROTECTED_HINT: Record<string, string> = {
  macos: '/System、/usr、/private/etc、/private/var、~/Library 等系统与个人库目录',
  windows: 'Windows、Program Files、ProgramData 等系统目录',
};

export function SafetySettings({ options, patch, disabled, systemInfo }: Props) {
  const platform = systemInfo?.platform ?? 'macos';

  return (
    <div className="settings-form">
      <div className="card">
        <label className="check">
          <input
            type="checkbox"
            checked={options.protectSystemDirs}
            onChange={(e) => patch({ protectSystemDirs: e.target.checked })}
            disabled={disabled}
          />
          <span>
            系统关键目录保护锁
            <small>{PROTECTED_HINT[platform] ?? '系统与个人库目录'}</small>
          </span>
        </label>
        <small className="hint" style={{ paddingLeft: 21 }}>
          不开玩笑的一层：即便你把整个磁盘选为扫描根，这些目录在遍历阶段就被剪枝，不会被读到。
        </small>
      </div>

      {options.protectSystemDirs === false && (
        <div className="alert warn" style={{ marginTop: 10 }}>
          <strong>保护锁已关闭。</strong>
          <div style={{ marginTop: 4 }}>
            你仍然受到「清理目标必须在扫描根内」的边界校验保护，但系统目录会被纳入扫描范围。
            仅在明确知道自己在做什么时保持关闭。
          </div>
        </div>
      )}

      <div className="divider" />

      <div className="card">
        <label className="check">
          <input
            type="checkbox"
            checked={options.checkReferences}
            onChange={(e) => patch({ checkReferences: e.target.checked })}
            disabled={disabled}
          />
          <span>
            引用完整性检查
            <small>检查素材库与快捷方式引用，命中的文件视为受保护，永不标记删除</small>
          </span>
        </label>
        <small className="hint" style={{ paddingLeft: 21 }}>
          会自动发现：Lightroom 目录（.lrcat）、Apple Music / iTunes 资料库、
          Premiere 工程文件、macOS Alias 与 Windows 快捷方式。
        </small>
      </div>

      <div className="divider" />

      <div className="field">
        <span>始终生效的安全红线</span>
        <ul className="steps">
          <li>
            <b>无确认不执行</b>：缺少显式确认时，真实执行直接被拒绝，一个文件都不会动
          </li>
          <li>
            <b>不越界</b>：清理目标必须位于扫描根之内，且不能是符号链接指向的外部路径
          </li>
          <li>
            <b>可追溯</b>：每次清理写入 SQLite 日志与 JSON 双份记录，隔离区移动支持一键撤销
          </li>
          <li>
            <b>不空组</b>：任何情况下每组至少保留一份，硬链接同源与配对成员强制保留
          </li>
        </ul>
        <small className="hint">
          <Icon name="shield" size={11} /> 这四条不随任何设置变化，是硬编码的。
        </small>
      </div>
    </div>
  );
}
