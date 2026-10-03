//! 动作命令：执行清理、撤销、压缩包条目移除、系统级打开。

use std::path::{Path, PathBuf};

use tauri::State;

use crate::actions::{self, ActionContext};
use crate::archive;
use crate::commands::session_mut;
use crate::domain::models::{ActionPlan, ActionResult, MarkDecision, ScanResult};
use crate::error::{AppError, AppResult};
use crate::scan::filter::ProtectedPaths;
use crate::state::AppState;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct JournalItem {
    path: String,
    moved_to: Option<String>,
    group_id: String,
    freed_bytes: u64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Journal {
    id: String,
    created_at: i64,
    mode: String,
    dry_run: bool,
    items: Vec<JournalItem>,
    /// 撤销状态：MoveTo 模式可撤销，其它模式为 false 并附带原因。
    undoable: bool,
    undo_hint: String,
}

/// 执行清理计划。
///
/// 前端的 `plan.confirmed` 是硬性门槛：没有它，动作层直接拒绝。
#[tauri::command]
pub fn execute_actions(
    state: State<'_, AppState>,
    scan_id: String,
    plan: ActionPlan,
) -> AppResult<ActionResult> {
    // 先取出扫描上下文（根目录范围），再执行动作。
    let (allowed_roots, _) = {
        let sessions = state.sessions.lock();
        let session = sessions
            .get(&scan_id)
            .ok_or_else(|| AppError::UnknownScan(scan_id.clone()))?;
        (session.options.roots.clone(), ())
    };

    let protected = ProtectedPaths::new(state.config.protect_system_dirs);
    let ctx = ActionContext {
        config: &state.config,
        allowed_roots: &allowed_roots,
        protected: &protected,
    };

    let mut outcome = actions::execute(&plan, &ctx)?;

    if !plan.dry_run && outcome.succeeded > 0 {
        let journal_id = uuid::Uuid::new_v4().to_string();
        let undoable = plan.mode == crate::domain::models::DeleteMode::MoveTo;
        let journal = Journal {
            id: journal_id.clone(),
            created_at: chrono::Utc::now().timestamp(),
            mode: plan.mode.as_str().to_string(),
            dry_run: false,
            items: outcome
                .outcomes
                .iter()
                .filter(|o| o.success)
                .map(|o| JournalItem {
                    path: o.path.clone(),
                    moved_to: o.moved_to.clone(),
                    group_id: plan
                        .targets
                        .iter()
                        .find(|t| t.path == o.path)
                        .map(|t| t.group_id.clone())
                        .unwrap_or_default(),
                    freed_bytes: o.freed_bytes,
                })
                .collect(),
            undoable,
            undo_hint: if undoable {
                "文件已移入隔离区，可一键撤销还原".into()
            } else {
                match plan.mode {
                    crate::domain::models::DeleteMode::Trash => {
                        "文件已移入系统废纸篓，请从废纸篓还原".into()
                    }
                    _ => "硬链接替换不可逆（原路径已指向保留文件）".into(),
                }
            },
        };

        write_journal(&state, &journal)?;

        // 回写结果：把已成功处理的文件从分组中剔除，UI 立刻反映真实状态。
        let succeeded_paths: Vec<String> = outcome
            .outcomes
            .iter()
            .filter(|o| o.success)
            .map(|o| o.path.clone())
            .collect();

        let _ = session_mut(&state, &scan_id, |session| {
            if let Some(result) = session.result.as_mut() {
                prune_removed(result, &succeeded_paths);
                crate::commands::result_cmd::recompute_stats(result);
            }
            Ok(())
        });

        outcome.journal_id = Some(journal_id);
    }

    tracing::info!(
        mode = plan.mode.as_str(),
        dry_run = plan.dry_run,
        succeeded = outcome.succeeded,
        failed = outcome.failed,
        freed = outcome.freed_bytes,
        "清理动作完成"
    );

    Ok(outcome)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoResult {
    pub restored: u64,
    pub errors: Vec<String>,
    pub hint: String,
}

#[tauri::command]
pub fn undo_action(state: State<'_, AppState>, journal_id: String) -> AppResult<UndoResult> {
    let raw = state
        .db
        .read(|conn| crate::db::session_repo::get_journal(conn, &journal_id))?
        .ok_or_else(|| AppError::NotFound(format!("操作流水 {journal_id}")))?;

    let journal: Journal = serde_json::from_str(&raw)?;

    if !journal.undoable {
        return Ok(UndoResult {
            restored: 0,
            errors: vec![journal.undo_hint.clone()],
            hint: journal.undo_hint,
        });
    }

    let pairs: Vec<(String, String)> = journal
        .items
        .iter()
        .filter_map(|i| i.moved_to.clone().map(|m| (i.path.clone(), m)))
        .collect();

    let (restored, errors) = actions::undo(&pairs);

    state
        .db
        .write(|tx| crate::db::session_repo::mark_journal_undone(tx, &journal_id))?;

    Ok(UndoResult {
        restored,
        errors,
        hint: format!("已还原 {restored} 个文件"),
    })
}

/// 移除压缩包内部条目（重写压缩包，原包原子替换）。
#[tauri::command]
pub fn remove_archive_entries(
    archive_path: String,
    inner_paths: Vec<String>,
    confirmed: bool,
) -> AppResult<u64> {
    if !confirmed {
        return Err(AppError::forbidden("缺少显式确认，已拒绝重写压缩包"));
    }
    if inner_paths.is_empty() {
        return Err(AppError::config("没有需要移除的压缩包条目"));
    }
    let removed = archive::remove_zip_entries(Path::new(&archive_path), &inner_paths)
        .map_err(AppError::internal)?;
    Ok(removed)
}

/// 在系统文件管理器中定位文件；文件已不存在时退回到打开其父目录。
#[tauri::command]
pub fn reveal_in_finder(path: String) -> AppResult<()> {
    let target = PathBuf::from(&path);
    let existing = if target.exists() {
        target.clone()
    } else {
        target
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| AppError::NotFound(path.clone()))?
    };

    #[cfg(target_os = "macos")]
    {
        let is_dir = existing.is_dir();
        let mut cmd = std::process::Command::new("open");
        if !is_dir {
            cmd.arg("-R");
        }
        cmd.arg(&existing);
        cmd.spawn()
            .map_err(|e| AppError::internal(format!("无法打开访达：{e}")))?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(existing)
            .spawn()
            .map_err(|e| AppError::internal(format!("无法打开资源管理器：{e}")))?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(existing)
            .spawn()
            .map_err(|e| AppError::internal(format!("无法打开文件管理器：{e}")))?;
        Ok(())
    }
}

/// 取消某个文件/分组的待清理标记（UI 快捷操作）。
#[tauri::command]
pub fn keep_file(
    state: State<'_, AppState>,
    scan_id: String,
    group_id: String,
    path: String,
) -> AppResult<ScanResult> {
    crate::commands::result_cmd::set_file_decision(
        state,
        scan_id,
        group_id,
        path,
        MarkDecision::Keep,
    )
}

/// 估算执行计划的影响面，供 UI 展示确认弹窗内容（不做任何实际改动）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanPreview {
    pub count: u64,
    pub bytes: u64,
    pub human: String,
    pub mode_label: String,
    pub mode_description: String,
    /// 会被拒绝的目标及原因 —— 在用户点击「执行」之前就告诉他哪些动不了。
    pub blocked: Vec<String>,
}

