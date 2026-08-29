//! Writer 接缝（issue #55 / spec #52）：交易写入的单一权威。
//!
//! 把「交易输入 → 落库行」收口为三个步骤：
//! - [`normalize`]：通用 kind（income/expense/transfer/refund）字段归一化——
//!   金额>0 校验、退款继承原支出账户/币种/分类、transfer 必须有 `to_account_id`、
//!   本位币折算走 Amount 接缝的 [`amount::convert_to_native`]（基准为全局默认币种）。
//! - [`insert_row`]：归一化行 → 全列 INSERT，模块内部生成 `id` 与审计字段
//!   （created_at / updated_at / version / device_id / is_deleted），返回 `id`。
//! - [`update_row`]：归一化行 → 按 `id` UPDATE，保留 `created_at` 与幂等身份
//!   （`idempotency_key` / `dedup_hash`），`version` 递增。
//!
//! **边界**：kind 分派（buy/sell 持仓副作用）、幂等/去重、事务边界留在命令层编排；
//! buy/sell 经其投资层产出归一化行后调用 [`insert_row`]/[`update_row`] 落交易行字段。
//! 本模块不反向依赖命令层：入参/归一化行均为模块自有类型，与 `models::TransactionInput`
//! 等命令层模型解耦（接线时由命令层做字段转换）。

use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::params;

use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};

use super::amount::{self, TransactionKind};

/// 通用 kind 的写入入参（income / expense / transfer / refund）。
///
/// 与命令层 `models::TransactionInput` 解耦：不含 buy/sell 的投资字段
/// （instrument_id/quantity/price_cents/fee_cents）与幂等键——幂等身份由
/// 命令层在落库后另行回写，不属本模块职责。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_id: String,
    pub to_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    /// 修改路径该行**当前**的商户 id（创建路径为 None）。提交的 [`merchant_id`] 与其
    /// 相同视为「保持历史引用」：软删商户的历史交易仍可修改其他字段，跳过在用校验；
    /// 改选其他商户则按新选择校验在用。与账户/分类的更新语义一致（引用已软删的
    /// 参考数据不阻止编辑既有行，issue #188 / ADR-0028）。
    pub existing_merchant_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
}

/// 归一化后的交易行字段（供 [`insert_row`] / [`update_row`] 落库）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedRow {
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
}

/// 校验并按 kind 归一化交易字段，产出可直接 INSERT/UPDATE 的交易行。
///
/// 只处理通用 kind（income/expense/transfer/refund）；buy/sell/dividend/split 属
/// 投资层路径（产出归一化行后调 [`insert_row`]/[`update_row`]），收到即报错防误用。
///
/// 语义（与命令层通用 kind 写入路径一致，见 `commands::transactions::write`）：
/// - 金额必须 > 0；
/// - transfer 必须指定 `to_account_id`；
/// - 商户（merchant_id）：income/expense 携带的商户必须存在且未软删除（软删商户
///   不可再被新交易选择，历史引用照常保留）；refund 忽略调用方填的 merchant_id，
///   自动继承原支出商户（与账户/币种/分类同款继承语义，ADR-0028）；
/// - refund 必须关联**未删除**的原支出交易，且账户/币种/分类继承原支出
///   （忽略调用方填的 account_id / currency_code / category_id）；
/// - 本位币折算走 Amount 接缝 [`amount::convert_to_native`]（基准为全局默认币种）。
///
/// 与旧命令层实现的两处**刻意差异**（issue #59 定时引擎 / issue #60 创建修改与
/// 买入卖出行已接线，语义由本模块测试锁定）：
/// - 退款来源不存在/已软删除返回 [`AppError::NotFound`]（旧实现为 rusqlite 裸错，
///   语义不明）；其余错误文案（转账/金额/退款只能关联支出）逐字沿用旧文本，
///   保证接线后 HTTP/BDD 断言不回退；
/// - 折算目标为全局默认币种（spec #52 口径权威），而非旧实现的账户币种——
///   MVP 全 CNY 时二者 1:1 无差异，汇率生效后以本模块为准（旧实现已随接线删除）。
pub fn normalize(conn: &Connection, input: &Input) -> Result<NormalizedRow> {
    match input.kind {
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => {}
        TransactionKind::Buy
        | TransactionKind::Sell
        | TransactionKind::Dividend
        | TransactionKind::Split => {
            return Err(AppError::Invalid(format!(
                "writer::normalize 仅处理通用交易类型（income/expense/transfer/refund），收到: {}",
                input.kind
            )));
        }
    }
    if input.kind == TransactionKind::Transfer && input.to_account_id.is_none() {
        return Err(AppError::Invalid("转账必须指定目标账户".into()));
    }
    if input.amount_cents <= 0 {
        return Err(AppError::Invalid("金额必须大于 0".into()));
    }
    let (category_id, account_id, currency_code, merchant_id, refund_of_id) = if input.kind
        == TransactionKind::Refund
    {
        let ref_id = input
            .refund_of_transaction_id
            .clone()
            .ok_or_else(|| AppError::Invalid("退款必须关联原支出交易".into()))?;
        // 只认未删除的原支出交易；不存在或已软删除均视为无效来源（NotFound），
        // 其余数据库错误（锁/损坏等）原样上抛。
        let (cat, acc, cur, mer, okind): (
            Option<String>,
            String,
            String,
            Option<String>,
            TransactionKind,
        ) = conn
            .query_row(
                "SELECT category_id, account_id, currency_code, merchant_id, kind \
                 FROM transactions WHERE id=?1 AND is_deleted=0",
                params![ref_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("退款关联的原支出交易不存在: {ref_id}")))?;
        if okind != TransactionKind::Expense {
            return Err(AppError::Invalid("退款只能关联支出交易".into()));
        }
        // 商户与账户/币种/分类同款继承语义：忽略调用方填的 merchant_id，取原支出商户。
        (cat, acc, cur, mer, Some(ref_id))
    } else {
        // 商户字典校验：income/expense 携带的商户必须存在且未软删除——软删商户
        // 不可再被**新选择**（新建交易引用；历史引用照常保留）。修改路径提交值与
        // 该行当前商户相同视为保持历史引用（`existing_merchant_id`），跳过校验。
        if let Some(merchant_id) = &input.merchant_id {
            let unchanged = input.existing_merchant_id.as_deref() == Some(merchant_id.as_str());
            if !unchanged {
                let active: bool = conn
                    .query_row(
                        "SELECT 1 FROM merchants WHERE id=?1 AND is_deleted=0",
                        params![merchant_id],
                        |_| Ok(true),
                    )
                    .optional()?
                    .is_some();
                if !active {
                    return Err(AppError::Invalid(format!(
                        "商户不存在或已删除: {merchant_id}"
                    )));
                }
            }
        }
        (
            input.category_id.clone(),
            input.account_id.clone(),
            input.currency_code.clone(),
            input.merchant_id.clone(),
            None,
        )
    };
    let native = amount::convert_to_native(conn, input.amount_cents, &currency_code)?;
    let to_account_id = if input.kind == TransactionKind::Transfer {
        input.to_account_id.clone()
    } else {
        None
    };
    Ok(NormalizedRow {
        kind: input.kind,
        amount_cents: input.amount_cents,
        currency_code,
        amount_native_cents: native,
        account_id,
        to_account_id,
        category_id,
        merchant_id,
        refund_of_transaction_id: refund_of_id,
        note: input.note.clone(),
        date: input.date.clone(),
    })
}

