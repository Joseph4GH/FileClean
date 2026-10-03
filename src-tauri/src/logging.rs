//! 结构化日志。
//!
//! - 默认输出人类可读格式到 stderr（IDE / 终端调试友好）。
//! - `FILECLEAN_LOG_JSON=1` 时切换为 JSON 行协议，便于被日志采集器消费。
//! - 每条扫描相关日志都携带 `scan_id` 作为关联 ID。
//! - 敏感字段（密码、令牌、用户隐私路径）不进入日志埋点。

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use crate::config::AppConfig;

static INITIALIZED: std::sync::Once = std::sync::Once::new();

pub fn init(config: &AppConfig) {
    INITIALIZED.call_once(|| {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            let level = config.log_level.to_ascii_lowercase();
            EnvFilter::new(format!("fileclean_lib={level},fileclean={level},warn"))
        });

        let registry = tracing_subscriber::registry().with(filter);

        if config.log_json {
            let layer = fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(false)
                .with_target(true)
                .with_writer(std::io::stderr);
            registry.with(layer).init();
        } else {
            let layer = fmt::layer()
                .with_target(true)
                .with_level(true)
                .compact()
                .with_writer(std::io::stderr);
            registry.with(layer).init();
        }

        tracing::debug!(
            data_dir = %config.data_dir.display(),
            threads = config.effective_threads(),
            "配置加载完成"
        );
    });
}

/// 在日志中安全地展示路径：隐藏用户主目录前缀，避免把用户名写进日志。
pub fn redact_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() && path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
}
