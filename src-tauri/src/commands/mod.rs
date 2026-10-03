//! 命令层（控制器）：只做「解析入参 → 调用服务 → 格式化响应」，
//! 不承载业务逻辑，也不直接操作文件系统。

pub mod action_cmd;
pub mod result_cmd;
pub mod scan_cmd;
pub mod session_cmd;

use crate::domain::models::{ScanResult, ScanStatus};
use crate::error::{AppError, AppResult};
use crate::state::{AppState, ScanStatusPayload};

/// 取出会话的可变引用；不存在时返回统一的业务错误。
pub fn session_mut<T>(
    state: &AppState,
    scan_id: &str,
    f: impl FnOnce(&mut crate::state::ScanSession) -> AppResult<T>,
) -> AppResult<T> {
    let mut sessions = state.sessions.lock();
    let session = sessions
        .get_mut(scan_id)
        .ok_or_else(|| AppError::UnknownScan(scan_id.to_string()))?;
    f(session)
}

pub fn session_ref<T>(
    state: &AppState,
    scan_id: &str,
    f: impl FnOnce(&crate::state::ScanSession) -> AppResult<T>,
) -> AppResult<T> {
    let sessions = state.sessions.lock();
    let session = sessions
        .get(scan_id)
        .ok_or_else(|| AppError::UnknownScan(scan_id.to_string()))?;
    f(session)
}

pub fn status_of(state: &AppState, scan_id: &str) -> AppResult<ScanStatusPayload> {
    session_ref(state, scan_id, |s| {
        Ok(ScanStatusPayload {
            scan_id: scan_id.to_string(),
            status: s.status,
            progress: s.progress.clone(),
            error: s.error.clone(),
            has_result: s.result.is_some(),
        })
    })
}

/// 确保扫描已结束再取结果，避免前端拿到半成品。
pub fn finished_result(state: &AppState, scan_id: &str) -> AppResult<ScanResult> {
    session_ref(state, scan_id, |s| match s.status {
        ScanStatus::Done => s
            .result
            .clone()
            .ok_or_else(|| AppError::internal("扫描已标记完成但结果缺失")),
        ScanStatus::Cancelled => Err(AppError::Cancelled),
        ScanStatus::Failed => Err(AppError::internal(
            s.error.clone().unwrap_or_else(|| "扫描失败".into()),
        )),
        ScanStatus::Running => Err(AppError::config("扫描尚未完成，请稍候")),
    })
}
