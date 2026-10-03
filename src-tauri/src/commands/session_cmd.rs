//! 会话命令：扫描结果可存为项目、可导出为文件、可恢复继续处理。

use std::path::PathBuf;

use tauri::State;

use crate::commands::finished_result;
use crate::db::session_repo;
use crate::domain::models::{AutoMarkConfig, ScanResult, SessionMeta};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionPayload {
    version: u32,
    result: ScanResult,
    mark_config: AutoMarkConfig,
}

const SESSION_VERSION: u32 = 1;

#[tauri::command]
pub fn save_session(
    state: State<'_, AppState>,
    scan_id: String,
    name: Option<String>,
) -> AppResult<SessionMeta> {
    let (result, mark_config) = crate::commands::session_ref(&state, &scan_id, |s| {
        Ok((s.result.clone(), s.mark_config.clone()))
    })?;

    let result = result.ok_or_else(|| AppError::config("扫描尚未完成，无法保存会话"))?;
    let name = name.unwrap_or_else(|| {
        format!(
            "扫描 {} · {} 个分组",
            chrono::Local::now().format("%Y-%m-%d %H:%M"),
            result.stats.groups
        )
    });

    let payload = serde_json::to_string(&SessionPayload {
        version: SESSION_VERSION,
        result,
        mark_config,
    })?;

    let id = uuid::Uuid::new_v4().to_string();
    let mut meta = state
        .db
        .write(|tx| session_repo::save(tx, &id, &name, &payload))?;
    meta.roots = state
        .sessions
        .lock()
        .get(&scan_id)
        .map(|s| s.options.roots.clone())
        .unwrap_or_default();
    Ok(meta)
}

/// 覆盖保存到已有会话（继续上次操作时使用）。
#[tauri::command]
pub fn update_session(
    state: State<'_, AppState>,
    session_id: String,
    scan_id: String,
    name: Option<String>,
) -> AppResult<SessionMeta> {
    let (result, mark_config) = crate::commands::session_ref(&state, &scan_id, |s| {
        Ok((s.result.clone(), s.mark_config.clone()))
    })?;
    let result = result.ok_or_else(|| AppError::config("扫描尚未完成"))?;

    let payload = serde_json::to_string(&SessionPayload {
        version: SESSION_VERSION,
        result,
        mark_config,
    })?;

    let final_name = name.unwrap_or_else(|| "未命名会话".to_string());
    state
        .db
        .write(|tx| session_repo::save(tx, &session_id, &final_name, &payload))
}

#[tauri::command]
pub fn list_sessions(state: State<'_, AppState>) -> AppResult<Vec<SessionMeta>> {
    state.db.read(session_repo::list)
}

#[tauri::command]
pub fn delete_session(state: State<'_, AppState>, session_id: String) -> AppResult<()> {
    state.db.write(|tx| session_repo::delete(tx, &session_id))?;
    Ok(())
}

/// 载入会话：把保存的结果重新注册成一个可继续操作的扫描会话。
#[tauri::command]
pub fn load_session(state: State<'_, AppState>, session_id: String) -> AppResult<ScanResult> {
    let raw = state
        .db
        .read(|conn| session_repo::load(conn, &session_id))?
        .ok_or_else(|| AppError::NotFound(format!("会话 {session_id}")))?;

    let payload: SessionPayload = serde_json::from_str(&raw)?;
    if payload.version > SESSION_VERSION {
        return Err(AppError::Unsupported(format!(
            "会话版本 {} 高于当前支持的 {}，请升级应用",
            payload.version, SESSION_VERSION
        )));
    }

    register_session(&state, payload.result.clone(), payload.mark_config);
    Ok(payload.result)
}

/// 导出为独立的 `.fileclean.json`，便于跨机器携带或归档。
#[tauri::command]
pub fn export_session_file(
    state: State<'_, AppState>,
    scan_id: String,
    out_path: Option<String>,
) -> AppResult<String> {
    let result = finished_result(&state, &scan_id)?;
    let mark_config = crate::commands::session_ref(&state, &scan_id, |s| Ok(s.mark_config.clone()))?;

    let path = match out_path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => state.config.export_dir.join(format!(
            "fileclean-session-{}.json",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        )),
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let payload = serde_json::to_string_pretty(&SessionPayload {
        version: SESSION_VERSION,
        result,
        mark_config,
    })?;
    std::fs::write(&path, payload)?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn import_session_file(state: State<'_, AppState>, path: String) -> AppResult<ScanResult> {
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| AppError::NotFound(path.clone()))?;
    let payload: SessionPayload = serde_json::from_str(&raw)
        .map_err(|e| AppError::Unsupported(format!("会话文件解析失败：{e}")))?;

    if payload.version > SESSION_VERSION {
        return Err(AppError::Unsupported(format!(
            "会话文件版本 {} 高于当前支持的 {}",
            payload.version, SESSION_VERSION
        )));
    }

    register_session(&state, payload.result.clone(), payload.mark_config);
    Ok(payload.result)
}

/// 判断某个会话是否还「活着」（可用于 UI 提示是否可继续清理）。
#[tauri::command]
pub fn session_alive(state: State<'_, AppState>, scan_id: String) -> AppResult<bool> {
    let sessions = state.sessions.lock();
    Ok(sessions.contains_key(&scan_id))
}

/// 把一份结果注册成新的运行时会话，返回新的 scanId。
#[tauri::command]
pub fn adopt_result(
    state: State<'_, AppState>,
    result: ScanResult,
    mark_config: Option<AutoMarkConfig>,
) -> AppResult<String> {
    Ok(register_session(
        &state,
        result,
        mark_config.unwrap_or_default(),
    ))
}

fn register_session(
    state: &AppState,
    result: ScanResult,
    mark_config: AutoMarkConfig,
) -> String {
    use crate::domain::models::{ScanPhase, ScanStatus};
    use crate::state::ScanSession;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    // 载入的结果不允许再执行清理（源文件可能早已变化），
    // 因此标记为已完成但重置所有「待清理」为未决策，强制用户重新确认。
    let mut result = result;
    for group in &mut result.groups {
        for file in &mut group.files {
            if file.decision == crate::domain::models::MarkDecision::Remove {
                file.decision = crate::domain::models::MarkDecision::Undecided;
                file.reasons
                    .push("会话载入：需重新确认后再执行清理".into());
            }
        }
        group.recompute();
    }
    crate::commands::result_cmd::recompute_stats(&mut result);

    let scan_id = uuid::Uuid::new_v4().to_string();
    let options = result.options.clone();

    let mut session = ScanSession::new(
        &scan_id,
        options,
        mark_config,
        Arc::new(AtomicBool::new(false)),
    );
    session.status = ScanStatus::Done;
    session.result = Some(result);
    session.progress.phase = ScanPhase::Done;
    session.progress.phase_label = ScanPhase::Done.label().to_string();
    session.progress.percent = 1.0;
    session.progress.message = "已从会话载入（清理前需重新确认）".into();

    state.sessions.lock().insert(scan_id.clone(), session);
    scan_id
}
