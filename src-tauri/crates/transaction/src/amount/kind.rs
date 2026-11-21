use serde::{Deserialize, Serialize};
use utoipa::openapi::{ObjectBuilder, RefOr, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

use ledger_infra::closed_set::closed_set;

closed_set! {
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionKind {
    Income => "income",
    Expense => "expense",
    Transfer => "transfer",
    Refund => "refund",
    Buy => "buy",
    Sell => "sell",
    Dividend => "dividend",
    Split => "split",
    Convert => "convert",
}
err_label = "交易类型",
err_code = "transaction.kind-unknown",
}

impl rusqlite::types::FromSql for TransactionKind {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        TransactionKind::parse(value.as_str()?)
            .map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
    }
}

impl PartialSchema for TransactionKind {
    fn schema() -> RefOr<Schema> {
        RefOr::T(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .enum_values(Some(TransactionKind::ALL.map(|k| k.as_str().to_string())))
                .description(Some(
                    "交易类型（闭集，小写字符串，与 transactions.kind 的 CHECK 约束一致）",
                ))
                .build(),
        ))
    }
}

impl ToSchema for TransactionKind {}

impl Serialize for TransactionKind {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TransactionKind {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        TransactionKind::parse(&s).map_err(serde::de::Error::custom)
    }
}
