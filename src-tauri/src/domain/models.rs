//! 领域模型：前后端共享的数据契约（序列化统一 camelCase）。

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// ============================ 哈希算法 ============================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Xxhash3,
    Blake3,
}

impl Default for HashAlgorithm {
    fn default() -> Self {
        // BLAKE3 在 NVMe 上吞吐最高（可多线程），作为默认指纹算法。
        HashAlgorithm::Blake3
    }
}

impl HashAlgorithm {
    pub fn as_str(self) -> &'static str {
        match self {
            HashAlgorithm::Md5 => "md5",
            HashAlgorithm::Sha1 => "sha1",
            HashAlgorithm::Sha256 => "sha256",
            HashAlgorithm::Xxhash3 => "xxhash3",
            HashAlgorithm::Blake3 => "blake3",
        }
    }

    /// 输出摘要的十六进制长度，用于截断展示与缓存校验。
    pub fn digest_hex_len(self) -> usize {
        match self {
            HashAlgorithm::Md5 => 32,
            HashAlgorithm::Sha1 => 40,
            HashAlgorithm::Sha256 => 64,
            HashAlgorithm::Xxhash3 => 16,
            HashAlgorithm::Blake3 => 64,
        }
    }

    pub fn all() -> [HashAlgorithm; 5] {
        [
            HashAlgorithm::Blake3,
            HashAlgorithm::Xxhash3,
            HashAlgorithm::Sha256,
            HashAlgorithm::Sha1,
            HashAlgorithm::Md5,
        ]
    }
}

impl FromStr for HashAlgorithm {
    type Err = AppError;

    fn from_str(s: &str) -> AppResult<Self> {
        match s.to_ascii_lowercase().replace('-', "").as_str() {
            "md5" => Ok(HashAlgorithm::Md5),
            "sha1" => Ok(HashAlgorithm::Sha1),
            "sha256" => Ok(HashAlgorithm::Sha256),
            "xxhash" | "xxhash3" | "xxh3" => Ok(HashAlgorithm::Xxhash3),
            "blake3" => Ok(HashAlgorithm::Blake3),
            other => Err(AppError::config(format!(
                "未知哈希算法：{other}（可选 md5/sha1/sha256/xxhash3/blake3）"
            ))),
        }
    }
}

/// ============================ 文件条目 ============================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    /// 该文件所属的扫描根，用于计算相对路径与目录优先级。
    pub root: String,
    pub size: u64,
    /// Unix 秒。
    pub modified: i64,
    pub created: i64,
    /// 小写、不含点，例如 `jpg`；无扩展名为空串。
    pub extension: String,
    pub inode: u64,
    /// 所在设备号，与 inode 共同构成「同一物理文件」的判定依据。
    pub device: u64,
    /// 硬链接计数，> 1 说明该 inode 被多个路径共用。
    pub hard_links: u32,
    pub is_symlink: bool,
    /// 相对扫描根的层级深度（根下第一层为 1）。
    pub depth: usize,
    /// 相对于扫描根的路径，例如 `photos/2024/a.jpg`。
    pub relative_path: String,
}

impl FileEntry {
    /// 按路径长度排序用的「路径权重」：短路径优先保留时使用。
    pub fn path_len(&self) -> usize {
        self.path.chars().count()
    }

    pub fn file_name(&self) -> &str {
        self.path.rsplit(['/', '\\']).next().unwrap_or(&self.path)
    }

    pub fn parent_dir(&self) -> &str {
        match self.path.rfind(['/', '\\']) {
            Some(idx) => &self.path[..idx],
            None => "",
        }
    }
}

/// ============================ 扫描过滤 ============================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanFilter {
    /// 非空时仅扫描这些扩展名（白名单）。
    pub include_extensions: Vec<String>,
    /// 始终排除这些扩展名（黑名单优先级高于白名单）。
    pub exclude_extensions: Vec<String>,
    pub min_size: u64,
    /// 为 0 或 None 表示不设上限。
    pub max_size: Option<u64>,
    pub modified_after: Option<i64>,
    pub modified_before: Option<i64>,
    pub include_hidden: bool,
    pub include_system: bool,
    pub follow_symlinks: bool,
    /// 相对扫描根的 glob 排除规则，例如 `**/node_modules/**`。
    pub exclude_globs: Vec<String>,
    /// 小于该深度的文件不纳入（可用于跳过目录顶层的杂项文件）。
    pub min_depth: usize,
}

