//! 动作层：把「待清理」的决策安全地落盘。
//!
//! 三条不可逾越的红线：
//! 1. **无确认不执行** —— `plan.confirmed == false` 且非演练，直接拒绝；
//! 2. **不越界** —— 目标必须位于本次扫描根之内，且不落在系统保护目录；
//! 3. **可追溯** —— 每次执行都写审计流水（SQLite + JSON 文件），移动类操作可一键撤销。

use std::path::{Path, PathBuf};

use crate::config::AppConfig;
use crate::domain::models::{
    ActionOutcome, ActionPlan, ActionResult, ActionTarget, DeleteMode,
};
use crate::error::{AppError, AppResult};
use crate::scan::filter::ProtectedPaths;

pub struct ActionContext<'a> {
    pub config: &'a AppConfig,
    /// 本次会话允许操作的范围（扫描根）。
    pub allowed_roots: &'a [String],
    pub protected: &'a ProtectedPaths,
}

pub fn execute(plan: &ActionPlan, ctx: &ActionContext<'_>) -> AppResult<ActionResult> {
    if !plan.confirmed && !plan.dry_run {
        return Err(AppError::forbidden(
            "缺少显式确认：真实执行必须由前端传入 confirmed = true",
        ));
    }
    if plan.targets.is_empty() {
        return Err(AppError::config("没有需要处理的目标文件"));
    }

    let move_dir = resolve_move_dir(plan, ctx)?;

    // 扫描根统一规范化：目标路径在 prepare_target 里已被 canonicalize，
    // 若这里仍用原始根做前缀比较，遇到 /var → /private/var 这类软链会算错相对路径。
    let roots = canonical_roots(ctx.allowed_roots);

    let mut outcomes: Vec<ActionOutcome> = Vec::with_capacity(plan.targets.len());

    for target in &plan.targets {
        // 压缩包内部条目走独立通道（重写压缩包），不走文件删除。
        if target.archive_inner_path.is_some() {
            outcomes.push(ActionOutcome {
                path: target.path.clone(),
                success: false,
                error: Some("压缩包内部条目请使用「移除压缩包条目」操作".into()),
                freed_bytes: 0,
                moved_to: None,
            });
            continue;
        }

        let prepared = prepare_target(target, &roots, ctx.protected);
        let path = match prepared {
            Ok(p) => p,
            Err(e) => {
                outcomes.push(ActionOutcome {
                    path: target.path.clone(),
                    success: false,
                    error: Some(e.to_string()),
                    freed_bytes: 0,
                    moved_to: None,
                });
                continue;
            }
        };

        if plan.dry_run {
            outcomes.push(ActionOutcome {
                path: target.path.clone(),
                success: true,
                error: None,
                freed_bytes: target.size,
                moved_to: Some(format!("[演练] 将以「{}」方式处理", plan.mode.label())),
            });
            continue;
        }

        let result = match plan.mode {
            DeleteMode::Trash => do_trash(&path, ctx.config.trash_dir.as_deref()),
            DeleteMode::HardlinkReplace => do_hardlink_replace(&path, target),
            DeleteMode::MoveTo => do_move(&path, &move_dir, &roots),
        };

        match result {
            Ok(moved_to) => outcomes.push(ActionOutcome {
                path: target.path.clone(),
                success: true,
                error: None,
                freed_bytes: target.size,
                moved_to,
            }),
            Err(e) => outcomes.push(ActionOutcome {
                path: target.path.clone(),
                success: false,
                error: Some(e),
                freed_bytes: 0,
                moved_to: None,
            }),
        }
    }

    let succeeded = outcomes.iter().filter(|o| o.success).count() as u64;
    let failed = outcomes.len() as u64 - succeeded;
    let freed_bytes = outcomes.iter().filter(|o| o.success).map(|o| o.freed_bytes).sum();

    Ok(ActionResult {
        journal_id: None,
        mode: plan.mode,
        dry_run: plan.dry_run,
        outcomes,
        succeeded,
        failed,
        freed_bytes,
    })
}

/// 把扫描根规范化为绝对、去符号链接的形式（保留原路径作为兜底）。
fn canonical_roots(roots: &[String]) -> Vec<PathBuf> {
    roots
        .iter()
        .flat_map(|root| {
            let raw = PathBuf::from(root);
            match raw.canonicalize() {
                Ok(canonical) if canonical != raw => vec![canonical, raw],
                _ => vec![raw],
            }
        })
        .collect()
}

