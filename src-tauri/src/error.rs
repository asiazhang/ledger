use serde::{Serialize, Serializer};
use thiserror::Error;

/// 码化错误的归类：决定 HTTP 状态码与序列化后的 `kind` 值（与既有枚举变体同值域）。
/// 仅码化业务错误使用：Invalid = 参数错误（400）、NotFound = 数据不存在（404）。
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

#[derive(Debug, Error)]
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
    /// 码化错误（issue #342 二期 / ADR-0049）：`code` 是稳定错误码（领域语言命名，
    /// 如 `transfer.to-account-required`），`message` 中文原样保留，`params` 是前端
    /// 插值参数（按消息中占位出现顺序）。序列化在既有 `kind`/`message` 之外**只增**
    /// `code` 与可选 `params`，既有消费方与测试不受影响。
    #[error("{message}")]
    Coded {
        class: ErrClass,
        code: String,
        message: String,
        params: Vec<String>,
    },
}

impl AppError {
    /// 码化参数错误（HTTP 400 / kind=Invalid）：业务校验失败的用户可见错误条件。
    pub fn coded(code: &str, message: impl Into<String>) -> Self {
        AppError::Coded {
            class: ErrClass::Invalid,
            code: code.to_string(),
            message: message.into(),
            params: Vec::new(),
        }
    }

    /// 码化参数错误 + 插值参数：`params` 按消息中动态值的出现顺序排列，
    /// 供前端按码本地化时插值（zh 模板 `{0}→{1}`、en 模板同序）。
    pub fn codedp(code: &str, message: impl Into<String>, params: &[&str]) -> Self {
        AppError::Coded {
            class: ErrClass::Invalid,
            code: code.to_string(),
            message: message.into(),
            params: params.iter().map(|p| (*p).to_string()).collect(),
        }
    }

    /// 码化数据不存在（HTTP 404 / kind=NotFound）。
    pub fn coded_not_found(code: &str, message: impl Into<String>) -> Self {
        AppError::Coded {
            class: ErrClass::NotFound,
            code: code.to_string(),
            message: message.into(),
            params: Vec::new(),
        }
    }

    /// 序列化parts：`(kind, message, code, params)`——code/params 为 None 时字段整体缺席。
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

/// 手写序列化（issue #342 二期 / ADR-0049，**只增不改**契约）：
/// 既有字段 `kind`/`message` 的取值与顺序与 derive 时代完全一致；码化错误与
/// Db/Parse/Io 追加 `code`，带插值参数的错误再追加 `params`。无码错误
/// （未被码化收敛的 Invalid/NotFound）保持原两字段形态，前端降级透传原文。
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let (kind, message, code, params) = self.parts();
        let mut s = serializer.serialize_struct("AppError", 4)?;
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
        let err = AppError::coded_not_found("account.not-found", "账户不存在");
        assert_eq!(
            serde_json::to_value(err).unwrap(),
            json!({
                "kind": "NotFound",
                "message": "账户不存在",
                "code": "account.not-found"
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
}
