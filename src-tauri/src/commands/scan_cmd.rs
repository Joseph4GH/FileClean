//! 扫描控制命令。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::{self, status_of};
use crate::domain::models::{AutoMarkConfig, ScanOptions, ScanPhase, ScanProgress, ScanStatus};
use crate::error::{AppError, AppResult};
use crate::pipeline;
use crate::state::{AppState, ScanSession, ScanStatusPayload};
use crate::state::sweep_finished;

pub const EVENT_PROGRESS: &str = "scan://progress";
pub const EVENT_DONE: &str = "scan://done";
pub const EVENT_ERROR: &str = "scan://error";

/// 进度事件的最小间隔，防止哈希阶段的事件把 WebView 压垮。
const EMIT_INTERVAL_MS: u64 = 90;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanDonePayload {
    scan_id: String,
    groups: u64,
    wasted_bytes: u64,
    duration_ms: u64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanErrorPayload {
    scan_id: String,
    code: String,
    message: String,
}

/// 启动一次后台扫描，立刻返回 `scanId`。
///
/// 参数校验同步完成 —— 配置错误会当场抛出，用户不必等到进度条跑起来才发现。
#[tauri::command]
pub fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    options: ScanOptions,
    mark_config: Option<AutoMarkConfig>,
) -> AppResult<String> {
    options.validate()?;
    sweep_finished(&state);

    let scan_id = uuid::Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let mark_config = mark_config.unwrap_or_default();

    {
        let mut sessions = state.sessions.lock();
        sessions.insert(
            scan_id.clone(),
            ScanSession::new(&scan_id, options.clone(), mark_config.clone(), cancel.clone()),
        );
    }

    let handle = app.clone();
    let thread_id = scan_id.clone();

    std::thread::Builder::new()
        .name(format!("fileclean-scan-{}", &scan_id[..8]))
        .spawn(move || {
            let state = handle.state::<AppState>();
            let config = state.config.clone();
            let db = state.db.clone();
            let started = now_millis();

            let last_emit = AtomicU64::new(0);
            // 闭包需要 `'static` 生命周期之外的所有权，因此克隆句柄与 ID 进去。
            let sink_handle = handle.clone();
            let sink_id = thread_id.clone();
            let sink = move |mut p: ScanProgress| {
                let now = now_millis();
                let last = last_emit.load(Ordering::Relaxed);
                let terminal = p.phase.is_terminal();
                if !terminal && now.saturating_sub(last) < EMIT_INTERVAL_MS {
                    return;
                }
                last_emit.store(now, Ordering::Relaxed);
                p.scan_id = sink_id.clone();
                p.phase_label = p.phase.label().to_string();
                {
                    let state = sink_handle.state::<AppState>();
                    let mut sessions = state.sessions.lock();
                    if let Some(session) = sessions.get_mut(&sink_id) {
                        session.progress = p.clone();
                    }
                }
                let _ = sink_handle.emit(EVENT_PROGRESS, &p);
            };

            let outcome = pipeline::run_scan(
                &thread_id,
                &options,
                &mark_config,
                &db,
                &config,
                &cancel,
                &sink,
            );

            {
                let state = handle.state::<AppState>();
                let mut sessions = state.sessions.lock();
                if let Some(session) = sessions.get_mut(&thread_id) {
                    match outcome {
                        Ok(out) => {
                            let mut progress = session.progress.clone();
                            progress.phase = ScanPhase::Done;
                            progress.phase_label = ScanPhase::Done.label().to_string();
                            progress.percent = 1.0;
                            progress.elapsed_ms = out.result.stats.duration_ms;
                            progress.groups_found = out.result.stats.groups;
                            progress.message = format!(
                                "完成：{} 个分组，可回收 {}",
                                out.result.stats.groups,
                                pipeline::human_bytes(out.result.stats.wasted_bytes)
                            );
                            session.progress = progress;
                            session.status = ScanStatus::Done;
                            session.result = Some(out.result);
                        }
                        Err(AppError::Cancelled) => {
                            session.status = ScanStatus::Cancelled;
                            session.progress.phase = ScanPhase::Cancelled;
                            session.progress.message = "扫描已取消".into();
                        }
                        Err(e) => {
                            session.status = ScanStatus::Failed;
                            session.progress.phase = ScanPhase::Failed;
                            session.progress.message = e.to_string();
                            session.error = Some(e.to_string());
                        }
                    }
                }
            }

            // 事件在会话状态更新之后发出，前端收到即可直接取结果。
            {
                let state = handle.state::<AppState>();
                let sessions = state.sessions.lock();
                if let Some(session) = sessions.get(&thread_id) {
                    match session.status {
                        ScanStatus::Done => {
                            if let Some(result) = &session.result {
                                let _ = handle.emit(
                                    EVENT_DONE,
                                    &ScanDonePayload {
                                        scan_id: thread_id.clone(),
                                        groups: result.stats.groups,
                                        wasted_bytes: result.stats.wasted_bytes,
                                        duration_ms: result.stats.duration_ms,
                                    },
                                );
                            }
                        }
                        ScanStatus::Failed => {
                            let _ = handle.emit(
                                EVENT_ERROR,
                                &ScanErrorPayload {
                                    scan_id: thread_id.clone(),
                                    code: "SCAN_FAILED".into(),
                                    message: session
                                        .error
                                        .clone()
                                        .unwrap_or_else(|| "扫描失败".into()),
                                },
                            );
                        }
                        _ => {}
                    }
                }
            }

            tracing::info!(
                scan_id = %thread_id,
                elapsed_ms = now_millis().saturating_sub(started),
                "扫描线程结束"
            );
        })
        .map_err(|e| AppError::internal(format!("无法启动扫描线程：{e}")))?;

    Ok(scan_id)
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>, scan_id: String) -> AppResult<()> {
    commands::session_ref(&state, &scan_id, |s| {
        s.cancel.store(true, Ordering::Relaxed);
        Ok(())
    })
}