impl Default for ScanFilter {
    fn default() -> Self {
        ScanFilter {
            include_extensions: Vec::new(),
            exclude_extensions: Vec::new(),
            min_size: 1, // 零字节文件互相「重复」没有清理价值，默认跳过
            max_size: None,
            modified_after: None,
            modified_before: None,
            include_hidden: false,
            include_system: false,
            follow_symlinks: false,
            exclude_globs: default_exclude_globs(),
            min_depth: 0,
        }
    }
}

pub fn default_exclude_globs() -> Vec<String> {
    vec![
        "**/node_modules/**".into(),
        "**/.git/**".into(),
        "**/target/debug/**".into(),
        "**/Library/Caches/**".into(),
        "**/.Trash/**".into(),
        "**/.Trashes/**".into(),
        "**/$RECYCLE.BIN/**".into(),
        "**/System Volume Information/**".into(),
    ]
}

/// 跨平台「无价值」文件名，默认永远跳过。
pub const JUNK_FILE_NAMES: &[&str] = &[
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    "ehthumbs.db",
    ".localized",
    "Icon\r",
];

pub const JUNK_EXTENSIONS: &[&str] = &["tmp", "temp", "crdownload", "part", "partial", "download"];

/// ============================ 扫描选项 ============================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOptions {
    pub roots: Vec<String>,
    pub filter: ScanFilter,
    pub algorithm: HashAlgorithm,
    pub use_cache: bool,

    /// 图片感知哈希（pHash / dHash）相似检测。
    pub detect_perceptual: bool,
    /// 相似度阈值 0.0 - 1.0，例如 0.9 表示 90% 相似即算命中。
    pub perceptual_threshold: f32,

    /// 音频指纹相似检测。
    pub audio_fingerprint: bool,
    pub audio_threshold: f32,

    /// 压缩包内条目扫描（ZIP / 7Z）。
    pub scan_archives: bool,
    pub archive_max_entry_bytes: u64,

    /// RAW + JPEG 配对识别（视为一个整体，不参与重复清理）。
    pub raw_jpeg_pairing: bool,

    /// 引用完整性检查（Lightroom / Music / Premiere 目录 + 链接解析）。
    pub check_references: bool,

    /// 系统关键目录保护锁。
    pub protect_system_dirs: bool,

    /// 分组最少文件数，少于该数量的组不上报。
    pub min_group_size: usize,

    /// 0 表示自动。
    pub threads: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            roots: Vec::new(),
            filter: ScanFilter::default(),
            algorithm: HashAlgorithm::default(),
            use_cache: true,
            detect_perceptual: false,
            perceptual_threshold: 0.90,
            audio_fingerprint: false,
            audio_threshold: 0.92,
            scan_archives: false,
            archive_max_entry_bytes: 512 * 1024 * 1024,
            raw_jpeg_pairing: true,
            check_references: false,
            protect_system_dirs: true,
            min_group_size: 2,
            threads: 0,
        }
    }
}

impl ScanOptions {
    /// 边界校验：任何来自 UI / CLI 的输入都在这里过一遍，绝不信任调用方。
    pub fn validate(&self) -> AppResult<()> {
        if self.roots.is_empty() {
            return Err(AppError::config("至少需要指定一个扫描目录"));
        }
        for root in &self.roots {
            let p = std::path::Path::new(root);
            if !p.exists() {
                return Err(AppError::NotFound(root.clone()));
            }
            if !p.is_dir() {
                return Err(AppError::config(format!("扫描根不是目录：{root}")));
            }
        }
        if !(0.0..=1.0).contains(&self.perceptual_threshold) {
            return Err(AppError::config("相似度阈值必须落在 0.0 - 1.0 之间"));
        }
        if !(0.0..=1.0).contains(&self.audio_threshold) {
            return Err(AppError::config("音频相似度阈值必须落在 0.0 - 1.0 之间"));
        }
        if self.min_group_size < 2 {
            return Err(AppError::config("分组最少文件数不得小于 2"));
        }
        if let Some(max) = self.filter.max_size {
            if max > 0 && max < self.filter.min_size {
                return Err(AppError::config("文件大小上限不得小于下限"));
            }
        }
        if let (Some(after), Some(before)) = (self.filter.modified_after, self.filter.modified_before)
        {
            if before < after {
                return Err(AppError::config("修改时间上限不得早于下限"));
            }
        }
        if self.threads > 256 {
            return Err(AppError::config("线程数上限为 256"));
        }
        Ok(())
    }
}