/// 目标校验 + 归一化。
fn prepare_target(
    target: &ActionTarget,
    roots: &[PathBuf],
    protected: &ProtectedPaths,
) -> AppResult<PathBuf> {
    let path = PathBuf::from(&target.path);

    if !path.is_absolute() {
        return Err(AppError::forbidden(format!("拒绝处理非绝对路径：{}", target.path)));
    }

    let meta = std::fs::symlink_metadata(&path)
        .map_err(|e| AppError::NotFound(format!("{}（{e}）", target.path)))?;

    if meta.file_type().is_symlink() {
        return Err(AppError::forbidden(format!(
            "拒绝处理符号链接本身：{}（请手动确认）",
            target.path
        )));
    }
    if meta.is_dir() {
        return Err(AppError::forbidden(format!("目标不是普通文件：{}", target.path)));
    }

    if protected.is_protected(&path) {
        return Err(AppError::forbidden(format!("目标位于系统保护目录内：{}", target.path)));
    }

    let canonical = path
        .canonicalize()
        .map_err(|e| AppError::NotFound(format!("{}（{e}）", target.path)))?;

    let in_scope = roots.iter().any(|root| canonical.starts_with(root));
    if !in_scope {
        return Err(AppError::forbidden(format!(
            "目标不在本次扫描范围内，已拒绝：{}",
            target.path
        )));
    }

    Ok(canonical)
}

