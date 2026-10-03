//! 集中式配置：全部来自环境变量，启动时一次性解析并校验（快速失败）。
//!
//! 任何密钥 / 用户隐私数据都不落代码库，真实 `.env` 已在 `.gitignore` 中排除。

use std::path::{Path, PathBuf};

use crate::domain::models::DeleteMode;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// 应用数据根目录（数据库、日志、会话、隔离区）。
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub log_dir: PathBuf,
    pub quarantine_dir: PathBuf,
    pub export_dir: PathBuf,
    /// 显式指定的回收站目录。设置后**不再调用系统回收站**，改为直接移入该目录。
    ///
    /// 存在的理由有两个，都是实际踩出来的：
    ///   1. 系统回收站依赖运行环境（macOS 要 Finder 自动化授权、Linux 要 XDG 目录），
    ///      在 CI / 纯 SSH 会话里落点不确定，甚至完全不可用；
    ///   2. `$HOME` 只能重定向「备用方案」，重定向不了系统原生回收站 ——
    ///      测试因此会把文件丢进开发者真实的废纸篓，而且无法断言可读回性。
    pub trash_dir: Option<PathBuf>,

    pub log_level: String,
    pub log_json: bool,

    /// 默认并发线程数（0 表示交给 rayon 按 CPU 核数自动决定）。
    pub default_threads: usize,
    /// 扫描文件数量硬上限，防止误选根目录导致失控。
    pub max_scan_files: usize,
    /// 增量缓存最大有效天数，超过则强制重算哈希。
    pub cache_max_age_days: u32,
    /// 是否默认启用系统关键目录保护锁。
    pub protect_system_dirs: bool,
    /// 默认删除模式。
    pub default_delete_mode: DeleteMode,
    /// 单文件全量哈希的上限（字节）。超过此值仅计算部分哈希 + 抽样哈希。
    pub max_full_hash_bytes: u64,
}

fn env_str(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn env_bool(key: &str, default: bool) -> AppResult<bool> {
    match env_str(key) {
        None => Ok(default),
        Some(raw) => match raw.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            other => Err(AppError::config(format!(
                "环境变量 {key} 取值非法：{other}（期望 true/false）"
            ))),
        },
    }
}

fn env_usize(key: &str, default: usize) -> AppResult<usize> {
    match env_str(key) {
        None => Ok(default),
        Some(raw) => raw
            .parse::<usize>()
            .map_err(|_| AppError::config(format!("环境变量 {key} 取值非法：{raw}（期望无符号整数）"))),
    }
}

fn env_u32(key: &str, default: u32) -> AppResult<u32> {
    match env_str(key) {
        None => Ok(default),
        Some(raw) => raw
            .parse::<u32>()
            .map_err(|_| AppError::config(format!("环境变量 {key} 取值非法：{raw}（期望无符号整数）"))),
    }
}

fn env_u64(key: &str, default: u64) -> AppResult<u64> {
    match env_str(key) {
        None => Ok(default),
        Some(raw) => raw
            .parse::<u64>()
            .map_err(|_| AppError::config(format!("环境变量 {key} 取值非法：{raw}（期望无符号整数）"))),
    }
}

/// 平台默认数据目录（不引入额外依赖，按平台约定拼装）。
pub fn default_data_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("FILECLEAN_DATA_DIR") {
        if !explicit.trim().is_empty() {
            return PathBuf::from(explicit);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("FileClean")
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| "C:\\Temp".into());
        PathBuf::from(base).join("FileClean")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            if !xdg.trim().is_empty() {
                return PathBuf::from(xdg).join("fileclean");
            }
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(".local").join("share").join("fileclean")
    }
}

impl AppConfig {
    pub fn from_env() -> AppResult<Self> {
        // .env 仅用于本地开发便利；缺失不是错误（IDE 启动时通常没有）。
        let _ = dotenvy::dotenv();

        let data_dir = default_data_dir();

        let default_delete_mode = match env_str("FILECLEAN_DEFAULT_DELETE_MODE")
            .unwrap_or_else(|| "trash".into())
            .to_ascii_lowercase()
            .as_str()
        {
            "trash" => DeleteMode::Trash,
            "hardlink" => DeleteMode::HardlinkReplace,
            "move" => DeleteMode::MoveTo,
            other => {
                return Err(AppError::config(format!(
                    "FILECLEAN_DEFAULT_DELETE_MODE 取值非法：{other}（可选 trash/hardlink/move）"
                )))
            }
        };

        let cfg = AppConfig {
            db_path: data_dir.join("fileclean.sqlite3"),
            log_dir: data_dir.join("logs"),
            quarantine_dir: data_dir.join("quarantine"),
            export_dir: data_dir.join("exports"),
            trash_dir: env_str("FILECLEAN_TRASH_DIR").map(PathBuf::from),
            data_dir,

            log_level: env_str("FILECLEAN_LOG_LEVEL").unwrap_or_else(|| "info".into()),
            log_json: env_bool("FILECLEAN_LOG_JSON", false)?,

            default_threads: env_usize("FILECLEAN_THREADS", 0)?,
            max_scan_files: env_usize("FILECLEAN_MAX_SCAN_FILES", 2_000_000)?,
            cache_max_age_days: env_u32("FILECLEAN_CACHE_MAX_AGE_DAYS", 30)?,
            protect_system_dirs: env_bool("FILECLEAN_PROTECT_SYSTEM_DIRS", true)?,
            default_delete_mode,
            max_full_hash_bytes: env_u64("FILECLEAN_MAX_FULL_HASH_BYTES", 8 * 1024 * 1024 * 1024)?,
        };

        cfg.validate()?;
        Ok(cfg)
    }

    /// 启动时集中校验：目录可创建、关键路径可写。
    fn validate(&self) -> AppResult<()> {
        if self.default_threads > 256 {
            return Err(AppError::config(format!(
                "FILECLEAN_THREADS 过大（{}），上限 256",
                self.default_threads
            )));
        }
        if self.cache_max_age_days == 0 || self.cache_max_age_days > 3650 {
            return Err(AppError::config(
                "FILECLEAN_CACHE_MAX_AGE_DAYS 需落在 1..=3650 区间".to_string(),
            ));
        }
        if self.max_scan_files == 0 {
            return Err(AppError::config("FILECLEAN_MAX_SCAN_FILES 必须大于 0".to_string()));
        }

        for dir in [
            self.data_dir.as_path(),
            self.log_dir.as_path(),
            self.quarantine_dir.as_path(),
            self.export_dir.as_path(),
        ] {
            ensure_writable_dir(dir)?;
        }

        if let Some(parent) = self.db_path.parent() {
            ensure_writable_dir(parent)?;
        }
        Ok(())
    }

    /// 解析后的有效线程数。
    pub fn effective_threads(&self) -> usize {
        if self.default_threads > 0 {
            self.default_threads
        } else {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
        }
    }

    pub fn cache_ttl_secs(&self) -> i64 {
        i64::from(self.cache_max_age_days) * 86_400
    }
}

fn ensure_writable_dir(dir: &Path) -> AppResult<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| {
            AppError::Config(format!("无法创建目录 {}：{e}", dir.display()))
        })?;
    }
    // 真实写探针：只判断 exists() 会漏掉只读挂载与权限位问题。
    let probe = dir.join(".fileclean-write-probe");
    std::fs::write(&probe, b"ok").map_err(|e| {
        AppError::Config(format!("目录不可写 {}：{e}", dir.display()))
    })?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}