/// ============================ 扫描阶段 ============================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    Idle,
    Walking,
    Filtering,
    PartialHash,
    FullHash,
    Perceptual,
    Audio,
    Archives,
    References,
    Marking,
    Done,
    Cancelled,
    Failed,
}

impl ScanPhase {
    pub fn label(self) -> &'static str {
        match self {
            ScanPhase::Idle => "空闲",
            ScanPhase::Walking => "遍历目录",
            ScanPhase::Filtering => "应用过滤规则",
            ScanPhase::PartialHash => "计算分块指纹（预筛）",
            ScanPhase::FullHash => "计算完整哈希",
            ScanPhase::Perceptual => "感知哈希相似检测",
            ScanPhase::Audio => "音频指纹比对",
            ScanPhase::Archives => "扫描压缩包内条目",
            ScanPhase::References => "引用完整性检查",
            ScanPhase::Marking => "应用保留策略",
            ScanPhase::Done => "完成",
            ScanPhase::Cancelled => "已取消",
            ScanPhase::Failed => "失败",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, ScanPhase::Done | ScanPhase::Cancelled | ScanPhase::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub scan_id: String,
    pub phase: ScanPhase,
    pub phase_label: String,
    pub files_seen: u64,
    pub files_total: u64,
    pub bytes_processed: u64,
    pub groups_found: u64,
    pub cache_hits: u64,
    pub current_path: String,
    pub elapsed_ms: u64,
    /// 0.0 - 1.0，前端进度条直接消费。
    pub percent: f32,
    pub message: String,
}

impl ScanProgress {
    pub fn new(scan_id: impl Into<String>, phase: ScanPhase) -> Self {
        let scan_id = scan_id.into();
        ScanProgress {
            scan_id,
            phase,
            phase_label: phase.label().to_string(),
            files_seen: 0,
            files_total: 0,
            bytes_processed: 0,
            groups_found: 0,
            cache_hits: 0,
            current_path: String::new(),
            elapsed_ms: 0,
            percent: 0.0,
            message: String::new(),
        }
    }

    pub fn with_percent(mut self, percent: f32) -> Self {
        self.percent = percent.clamp(0.0, 1.0);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanStatus {
    Running,
    Done,
    Cancelled,
    Failed,
}

/// ============================ 重复分组 ============================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchKind {
    /// 二进制完全一致。
    Exact,
    /// 图片感知哈希相似。
    Perceptual,
    /// 音频指纹相似。
    Audio,
    /// 压缩包内部条目重复。
    ArchiveInternal,
    /// RAW + JPEG 配对（信息性分组，不参与删除）。
    RawJpegPair,
}

impl MatchKind {
    pub fn label(self) -> &'static str {
        match self {
            MatchKind::Exact => "完全重复",
            MatchKind::Perceptual => "图片相似",
            MatchKind::Audio => "音频相似",
            MatchKind::ArchiveInternal => "压缩包内重复",
            MatchKind::RawJpegPair => "RAW/JPEG 配对",
        }
    }