fn resolve_move_dir(plan: &ActionPlan, ctx: &ActionContext<'_>) -> AppResult<PathBuf> {
    if plan.mode != DeleteMode::MoveTo {
        return Ok(ctx.config.quarantine_dir.clone());
    }
    let dir = match &plan.move_dir {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d),
        _ => ctx.config.quarantine_dir.clone(),
    };
    if ctx.protected.is_protected(&dir) {
        return Err(AppError::forbidden("隔离目录不能位于系统保护目录内"));
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

// ============================ 三种落地方式 ============================

/// 移入回收站。
///
/// 显式指定了回收目录（`FILECLEAN_TRASH_DIR`）就完全不碰系统回收站：落点确定、
/// 可读回，也是测试能把这一步完全沙箱化的唯一办法。
///
/// 未指定时：
/// - **macOS 走 `NSFileManager` 的原生回收站 API（`trashItemAtURL`）**，不借道
///   Finder/osascript。`trash` crate 的默认是 `DeleteMethod::Finder`，它会
///   `osascript` 调 Finder 来删 —— 这正是「逐个文件播放系统删除音效、且更慢」的来源；
///   `NsFileManager` 是进程内原生调用，无声且快。
/// - 其它平台（Windows/Linux）走系统原生回收站，本身即静默、快速。
///
/// 任一路径失败都退化为「移动到用户回收站目录」，仍然可恢复，符合「默认不永久删除」。
fn do_trash(path: &Path, explicit_dir: Option<&Path>) -> Result<Option<String>, String> {
    // 显式指定了回收目录就完全不碰系统回收站：落点确定、可读回，
    // 也是测试能把这一步完全沙箱化的唯一办法。
    if let Some(dir) = explicit_dir {
        let dest = move_into(dir.to_path_buf(), path)?;
        return Ok(Some(format!("已按指定回收目录放入 {dest}")));
    }

    #[cfg(target_os = "macos")]
    {
        match macos_trash(path) {
            Ok(()) => Ok(None),
            Err(primary) => match fallback_trash(path) {
                Ok(dest) => Ok(Some(format!(
                    "系统回收站不可用（{primary}），已改放入 {dest}"
                ))),
                Err(fallback_err) => Err(format!(
                    "移入回收站失败：{primary}；备用方案也失败：{fallback_err}"
                )),
            },
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        match trash::delete(path) {
            Ok(()) => Ok(None),
            Err(primary) => match fallback_trash(path) {
                Ok(dest) => Ok(Some(format!(
                    "系统回收站不可用（{primary}），已改放入 {dest}"
                ))),
                Err(fallback_err) => Err(format!(
                    "移入回收站失败：{primary}；备用方案也失败：{fallback_err}"
                )),
            },
        }
    }
}

/// macOS 上用 `NSFileManager` 的原生回收站（`trashItemAtURL`）把文件移入废纸篓。
///
/// 关键：必须显式选 `DeleteMethod::NsFileManager`。`trash` crate 的默认是
/// `DeleteMethod::Finder`，它 `osascript` 调 Finder 来删 —— 既逐个文件播放系统删除
/// 音效、又慢。`NsFileManager` 是进程内原生调用，无声且快，也不要求 Finder 自动化授权。
#[cfg(target_os = "macos")]
fn macos_trash(path: &Path) -> Result<(), String> {
    use trash::macos::TrashContextExtMacos;
    let mut ctx = trash::TrashContext::new();
    ctx.set_delete_method(trash::macos::DeleteMethod::NsFileManager);
    ctx.delete(path).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn fallback_trash(path: &Path) -> Result<String, String> {
    let home = std::env::var_os("HOME").ok_or_else(|| "无法确定 HOME 目录".to_string())?;
    move_into(PathBuf::from(home).join(".Trash"), path)
}

#[cfg(target_os = "linux")]
fn fallback_trash(path: &Path) -> Result<String, String> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| "无法确定用户数据目录".to_string())?;
    move_into(base.join("Trash/files"), path)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn fallback_trash(_path: &Path) -> Result<String, String> {
    Err("当前平台没有可用的回收站备用方案".into())
}

/// 把文件挪进指定目录（同名时自动改名），跨设备时退化为复制 + 删除。
fn move_into(dir: PathBuf, path: &Path) -> Result<String, String> {
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建回收站目录失败：{e}"))?;
    let name = path
        .file_name()
        .ok_or_else(|| "无法确定文件名".to_string())?;
    let dest = unique_path(&dir.join(name));
    if std::fs::rename(path, &dest).is_err() {
        std::fs::copy(path, &dest).map_err(|e| format!("复制到回收站失败：{e}"))?;
        std::fs::remove_file(path).map_err(|e| format!("删除原文件失败：{e}"))?;
    }
    Ok(dest.to_string_lossy().to_string())
}

/// 硬链接替换：先把保留文件链接到目标旁的临时名，再原子 rename 覆盖目标。
///
/// 这样即使中途失败，目标路径也不会出现「文件没了但链接没建上」的空窗。
fn do_hardlink_replace(path: &Path, target: &ActionTarget) -> Result<Option<String>, String> {
    let keep = target
        .keep_path
        .as_ref()
        .ok_or_else(|| "硬链接模式缺少保留文件路径".to_string())?;
    let keep_path = PathBuf::from(keep);

    if !keep_path.exists() {
        return Err(format!("保留文件不存在：{keep}"));
    }
    if same_filesystem(&keep_path, path).is_err() {
        return Err("保留文件与副本不在同一文件系统，无法建立硬链接".into());
    }

    let tmp = path.with_file_name(format!(
        ".{}.fileclean-link-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));

    std::fs::hard_link(&keep_path, &tmp).map_err(|e| format!("创建硬链接失败：{e}"))?;

    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(Some(format!("已替换为指向 {keep} 的硬链接"))),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(format!("替换失败：{e}"))
        }
    }
}

fn do_move(path: &Path, move_dir: &Path, roots: &[PathBuf]) -> Result<Option<String>, String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let bucket = move_dir.join(stamp.to_string());
    std::fs::create_dir_all(&bucket).map_err(|e| format!("创建隔离目录失败：{e}"))?;

    // 保留原有目录结构（相对扫描根），方便用户对照还原。
    let relative = roots
        .iter()
        .filter_map(|root| path.strip_prefix(root).ok())
        .find(|rel| !rel.as_os_str().is_empty())
        .map(|rel| rel.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(path.file_name().unwrap_or_default()));

    let mut dest = bucket.join(&relative);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目标目录失败：{e}"))?;
    }
    dest = unique_path(&dest);

    match std::fs::rename(path, &dest) {
        Ok(()) => Ok(Some(dest.to_string_lossy().to_string())),
        Err(_) => {
            // 跨设备：退化为复制 + 删除。
            std::fs::copy(path, &dest).map_err(|e| format!("复制到隔离区失败：{e}"))?;
            std::fs::remove_file(path).map_err(|e| format!("删除原文件失败：{e}"))?;
            Ok(Some(dest.to_string_lossy().to_string()))
        }
    }
}

fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = path.extension().map(|e| e.to_string_lossy().to_string());
    let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();

    for i in 1..10_000 {
        let name = match &ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

#[cfg(unix)]
fn same_filesystem(a: &Path, b: &Path) -> Result<(), ()> {
    use std::os::unix::fs::MetadataExt;
    let (ma, mb) = (
        std::fs::metadata(a).map_err(|_| ())?,
        std::fs::metadata(b).map_err(|_| ())?,
    );
    if ma.dev() == mb.dev() {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(not(unix))]
fn same_filesystem(_a: &Path, _b: &Path) -> Result<(), ()> {
    // Windows 上无法廉价判断卷标，交由 hard_link 自身报错。
    Ok(())
}

// ============================ 撤销 ============================

/// 撤销一次「移入隔离区」操作。回收站模式依赖系统自身的还原能力，此处不支持。
pub fn undo(targets: &[(String, String)]) -> (u64, Vec<String>) {
    let mut restored = 0u64;
    let mut errors = Vec::new();

    for (original, moved) in targets {
        let from = Path::new(moved);
        let to = Path::new(original);
        if !from.exists() {
            errors.push(format!("隔离文件已不存在：{moved}"));
            continue;
        }
        if let Some(parent) = to.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                errors.push(format!("恢复目录失败 {}：{e}", parent.display()));
                continue;
            }
        }
        if to.exists() {
            errors.push(format!("原位置已有同名文件，跳过：{original}"));
            continue;
        }
        match std::fs::rename(from, to) {
            Ok(()) => restored += 1,
            Err(e) => errors.push(format!("恢复失败 {original}：{e}")),
        }
    }

    (restored, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn ctx<'a>(config: &'a AppConfig, roots: &'a [String], protected: &'a ProtectedPaths) -> ActionContext<'a> {
        ActionContext {
            config,
            allowed_roots: roots,
            protected,
        }
    }

    fn temp_config() -> AppConfig {
        let dir = tempfile::tempdir().unwrap();
        // 让目录活到测试结束：泄漏一个 TempDir 是测试里可接受的代价。
        let base = dir.keep();
        AppConfig {
            data_dir: base.clone(),
            db_path: base.join("db.sqlite3"),
            log_dir: base.join("logs"),
            quarantine_dir: base.join("quarantine"),
            export_dir: base.join("exports"),
            trash_dir: None,
            log_level: "error".into(),
            log_json: false,
            default_threads: 1,
            max_scan_files: 100,
            cache_max_age_days: 30,
            protect_system_dirs: false,
            default_delete_mode: DeleteMode::MoveTo,
            max_full_hash_bytes: 1024,
        }
    }

    #[test]
    fn refuses_without_confirmation() {
        let config = temp_config();
        let protected = ProtectedPaths::new(false);
        let roots = vec![config.data_dir.to_string_lossy().to_string()];
        let plan = ActionPlan {
            mode: DeleteMode::Trash,
            move_dir: None,
            dry_run: false,
            targets: vec![ActionTarget {
                path: "/tmp/nonexistent".into(),
                keep_path: None,
                group_id: "g".into(),
                size: 1,
                archive_inner_path: None,
            }],
            confirmed: false,
        };
        let err = execute(&plan, &ctx(&config, &roots, &protected)).unwrap_err();
        assert_eq!(err.code(), "FORBIDDEN");
    }

    #[test]
    fn refuses_targets_outside_scan_roots() {
        let config = temp_config();
        let protected = ProtectedPaths::new(false);
        let other = tempfile::tempdir().unwrap();
        let victim = other.path().join("victim.txt");
        std::fs::write(&victim, b"data").unwrap();

        let roots = vec![config.data_dir.to_string_lossy().to_string()];
        let plan = ActionPlan {
            mode: DeleteMode::MoveTo,
            move_dir: None,
            dry_run: false,
            targets: vec![ActionTarget {
                path: victim.to_string_lossy().to_string(),
                keep_path: None,
                group_id: "g".into(),
                size: 4,
                archive_inner_path: None,
            }],
            confirmed: true,
        };
        let result = execute(&plan, &ctx(&config, &roots, &protected)).unwrap();
        assert_eq!(result.succeeded, 0);
        assert!(result.outcomes[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("不在本次扫描范围"));
        assert!(victim.exists(), "越界文件必须原封不动");
    }

    #[test]
    fn move_to_quarantine_preserves_relative_structure() {
        let config = temp_config();
        let protected = ProtectedPaths::new(false);
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("photos/2024");
        std::fs::create_dir_all(&nested).unwrap();
        let victim = nested.join("dup.jpg");
        std::fs::File::create(&victim)
            .unwrap()
            .write_all(b"payload")
            .unwrap();

        let roots = vec![root.path().to_string_lossy().to_string()];
        let plan = ActionPlan {
            mode: DeleteMode::MoveTo,
            move_dir: Some(config.quarantine_dir.to_string_lossy().to_string()),
            dry_run: false,
            targets: vec![ActionTarget {
                path: victim.to_string_lossy().to_string(),
                keep_path: None,
                group_id: "g".into(),
                size: 7,
                archive_inner_path: None,
            }],
            confirmed: true,
        };

        let result = execute(&plan, &ctx(&config, &roots, &protected)).unwrap();
        assert_eq!(result.succeeded, 1);
        assert!(!victim.exists());
        let moved = result.outcomes[0].moved_to.clone().unwrap();
        assert!(moved.ends_with("photos/2024/dup.jpg"), "实际落点：{moved}");

        // 撤销应把它放回原位。
        let (restored, errors) = undo(&[(plan.targets[0].path.clone(), moved)]);
        assert_eq!(restored, 1, "撤销失败：{errors:?}");
        assert!(victim.exists());
    }

    #[test]
    fn dry_run_touches_nothing() {
        let config = temp_config();
        let protected = ProtectedPaths::new(false);
        let root = tempfile::tempdir().unwrap();
        let victim = root.path().join("keep.txt");
        std::fs::write(&victim, b"x").unwrap();

        let roots = vec![root.path().to_string_lossy().to_string()];
        let plan = ActionPlan {
            mode: DeleteMode::Trash,
            move_dir: None,
            dry_run: true,
            targets: vec![ActionTarget {
                path: victim.to_string_lossy().to_string(),
                keep_path: None,
                group_id: "g".into(),
                size: 1,
                archive_inner_path: None,
            }],
            confirmed: false,
        };
        let result = execute(&plan, &ctx(&config, &roots, &protected)).unwrap();
        assert!(result.dry_run);
        assert_eq!(result.succeeded, 1);
        assert!(victim.exists(), "演练模式绝不能动到文件");
    }

    /// 回收站备用方案的核心搬迁逻辑：同名不覆盖、内容不丢。
    #[test]
    fn move_into_relocates_without_clobbering() {
        let trash = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();

        let first = src.path().join("a.txt");
        std::fs::write(&first, b"first").unwrap();
        let dest1 = move_into(trash.path().to_path_buf(), &first).unwrap();
        assert!(!first.exists(), "搬迁后原位置不应残留");
        assert_eq!(std::fs::read(&dest1).unwrap(), b"first");

        // 同名第二次进入：必须自动改名而不是覆盖既有文件。
        let second = src.path().join("a.txt");
        std::fs::write(&second, b"second").unwrap();
        let dest2 = move_into(trash.path().to_path_buf(), &second).unwrap();
        assert_ne!(dest1, dest2, "同名文件应被重命名");
        assert_eq!(std::fs::read(&dest2).unwrap(), b"second");
        assert_eq!(std::fs::read(&dest1).unwrap(), b"first", "先到的文件不能被覆盖");
    }

    /// 清理动作失败时必须**如实报错**，而不是静默当作成功。
    ///
    /// 这里刻意对**不存在的路径**调用 `do_trash(path, None)` —— 对真实文件调用会把
    /// 测试产物丢进用户自己的回收站。空路径足以覆盖「原生回收站」与「备用方案」
    /// 两条错误分支。
    #[test]
    fn trash_reports_error_when_nothing_to_move() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("definitely-not-here.txt");
        let err = do_trash(&missing, None).expect_err("对不存在的文件必须返回错误");
        assert!(!err.is_empty());
        assert!(dir.path().exists(), "不应牵连其它内容");
    }

    /// 指定回收目录时，文件必须真的搬过去，且内容逐字节不变。
    ///
    /// 这条断言以前写不出来：走系统回收站时落点取决于运行环境
    /// （macOS 上 `~/.Trash` 还受 TCC 保护、测试进程无权列举），
    /// 只能靠「原生路径恰好失败」凑巧通过 —— 属于依赖环境的假阳性。
    #[test]
    fn explicit_trash_dir_is_used_and_content_survives() {
        let src = tempfile::tempdir().unwrap();
        let trash = tempfile::tempdir().unwrap();
        let victim = src.path().join("dup.txt");
        std::fs::write(&victim, b"important payload").unwrap();

        let note = do_trash(&victim, Some(trash.path()))
            .expect("指定了回收目录就必须成功")
            .expect("应返回落点说明");
        assert!(note.contains("指定回收目录"), "应说明用的是显式目录：{note}");

        assert!(!victim.exists(), "成功后原位置必须清空");

        let moved: Vec<_> = std::fs::read_dir(trash.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(moved.len(), 1, "应恰好移动一个文件");
        assert_eq!(
            std::fs::read(moved[0].path()).unwrap(),
            b"important payload",
            "内容必须逐字节保留 —— 这是「默认不永久删除」的底线"
        );
    }
}
