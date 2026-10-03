//! 目录遍历：把文件系统铺平成待处理的 [`FileEntry`] 列表。
//!
//! 遍历本身是单线程的（I/O 元数据读取在 SSD 上已经很快且更省内存），
//! 真正的并行发生在后续的哈希计算阶段 —— 那里才是 CPU/IO 瓶颈。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use walkdir::{DirEntry, WalkDir};

use crate::domain::models::FileEntry;
use crate::error::{AppError, AppResult};
use crate::scan::filter::{extension_of, path_to_string, relative_path, FilterEngine, SkipReason};

#[derive(Debug, Default, Clone)]
pub struct WalkStats {
    pub files_seen: u64,
    pub files_accepted: u64,
    pub files_rejected: u64,
    pub bytes_accepted: u64,
    pub protected_dirs_skipped: u64,
    pub junk_dirs_skipped: u64,
    pub inaccessible_paths: u64,
    pub truncated: bool,
}

#[derive(Debug)]
pub struct WalkOutcome {
    pub entries: Vec<FileEntry>,
    pub stats: WalkStats,
    /// 被保护锁跳过的目录样例（最多保留若干条，用于 UI 说明）。
    pub skipped_examples: Vec<(String, &'static str)>,
}

pub struct WalkContext<'a> {
    pub cancel: &'a AtomicBool,
    pub max_files: usize,
    /// 每处理 N 个文件回调一次，避免高频事件把前端压垮。
    pub on_progress: Box<dyn FnMut(u64, &str) + Send + 'a>,
}

/// 单次遍历一个根目录。
pub fn walk_root(
    root: &Path,
    engine: &FilterEngine,
    ctx: &mut WalkContext<'_>,
    out: &mut WalkOutcome,
) -> AppResult<()> {
    engine.check_root(root)?;

    let follow = engine.filter().follow_symlinks;

    // 剪枝统计放在局部变量里：`filter_entry` 的闭包会在整个迭代期间借用它们，
    // 若直接借用 `out.stats` 会与主循环的 `out.stats.files_seen` 冲突。
    let mut prune_stats = PruneStats::default();
    let mut prune_examples: Vec<(String, &'static str)> = Vec::new();

    let walker = WalkDir::new(root)
        .follow_links(follow)
        .same_file_system(false)
        .contents_first(false)
        .into_iter()
        .filter_entry(|e| keep_entry(e, engine, root, &mut prune_stats, &mut prune_examples));

    let mut last_emit = Instant::now();
    let mut seen: u64 = 0;
    let mut accepted: u64 = 0;
    let mut rejected: u64 = 0;
    let mut bytes: u64 = 0;
    let mut inaccessible: u64 = 0;

    for item in walker {
        if ctx.cancel.load(Ordering::Relaxed) {
            out.stats.files_seen += seen;
            out.stats.files_accepted += accepted;
            out.stats.files_rejected += rejected;
            out.stats.bytes_accepted += bytes;
            out.stats.inaccessible_paths += inaccessible;
            merge_prune(out, prune_stats, prune_examples);
            return Err(AppError::Cancelled);
        }

        let entry = match item {
            Ok(e) => e,
            Err(err) => {
                // 权限不足 / 路径过长 / 遍历中断：计入统计并继续，不中断整次扫描。
                inaccessible += 1;
                tracing::debug!(error = %err, "遍历时跳过不可访问路径");
                continue;
            }
        };

        seen += 1;

        if seen % 512 == 0 || last_emit.elapsed() > Duration::from_millis(150) {
            last_emit = Instant::now();
            (ctx.on_progress)(seen, &path_to_string(entry.path()));
        }

        if !entry.file_type().is_file() {
            continue;
        }

        let is_symlink = entry.file_type().is_symlink();
        if is_symlink && !follow {
            rejected += 1;
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                inaccessible += 1;
                continue;
            }
        };

        let size = metadata.len();
        let modified = system_time_to_unix(metadata.modified().ok());
        let depth = entry.depth();

        if !engine.accept_file(entry.path(), root, size, modified, depth) {
            rejected += 1;
            continue;
        }

        let created = metadata
            .created()
            .ok()
            .map(|t| system_time_to_unix(Some(t)))
            .filter(|v| *v > 0)
            .unwrap_or(modified);

        let file_entry = FileEntry {
            path: path_to_string(entry.path()),
            root: path_to_string(root),
            size,
            modified,
            created,
            extension: extension_of(entry.path()),
            inode: inode_of(&metadata),
            device: device_of(&metadata),
            hard_links: hard_link_count(&metadata),
            is_symlink,
            depth,
            relative_path: relative_path(entry.path(), root),
        };

        accepted += 1;
        bytes += size;
        out.entries.push(file_entry);

        if out.entries.len() >= ctx.max_files {
            out.stats.truncated = true;
            tracing::warn!(limit = ctx.max_files, "达到文件数量上限，扫描提前结束");
            break;
        }
    }

    out.stats.files_seen += seen;
    out.stats.files_accepted += accepted;
    out.stats.files_rejected += rejected;
    out.stats.bytes_accepted += bytes;
    out.stats.inaccessible_paths += inaccessible;
    merge_prune(out, prune_stats, prune_examples);

    Ok(())
}