#[tauri::command]
pub fn preview_plan(state: State<'_, AppState>, plan: ActionPlan) -> AppResult<PlanPreview> {
    let protected = ProtectedPaths::new(state.config.protect_system_dirs);
    let mut blocked = Vec::new();
    let mut allowed_bytes = 0u64;

    for target in &plan.targets {
        let path = PathBuf::from(&target.path);
        if !path.is_absolute() {
            blocked.push(format!("{}：不是绝对路径", target.path));
            continue;
        }
        if protected.is_protected(&path) {
            blocked.push(format!("{}：位于系统保护目录内", target.path));
            continue;
        }
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                blocked.push(format!("{}：符号链接本身不予处理", target.path));
            }
            Ok(meta) if meta.is_dir() => {
                blocked.push(format!("{}：目标是目录", target.path));
            }
            Ok(_) => allowed_bytes += target.size,
            Err(_) => blocked.push(format!("{}：文件不存在或无法访问", target.path)),
        }
    }

    Ok(PlanPreview {
        count: plan.targets.len() as u64,
        bytes: allowed_bytes,
        human: crate::pipeline::human_bytes(allowed_bytes),
        mode_label: plan.mode.label().to_string(),
        mode_description: plan.mode.description().to_string(),
        blocked,
    })
}

fn write_journal(state: &AppState, journal: &Journal) -> AppResult<()> {
    let payload = serde_json::to_string(journal)?;

    state.db.write(|tx| {
        crate::db::session_repo::record_journal(
            tx,
            &journal.id,
            &journal.mode,
            journal.dry_run,
            journal.items.len() as u64,
            0,
            journal.items.iter().map(|i| i.freed_bytes).sum(),
            &payload,
        )
    })?;

    let dir = state.config.data_dir.join("journals");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{}.json", journal.id)), payload)?;
    Ok(())
}

/// 把已成功处理的路径从结果中移除，并清理空分组。
fn prune_removed(result: &mut ScanResult, removed: &[String]) {
    use std::collections::HashSet;
    let removed: HashSet<&str> = removed.iter().map(|s| s.as_str()).collect();

    for group in &mut result.groups {
        group
            .files
            .retain(|f| !removed.contains(f.entry.path.as_str()));
        group.recompute();
    }
    result
        .groups
        .retain(|g| g.kind == crate::domain::models::MatchKind::RawJpegPair || g.files.len() >= 2);
}
