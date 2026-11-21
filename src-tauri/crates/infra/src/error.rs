use serde::{Serialize, Serializer};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrClass {
    Invalid,
    NotFound,
}

impl ErrClass {
    fn kind_str(self) -> &'static str {
        match self {
            ErrClass::Invalid => "Invalid",
            ErrClass::NotFound => "NotFound",
        }
    }
}

#[derive(Debug, Clone, Error)]
pub enum AppError {
    #[error("数据库错误: {0}")]
    Db(String),
    #[error("数据不存在: {0}")]
    NotFound(String),
    #[error("参数错误: {0}")]
    Invalid(String),
    #[error("导入解析错误: {0}")]
    Parse(String),
    #[error("IO 错误: {0}")]
    Io(String),
    #[error("{message}")]
    Coded {
        class: ErrClass,
        code: String,
        message: String,
        params: Vec<String>,
    },
}

impl AppError {
    pub fn coded(code: &str, message: impl Into<String>) -> Self {
        AppError::Coded {
            class: ErrClass::Invalid,
            code: code.to_string(),
            message: message.into(),
            params: Vec::new(),
        }
    }

    pub fn codedp(code: &str, message: impl Into<String>, params: &[&str]) -> Self {
        AppError::Coded {
            class: ErrClass::Invalid,
            code: code.to_string(),
            message: message.into(),
            params: params.iter().map(|p| (*p).to_string()).collect(),
        }
    }

    pub fn coded_not_found(code: &str, message: impl Into<String>) -> Self {
        AppError::Coded {
            class: ErrClass::NotFound,
            code: code.to_string(),
            message: message.into(),
            params: Vec::new(),
        }
    }

    pub fn codedp_not_found(code: &str, message: impl Into<String>, params: &[&str]) -> Self {
        AppError::Coded {
            class: ErrClass::NotFound,
            code: code.to_string(),
            message: message.into(),
            params: params.iter().map(|p| (*p).to_string()).collect(),
        }
    }

    pub fn is_code(&self, code: &str) -> bool {
        matches!(self, AppError::Coded { code: c, .. } if c == code)
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            AppError::Coded { code, .. } => Some(code),
            _ => None,
        }
    }

    fn parts(&self) -> (&'static str, &str, Option<&str>, Option<&[String]>) {
        match self {
            AppError::Db(m) => ("Db", m, Some("db.error"), None),
            AppError::NotFound(m) => ("NotFound", m, None, None),
            AppError::Invalid(m) => ("Invalid", m, None, None),
            AppError::Parse(m) => ("Parse", m, Some("parse.error"), None),
            AppError::Io(m) => ("Io", m, Some("io.error"), None),
            AppError::Coded {
                class,
                code,
                message,
                params,
            } => (
                class.kind_str(),
                message,
                Some(code),
                if params.is_empty() {
                    None
                } else {
                    Some(params.as_slice())
                },
            ),
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let (kind, message, code, params) = self.parts();
        let field_count = 2 + usize::from(code.is_some()) + usize::from(params.is_some());
        let mut s = serializer.serialize_struct("AppError", field_count)?;
        s.serialize_field("kind", kind)?;
        s.serialize_field("message", message)?;
        if let Some(code) = code {
            s.serialize_field("code", code)?;
        }
        if let Some(params) = params {
            s.serialize_field("params", params)?;
        }
        s.end()
    }
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

impl From<rusqlite_migration::Error> for AppError {
    fn from(e: rusqlite_migration::Error) -> Self {
        Self::Db(e.to_string())
    }
}

impl From<zip::result::ZipError> for AppError {
    fn from(e: zip::result::ZipError) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::Parse(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(feature = "http")]
impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        use axum::Json;
        use axum::http::StatusCode;

        let status = match &self {
            AppError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Invalid(_) => StatusCode::BAD_REQUEST,
            AppError::Parse(_) => StatusCode::BAD_REQUEST,
            AppError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Coded { class, .. } => match class {
                ErrClass::Invalid => StatusCode::BAD_REQUEST,
                ErrClass::NotFound => StatusCode::NOT_FOUND,
            },
        };
        (status, Json(self)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 既有两字段形态不变_无码错误不携带code() {
        assert_eq!(
            serde_json::to_value(AppError::Invalid("参数不能为空".into())).unwrap(),
            json!({ "kind": "Invalid", "message": "参数不能为空" })
        );
        assert_eq!(
            serde_json::to_value(AppError::NotFound("账户不存在".into())).unwrap(),
            json!({ "kind": "NotFound", "message": "账户不存在" })
        );
    }

    #[test]
    fn 码化错误只增code与可选params_kind与message不变() {
        let err = AppError::coded("transfer.to-account-required", "转账目标账户不能为空");
        assert_eq!(
            serde_json::to_value(err).unwrap(),
            json!({
                "kind": "Invalid",
                "message": "转账目标账户不能为空",
                "code": "transfer.to-account-required"
            })
        );

        let err = AppError::codedp(
            "fx.rate-missing",
            format!("缺少 {}→{} 汇率，无法折算", "USD", "CNY"),
            &["USD", "CNY"],
        );
        assert_eq!(
            serde_json::to_value(err).unwrap(),
            json!({
                "kind": "Invalid",
                "message": "缺少 USD→CNY 汇率，无法折算",
                "code": "fx.rate-missing",
                "params": ["USD", "CNY"]
            })
        );
    }

    #[test]
    fn 码化不存在错误保持_not_found_kind() {
        let err = AppError::codedp_not_found(
            "account.not-found",
            format!("账户不存在: {}", "acc-1"),
            &["acc-1"],
        );
        assert_eq!(
            serde_json::to_value(err).unwrap(),
            json!({
                "kind": "NotFound",
                "message": "账户不存在: acc-1",
                "code": "account.not-found",
                "params": ["acc-1"]
            })
        );
    }

    #[test]
    fn 系统类错误携带稳定通用码() {
        assert_eq!(
            serde_json::to_value(AppError::Db("driver exploded".into())).unwrap(),
            json!({ "kind": "Db", "message": "driver exploded", "code": "db.error" })
        );
        assert_eq!(
            serde_json::to_value(AppError::Io("disk gone".into())).unwrap(),
            json!({ "kind": "Io", "message": "disk gone", "code": "io.error" })
        );
        assert_eq!(
            serde_json::to_value(AppError::Parse("bad json".into())).unwrap(),
            json!({ "kind": "Parse", "message": "bad json", "code": "parse.error" })
        );
    }

    #[test]
    fn is_code_按稳定错误码判读() {
        let err = AppError::coded(
            "encryption.passphrase-incorrect",
            "口令错误或文件损坏，请重试",
        );
        assert!(err.is_code("encryption.passphrase-incorrect"));
        assert!(!err.is_code("encryption.remember-no-cache"));
        assert!(!AppError::Io("disk gone".into()).is_code("encryption.passphrase-incorrect"));
    }
}