#[derive(Default)]
struct PruneStats {
    protected: u64,
    junk: u64,
}

fn merge_prune(out: &mut WalkOutcome, stats: PruneStats, examples: Vec<(String, &'static str)>) {
    out.stats.protected_dirs_skipped += stats.protected;
    out.stats.junk_dirs_skipped += stats.junk;
    for item in examples {
        if out.skipped_examples.len() >= 50 {
            break;
        }
        out.skipped_examples.push(item);
    }
}

/// `filter_entry` 回调：返回 false 则该目录整体不进入。
fn keep_entry(
    entry: &DirEntry,
    engine: &FilterEngine,
    root: &Path,
    stats: &mut PruneStats,
    examples: &mut Vec<(String, &'static str)>,
) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let is_dir =
        entry.file_type().is_dir() || (entry.file_type().is_symlink() && entry.path().is_dir());
    if !is_dir {
        return true;
    }

    match engine.should_prune_dir(entry.path(), root, entry.file_type().is_symlink()) {
        None => true,
        Some(reason) => {
            match reason {
                SkipReason::SystemProtected => stats.protected += 1,
                SkipReason::StructuralJunk => stats.junk += 1,
                _ => {}
            }
            if examples.len() < 50 {
                examples.push((path_to_string(entry.path()), reason.label()));
            }
            false
        }
    }
}

fn system_time_to_unix(t: Option<SystemTime>) -> i64 {
    match t {
        Some(t) => match t.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            // 时钟回拨到 1970 之前：取 0 而不是 panic。
            Err(_) => 0,
        },
        None => 0,
    }
}

#[cfg(unix)]
fn inode_of(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.ino()
}

#[cfg(not(unix))]
fn inode_of(_meta: &std::fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn device_of(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.dev()
}

#[cfg(not(unix))]
fn device_of(_meta: &std::fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn hard_link_count(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.nlink().min(u32::MAX as u64) as u32
}

#[cfg(not(unix))]
fn hard_link_count(_meta: &std::fs::Metadata) -> u32 {
    1
}

/// 便捷入口：遍历全部根并汇总。
pub fn walk_roots(
    roots: &[String],
    engine: &FilterEngine,
    ctx: &mut WalkContext<'_>,
) -> AppResult<WalkOutcome> {
    let mut out = WalkOutcome {
        entries: Vec::new(),
        stats: WalkStats::default(),
        skipped_examples: Vec::new(),
    };

    let mut scanned_roots = 0usize;
    let mut last_error: Option<AppError> = None;

    for root in roots {
        let path = PathBuf::from(root);
        if let Err(e) = walk_root(&path, engine, ctx, &mut out) {
            match e {
                AppError::Cancelled => return Err(e),
                // 单个根不可访问不应该让整次扫描失败，记录后继续下一个根。
                other => {
                    tracing::warn!(root = %root, error = %other, "跳过无法扫描的根目录");
                    out.stats.inaccessible_paths += 1;
                    if out.skipped_examples.len() < 50 {
                        out.skipped_examples.push((root.clone(), "不可访问"));
                    }
                    last_error = Some(other);
                }
            }
        } else {
            scanned_roots += 1;
        }
        if out.stats.truncated {
            break;
        }
    }

    // 所有根都被拒绝时必须显式报错 —— 否则用户会看到「扫描成功但 0 个文件」，
    // 误以为自己真的没有重复文件。
    if scanned_roots == 0 {
        if let Some(err) = last_error {
            return Err(err);
        }
    }

    Ok(out)
}