    /// 信息性分组不可删除，UI 需隐藏清理入口。
    pub fn is_informational(self) -> bool {
        matches!(self, MatchKind::RawJpegPair)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkDecision {
    /// 未决策。
    Undecided,
    /// 保留（智能选择或用户手动）。
    Keep,
    /// 待清理。
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFile {
    pub entry: FileEntry,
    pub hash: Option<String>,
    /// 感知哈希 / 音频指纹的十六进制表示。
    pub fingerprint: Option<String>,
    pub decision: MarkDecision,
    /// 命中该决策的理由，直接展示给用户。
    pub reasons: Vec<String>,
    /// 非空表示受保护，禁止删除（引用完整性 / 系统目录 / 配对成员）。
    pub protected: Option<String>,
    /// 是否为该组内智能选择出的首选文件。
    pub is_preferred: bool,
    /// ZIP 内部条目路径（压缩包场景）。
    pub archive_inner_path: Option<String>,
    pub is_symlink: bool,
}

impl DuplicateFile {
    pub fn from_entry(entry: FileEntry) -> Self {
        DuplicateFile {
            is_symlink: entry.is_symlink,
            entry,
            hash: None,
            fingerprint: None,
            decision: MarkDecision::Undecided,
            reasons: Vec::new(),
            protected: None,
            is_preferred: false,
            archive_inner_path: None,
        }
    }

    pub fn is_deletable(&self) -> bool {
        self.protected.is_none() && !self.is_symlink
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub id: String,
    pub kind: MatchKind,
    pub kind_label: String,
    /// 组签名（精确哈希 / 感知哈希 / 音频指纹），用于前端去重展示。
    pub signature: String,
    /// 展示用标题，通常取首选文件的文件名。
    pub title: String,
    pub files: Vec<DuplicateFile>,
    /// 该组可回收的空间（不含保留文件）。
    pub wasted_bytes: u64,
    pub total_bytes: u64,
    /// 相似组才有值：0.0 - 1.0。
    pub similarity: Option<f32>,
    /// 信息性分组（RAW/JPEG 配对）标记。
    pub informational: bool,
}

impl DuplicateGroup {
    pub fn recompute(&mut self) {
        self.total_bytes = self.files.iter().map(|f| f.entry.size).sum();
        self.wasted_bytes = self
            .files
            .iter()
            .filter(|f| f.decision != MarkDecision::Keep)
            .map(|f| f.entry.size)
            .sum();
        if self.title.is_empty() {
            if let Some(first) = self.files.first() {
                self.title = first.entry.file_name().to_string();
            }
        }
    }
}

/// ============================ 扫描统计与结果 ============================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStats {
    pub files_seen: u64,
    pub files_scanned: u64,
    pub files_skipped: u64,
    pub bytes_scanned: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub partial_hashed: u64,
    pub full_hashed: u64,
    pub groups: u64,
    pub duplicate_files: u64,
    pub wasted_bytes: u64,
    pub protected_files: u64,
    pub archive_entries: u64,
    pub referenced_files: u64,
    pub duration_ms: u64,
    /// 被系统保护锁跳过的目录数量。
    pub protected_dirs_skipped: u64,
    /// 无权限访问而跳过的路径数量。
    pub inaccessible_paths: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub scan_id: String,
    pub roots: Vec<String>,
    pub algorithm: HashAlgorithm,
    pub options: ScanOptions,
    pub groups: Vec<DuplicateGroup>,
    pub stats: ScanStats,
    pub created_at: i64,
}

/// ============================ 动作层 ============================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeleteMode {
    /// 移入系统回收站 / 废纸篓（可撤销）。
    Trash,
    /// 用硬链接替换副本，指向保留文件（省空间且原路径仍可用）。
    HardlinkReplace,
    /// 移动到隔离目录供人工二次审核。
    MoveTo,
}

impl DeleteMode {
    pub fn as_str(self) -> &'static str {
        match self {
            DeleteMode::Trash => "trash",
            DeleteMode::HardlinkReplace => "hardlink",
            DeleteMode::MoveTo => "move",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DeleteMode::Trash => "移入回收站",
            DeleteMode::HardlinkReplace => "硬链接替换",
            DeleteMode::MoveTo => "移入隔离区",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            DeleteMode::Trash => "默认方式，可从系统废纸篓还原",
            DeleteMode::HardlinkReplace => "保留文件不动，副本位置改为硬链接，路径引用不失效",
            DeleteMode::MoveTo => "移动到隔离目录，人工确认后再彻底清理",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionTarget {
    pub path: String,
    /// 硬链接模式下保留文件（链接源）的绝对路径。
    pub keep_path: Option<String>,
    pub group_id: String,
    pub size: u64,
    pub archive_inner_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionPlan {
    pub mode: DeleteMode,
    /// MoveTo 模式的隔离目录；缺省用配置里的默认隔离区。
    pub move_dir: Option<String>,
    /// true 时只演练不落盘。
    pub dry_run: bool,
    pub targets: Vec<ActionTarget>,
    /// 前端显式二次确认位，缺失时一律拒绝执行。
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionOutcome {
    pub path: String,
    pub success: bool,
    pub error: Option<String>,
    pub freed_bytes: u64,
    /// MoveTo 模式下文件的新位置，用于撤销。
    pub moved_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub journal_id: Option<String>,
    pub mode: DeleteMode,
    pub dry_run: bool,
    pub outcomes: Vec<ActionOutcome>,
    pub succeeded: u64,
    pub failed: u64,
    pub freed_bytes: u64,
}

/// ============================ 智能选择策略 ============================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MarkRule {
    /// 保留修改时间最新的。
    KeepNewest,
    /// 保留修改时间最早的。
    KeepOldest,
    /// 保留创建时间最早的（更贴近「原片」）。
    KeepOldestCreated,
    /// 保留路径最短的。
    KeepShortestPath,
    /// 保留文件名最长的（常意味着信息更完整）。
    KeepLongestName,
    /// 保留深度最浅的（目录层级越靠上越是原始位置）。
    KeepShallowest,
    /// 保留大小最大的（对相似图来说是分辨率更高的）。
    KeepLargest,
    /// 在指定目录（子串匹配，不区分大小写）中的文件优先保留。
    KeepInPath { pattern: String },
    /// 按扩展名优先级保留，数组靠前者优先（RAW 优先场景）。
    PreferExtension { extensions: Vec<String> },
    /// 保留硬链接数最多的（说明被其它路径共用）。
    KeepMostLinked,
}

impl MarkRule {
    pub fn label(&self) -> String {
        match self {
            MarkRule::KeepNewest => "保留最新修改".into(),
            MarkRule::KeepOldest => "保留最早修改".into(),
            MarkRule::KeepOldestCreated => "保留最早创建（原片优先）".into(),
            MarkRule::KeepShortestPath => "保留路径最短".into(),
            MarkRule::KeepLongestName => "保留文件名最长".into(),
            MarkRule::KeepShallowest => "保留目录层级最浅".into(),
            MarkRule::KeepLargest => "保留体积最大".into(),
            MarkRule::KeepInPath { pattern } => format!("优先保留位于「{pattern}」的文件"),
            MarkRule::PreferExtension { extensions } => {
                format!("扩展名优先级：{}", extensions.join(" > "))
            }
            MarkRule::KeepMostLinked => "保留硬链接数最多".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkRuleEntry {
    pub rule: MarkRule,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoMarkConfig {
    /// 按数组顺序构成优先级链，前面的规则优先判定。
    pub rules: Vec<MarkRuleEntry>,
    /// 每组至少保留多少份（保底不删空）。
    pub keep_per_group: usize,
    /// 受引用完整性保护的文件永不标记删除。
    pub never_delete_protected: bool,
    /// 只在相似组内自动标记（精确重复组更安全）。
    pub also_mark_similar: bool,
}

impl Default for AutoMarkConfig {
    fn default() -> Self {
        AutoMarkConfig {
            rules: default_rules(),
            keep_per_group: 1,
            never_delete_protected: true,
            also_mark_similar: false,
        }
    }
}

/// 默认策略链：先按目录优先级（原片/素材目录），再按扩展名（RAW 优先），
/// 最后按「创建时间最早 → 路径最短」兜底。
pub fn default_rules() -> Vec<MarkRuleEntry> {
    vec![
        MarkRuleEntry {
            rule: MarkRule::KeepInPath {
                pattern: "原片".into(),
            },
            enabled: true,
        },
        MarkRuleEntry {
            rule: MarkRule::PreferExtension {
                extensions: vec![
                    "cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2", "pef", "srw",
                ]
                .into_iter()
                .map(String::from)
                .collect(),
            },
            enabled: true,
        },
        MarkRuleEntry {
            rule: MarkRule::KeepOldestCreated,
            enabled: true,
        },
        MarkRuleEntry {
            rule: MarkRule::KeepShortestPath,
            enabled: true,
        },
    ]
}

/// ============================ 引用完整性 ============================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceHit {
    pub path: String,
    pub source: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceReport {
    pub catalogs_scanned: Vec<String>,
    pub hits: Vec<ReferenceHit>,
    /// 解析失败的目录（例如加密的 lrcat），需要如实告知用户。
    pub warnings: Vec<String>,
}

/// ============================ 预览 ============================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDiffPreview {
    pub left_path: String,
    pub right_path: String,
    pub identical: bool,
    pub identical_prefix_bytes: u64,
    pub first_difference_offset: Option<u64>,
    pub left_snippet: String,
    pub right_snippet: String,
    pub left_truncated: bool,
    pub right_truncated: bool,
    pub binary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailPreview {
    pub path: String,
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub error: Option<String>,
}

/// ============================ 会话 ============================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub roots: Vec<String>,
    pub group_count: u64,
    pub wasted_bytes: u64,
    pub file_path: Option<String>,
}

/// ============================ 导出 ============================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Csv,
    Html,
    Xml,
    Json,
}

impl FromStr for ExportFormat {
    type Err = AppError;

    fn from_str(s: &str) -> AppResult<Self> {
        match s.to_ascii_lowercase().as_str() {
            "csv" => Ok(ExportFormat::Csv),
            "html" | "htm" => Ok(ExportFormat::Html),
            "xml" => Ok(ExportFormat::Xml),
            "json" => Ok(ExportFormat::Json),
            other => Err(AppError::config(format!("未知导出格式：{other}"))),
        }
    }
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Html => "html",
            ExportFormat::Xml => "xml",
            ExportFormat::Json => "json",
        }
    }
}
