use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    #[error("数据库错误: {0}")]
    Db(String),
    #[error("数据不存在: {0}")]
    #[allow(dead_code)]
    NotFound(String),
    #[error("参数错误: {0}")]
    Invalid(String),
    #[error("导入解析错误: {0}")]
    Parse(String),
    #[error("IO 错误: {0}")]
    Io(String),
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<csv::Error> for AppError {
    fn from(e: csv::Error) -> Self {
        Self::Parse(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
