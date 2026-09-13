//! 交易类型闭集真源（共享语义区，issue #73 / ADR-0108）：9 种交易类型的单点定义。
//!
//! 职责：`closed_set!` 同体派生 enum / `ALL` / `as_str` / `parse` / `Display`，并实现
//! rusqlite `FromSql`、utoipa `ToSchema` 与 serde 小写字符串。不变量：与
//! `transactions.kind` 的 CHECK 约束（V001）一一对应，字符串字面量每变体只出现一次
//! （漏登/漏臂漂移不可表达）。ADR 指针：ADR-0108。陷阱：未知 kind 在 DB/wire 边界
//! 即报码化错误，不静默降级。

use serde::{Deserialize, Serialize};
use utoipa::openapi::{ObjectBuilder, RefOr, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

use ledger_infra::closed_set::closed_set;

// ---------------------------------------------------------------------------
// TransactionKind 枚举
// ---------------------------------------------------------------------------

closed_set! {
/// 交易类型真源（issue #73）。与 `transactions.kind` 的 CHECK 约束（V001）一一对应：
///
/// | kind | 含义 |
/// |------|------|
/// | [`TransactionKind::Income`] | 收入 |
/// | [`TransactionKind::Expense`] | 支出 |
/// | [`TransactionKind::Transfer`] | 转账（`account_id` 转出、`to_account_id` 转入） |
/// | [`TransactionKind::Refund`] | 退款（关联原支出交易） |
/// | [`TransactionKind::Buy`] | 买入证券（减少现金，扩展表记持仓） |
/// | [`TransactionKind::Sell`] | 卖出证券（增加现金） |
/// | [`TransactionKind::Dividend`] | 现金分红 |
/// | [`TransactionKind::Split`] | 拆股/送股（现金影响恒为 0） |
/// | [`TransactionKind::Convert`] | 基金转换（同一投资账户内两标的互换、无现金腿，六度量系数全 0） |
///
/// 五份表示（enum / `ALL` / `as_str` / `parse` / `Display`）由 `closed_set!`
/// 宏同体派生（ADR-0108）：字符串字面量每变体只出现一次，漏登/漏臂漂移
/// 不可表达。
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

// rusqlite：从 `transactions.kind` 列直接读为枚举（DB 边界：TEXT 列经 [`TransactionKind::parse`]
// 严格映射，未知值即 FromSql 错误——DB CHECK 约束（V001）保证正常数据不可达）。
impl rusqlite::types::FromSql for TransactionKind {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        TransactionKind::parse(value.as_str()?)
            .map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
    }
}

// OpenAPI（utoipa）：闭集枚举以小写字符串枚举值入文档，与 wire 格式一致
// （income/expense/transfer/refund/buy/sell/dividend/split）。内联 schema，
// 消费方（如 [`crate::model::Transaction`]）字段直接嵌入、无需注册组件。
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

// serde：以与 `transactions.kind` 同形的小写字符串序列化（wire 格式即小写字符串，
// 与裸 String 时代的 JSON 形状一致）；反序列化复用 [`TransactionKind::parse`]，
// 未知值报错文案与 parse 同源（serde 包装后附位置信息）。
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