#[tauri::command]
pub fn get_scan_status(state: State<'_, AppState>, scan_id: String) -> AppResult<ScanStatusPayload> {
    status_of(&state, &scan_id)
}

/// 取扫描结果。仅在扫描真正结束时返回，避免前端拿到半成品。
#[tauri::command]
pub fn get_scan_result(
    state: State<'_, AppState>,
    scan_id: String,
) -> AppResult<crate::domain::models::ScanResult> {
    commands::finished_result(&state, &scan_id)
}

#[tauri::command]
pub fn list_scans(state: State<'_, AppState>) -> AppResult<Vec<ScanStatusPayload>> {
    let sessions = state.sessions.lock();
    Ok(sessions
        .iter()
        .map(|(id, s)| ScanStatusPayload {
            scan_id: id.clone(),
            status: s.status,
            progress: s.progress.clone(),
            error: s.error.clone(),
            has_result: s.result.is_some(),
        })
        .collect())
}

/// 默认扫描选项 —— 前端首次打开时用它初始化表单，避免前后端默认值漂移。
#[tauri::command]
pub fn default_options() -> ScanOptions {
    ScanOptions::default()
}

#[tauri::command]
pub fn default_mark_config() -> AutoMarkConfig {
    AutoMarkConfig::default()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub platform: String,
    pub arch: String,
    pub logical_cores: usize,
    pub effective_threads: usize,
    pub data_dir: String,
    pub db_path: String,
    pub quarantine_dir: String,
    pub export_dir: String,
    pub protect_system_dirs: bool,
    pub cache_entries: u64,
    pub cache_ttl_days: u32,
}

#[tauri::command]
pub fn system_info(state: State<'_, AppState>) -> AppResult<SystemInfo> {
    let cache_entries = state
        .db
        .read(crate::db::cache_repo::count)
        .unwrap_or(0);

    Ok(SystemInfo {
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        logical_cores: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        effective_threads: state.config.effective_threads(),
        data_dir: state.config.data_dir.to_string_lossy().to_string(),
        db_path: state.config.db_path.to_string_lossy().to_string(),
        quarantine_dir: state.config.quarantine_dir.to_string_lossy().to_string(),
        export_dir: state.config.export_dir.to_string_lossy().to_string(),
        protect_system_dirs: state.config.protect_system_dirs,
        cache_entries,
        cache_ttl_days: state.config.cache_max_age_days,
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub entries: u64,
    pub bytes: u64,
}

#[tauri::command]
pub fn cache_stats(state: State<'_, AppState>) -> AppResult<CacheStats> {
    Ok(CacheStats {
        entries: state.db.read(crate::db::cache_repo::count)?,
        bytes: state.db.file_size_bytes(),
    })
}

/// 清空增量缓存，下次扫描将全量重算（用户怀疑缓存失真时使用）。
#[tauri::command]
pub fn clear_cache(state: State<'_, AppState>) -> AppResult<u64> {
    let removed = state.db.write(|tx| crate::db::cache_repo::purge_all(tx))?;
    tracing::info!(removed, "增量缓存已清空");
    Ok(removed as u64)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
