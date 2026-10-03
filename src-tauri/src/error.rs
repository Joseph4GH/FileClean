//! 类型化错误体系。
//!
//! 应用内部一律使用 [`AppError`]，通过 `?` 传播；跨 IPC 边界时由 `Serialize`
//! 实现折叠为 `{ code, message }`，**绝不**把堆栈或内部细节暴露给前端。

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("配置错误：{0}")]
    Config(String),

    #[error("文件系统错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("数据库错误：{0}")]
    Db(#[from] rusqlite::Error),

    #[error("序列化错误：{0}")]
    Serde(#[from] serde_json::Error),

    #[error("CSV 导出错误：{0}")]
    Csv(#[from] csv::Error),

    #[error("路径不存在或不可访问：{0}")]
    NotFound(String),

    #[error("不支持的格式：{0}")]
    Unsupported(String),

    #[error("违反安全策略：{0}")]
    Forbidden(String),

    #[error("扫描已取消")]
    Cancelled,

    #[error("扫描任务不存在：{0}")]
    UnknownScan(String),

    #[error("内部错误：{0}")]
    Internal(String),
}

impl AppError {
    /// 稳定的机器可读错误码，前端据此决定重试 / 提示策略。
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Config(_) => "CONFIG",
            AppError::Io(_) => "IO",
            AppError::Db(_) => "DB",
            AppError::Serde(_) => "SERDE",
            AppError::Csv(_) => "CSV",
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::Unsupported(_) => "UNSUPPORTED",
            AppError::Forbidden(_) => "FORBIDDEN",
            AppError::Cancelled => "CANCELLED",
            AppError::UnknownScan(_) => "UNKNOWN_SCAN",
            AppError::Internal(_) => "INTERNAL",
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        AppError::Internal(msg.into())
    }

    pub fn config(msg: impl Into<String>) -> Self {
        AppError::Config(msg.into())
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        AppError::Forbidden(msg.into())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(format!("{e:#}"))
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("AppError", 2)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
