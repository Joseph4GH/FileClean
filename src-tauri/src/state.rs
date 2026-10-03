//! 应用运行时状态：扫描会话注册表 + 取消令牌 + 数据库句柄。

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::config::AppConfig;
use crate::db::Db;
use crate::domain::models::{
    AutoMarkConfig, ScanOptions, ScanProgress, ScanResult, ScanStatus,
};

/// 一次扫描任务的完整运行时状态。
pub struct ScanSession {
    pub status: ScanStatus,
    pub progress: ScanProgress,
    pub result: Option<ScanResult>,
    pub options: ScanOptions,
    pub mark_config: AutoMarkConfig,
    pub cancel: Arc<AtomicBool>,
    pub error: Option<String>,
    pub started_at: i64,
}

impl ScanSession {
    pub fn new(
        scan_id: &str,
        options: ScanOptions,
        mark_config: AutoMarkConfig,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        ScanSession {
            status: ScanStatus::Running,
            progress: ScanProgress::new(scan_id, crate::domain::models::ScanPhase::Idle),
            result: None,
            options,
            mark_config,
            cancel,
            error: None,
            started_at: chrono::Utc::now().timestamp(),
        }
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatusPayload {
    pub scan_id: String,
    pub status: ScanStatus,
    pub progress: ScanProgress,
    pub error: Option<String>,
    pub has_result: bool,
}

pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: Arc<Db>,
    pub sessions: Mutex<HashMap<String, ScanSession>>,
}

impl AppState {
    pub fn new(config: AppConfig, db: Db) -> Self {
        AppState {
            config: Arc::new(config),
            db: Arc::new(db),
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

/// 回收超过 6 小时且已结束的会话，避免长时间运行时内存里堆满历史结果。
pub fn sweep_finished(state: &AppState) {
    let cutoff = chrono::Utc::now().timestamp() - 6 * 3600;
    let mut sessions = state.sessions.lock();
    sessions.retain(|_, s| s.status == ScanStatus::Running || s.started_at >= cutoff);
}
