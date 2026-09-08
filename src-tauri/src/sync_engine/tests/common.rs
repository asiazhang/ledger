//! 多端同步域测试薄皮（仅限本测试目录使用）：双端建库、交易语义输入构造器与
//! 业务字段行读取。通用夹具（建库两行序、账户种子）消费统一测试工厂
//! `crate::test_support`（ADR-0084）；本文件只留本域特有的输入构造与判据读取。

use rusqlite::Connection;

use crate::transaction::TransactionInput;
use crate::transaction::amount::TransactionKind;

/// 支出输入构造器（闭环测试的「A 端写」侧语义输入）。
pub(crate) fn make_expense(account_id: &str, amount_cents: i64, note: &str) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Expense,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: Some(note.into()),
        date: "2026-01-10".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    }
}

/// 一笔交易的业务字段快照（判据读取）：账本状态一致 = 业务字段相等。
///
/// 审计列（created_at / updated_at / device_id / version）是各端本地事实，
/// 不参与状态等值判定（ADR-0091：op 只携带 DeviceId / 逻辑时钟 / schema 版本，
/// 不携带墙钟；LWW 裁决依据是 op 全序，#856 承接）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TxnRow {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    pub account_id: String,
    pub to_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
    pub is_deleted: i64,
}

/// 按 id 读取交易业务字段（不存在返回 None）。
pub(crate) fn read_transaction(conn: &Connection, id: &str) -> Option<TxnRow> {
    conn.query_row(
        "SELECT kind, amount_cents, currency_code, amount_native_cents, account_id, \
         to_account_id, category_id, merchant_id, refund_of_transaction_id, note, date, is_deleted \
         FROM transactions WHERE id = ?1",
        [id],
        |r| {
            Ok(TxnRow {
                kind: r.get(0)?,
                amount_cents: r.get(1)?,
                currency_code: r.get(2)?,
                amount_native_cents: r.get(3)?,
                account_id: r.get(4)?,
                to_account_id: r.get(5)?,
                category_id: r.get(6)?,
                merchant_id: r.get(7)?,
                refund_of_transaction_id: r.get(8)?,
                note: r.get(9)?,
                date: r.get(10)?,
                is_deleted: r.get(11)?,
            })
        },
    )
    .ok()
}