/// 将归一化行落库为完整交易行：生成 `id`（UUID v7）+ 审计字段
/// （created_at / updated_at / version=1 / device_id / is_deleted=0），返回新交易 `id`。
///
/// 幂等身份（`idempotency_key` / `dedup_hash`）不在此写入——由命令层（批量导入）
/// 在落库后另行回写，保持本模块单一职责。
pub fn insert_row(conn: &Connection, row: &NormalizedRow) -> Result<String> {
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,merchant_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,0)",
        params![
            id,
            row.kind.as_str(),
            row.amount_cents,
            row.currency_code,
            row.amount_native_cents,
            row.account_id,
            row.to_account_id,
            row.category_id,
            row.merchant_id,
            row.refund_of_transaction_id,
            row.note,
            row.date,
            now,
            now,
            1,
            device_id(),
        ],
    )?;
    // 脏标记挂钩（issue #126）：落库成功即置脏，到期则写时顺带触发备份。
    crate::auto_backup::on_write(conn);
    Ok(id)
}

/// 按 `id` 更新交易行字段：保留 `id`、`created_at` 与幂等身份
/// （`idempotency_key` / `dedup_hash`），`version` 递增，`updated_at` / `device_id` 刷新。
///
/// buy/sell 同样经本函数落交易行字段（其持仓/卖出关联副作用由调用方另行处理）。
pub fn update_row(conn: &Connection, id: &str, row: &NormalizedRow) -> Result<()> {
    conn.execute(
        "UPDATE transactions \
         SET kind=?2, amount_cents=?3, currency_code=?4, amount_native_cents=?5, account_id=?6, \
         to_account_id=?7, category_id=?8, merchant_id=?9, refund_of_transaction_id=?10, note=?11, date=?12, \
         updated_at=?13, version=version+1, device_id=?14 \
         WHERE id=?1",
        params![
            id,
            row.kind.as_str(),
            row.amount_cents,
            row.currency_code,
            row.amount_native_cents,
            row.account_id,
            row.to_account_id,
            row.category_id,
            row.merchant_id,
            row.refund_of_transaction_id,
            row.note,
            row.date,
            now_iso(),
            device_id(),
        ],
    )?;
    // 脏标记挂钩（issue #126）：更新成功即置脏，到期则写时顺带触发备份。
    crate::auto_backup::on_write(conn);
    Ok(())
}

#[cfg(test)]
mod tests;
