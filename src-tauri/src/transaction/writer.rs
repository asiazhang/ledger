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
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：本模块对备份域零感知，
//! 落库后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。
//!
//! **边界**：kind 分派（buy/sell 持仓副作用）、幂等/去重留在命令层，事务边界自
//! issue #228 起归行为层创建编排入口（嵌套感知，ADR-0033）；
//! buy/sell 经其投资层产出归一化行后调用 [`insert_row`]/[`update_row`] 落交易行字段。
//! 本模块不反向依赖命令层：入参/归一化行均为模块自有类型，与 [`super::model::TransactionInput`]
//! 等命令层模型解耦（接线时由命令层做字段转换）。

use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::params;

use crate::db::balance::refresh_account_balances;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};

use super::amount::{self, TransactionKind};

/// 通用 kind 的写入入参（income / expense / transfer / refund）。
///
/// 与命令层 [`super::model::TransactionInput`] 解耦：不含 buy/sell 的投资字段
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
    /// 可选保单引用（issue #361 / ADR-0051 决策 3）：保费（expense）与保单现金流入
    /// （income）可挂一张保单。kind 准入（哪些 kind 可携带）收口在行为层
    /// [`super::behavior`]，本模块只做引用有效性校验。
    /// 修改路径该行**当前**的保单 id（创建路径为 None）由行为层并入
    /// [`existing_policy_id`]：提交值与其相同视为「保持历史引用」——已软删保单的
    /// 历史交易仍可修改其他字段，跳过在用校验（与 [`existing_merchant_id`] 同款语义，
    /// 引用已软删的参考数据不阻止编辑既有行，issue #188 / ADR-0028）。
    pub policy_id: Option<String>,
    pub existing_policy_id: Option<String>,
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
    /// 可选保单引用（issue #361 / ADR-0051 决策 3），随 [`Input::policy_id`] 归一化。
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
}

/// 校验并按 kind 归一化交易字段，产出可直接 INSERT/UPDATE 的交易行。
///
/// 只处理通用 kind（income/expense/transfer/refund）；buy/sell/dividend/split 属
/// 投资层路径（产出归一化行后调 [`insert_row`]/[`update_row`]），收到即报错防误用。
///
/// 语义（与交易行为层通用 kind 写入路径一致，见 `transaction::behavior`）：
/// - 金额必须 > 0；
/// - transfer 必须指定 `to_account_id`；
/// - 商户（merchant_id）：income/expense 携带的商户必须存在且未软删除（软删商户
///   不可再被新交易选择，历史引用照常保留）；refund 忽略调用方填的 merchant_id，
///   自动继承原支出商户（与账户/币种/分类同款继承语义，ADR-0028）；
/// - 保单（policy_id，issue #361）：携带的保单必须存在且未软删除（软删保单不可
///   再被新选择，历史引用照常保留——保单是档案非字典，ADR-0051 决策 5）；
///   修改路径提交值与该行当前保单相同视为保持历史引用（`existing_policy_id`）。
///   kind 准入在行为层收口，本模块不重复判 kind；refund 不继承保单（现金流入
///   记 income 挂单，非 refund，ADR-0051 决策 4），调用方传什么校验什么。
/// - refund 必须关联**未删除**的原支出交易，且账户/币种/分类继承原支出
///   （忽略调用方填的 account_id / currency_code / category_id）；
/// - 本位币折算走 Amount 接缝 [`amount::convert_to_native`]（基准为全局默认币种）。
///
/// 与旧命令层实现的两处**刻意差异**（issue #59 定时引擎 / issue #60 创建修改与
/// 买入卖出行已接线，语义由本模块测试锁定）：
/// - 退款来源不存在/已软删除返回码化 [`AppError::Coded`]（class NotFound，旧实现为 rusqlite 裸错，
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
        return Err(AppError::coded(
            "transfer.to-account-required",
            "转账必须指定目标账户",
        ));
    }
    if input.amount_cents <= 0 {
        return Err(AppError::coded(
            "transaction.amount-positive",
            "金额必须大于 0",
        ));
    }
    let (category_id, account_id, currency_code, merchant_id, refund_of_id) =
        if input.kind == TransactionKind::Refund {
            let ref_id = input.refund_of_transaction_id.clone().ok_or_else(|| {
                AppError::coded("refund.source-required", "退款必须关联原支出交易")
            })?;
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
                .ok_or_else(|| {
                    AppError::codedp_not_found(
                        "refund.source-not-found",
                        format!("退款关联的原支出交易不存在: {ref_id}"),
                        &[ref_id.as_str()],
                    )
                })?;
            if okind != TransactionKind::Expense {
                return Err(AppError::coded(
                    "refund.source-not-expense",
                    "退款只能关联支出交易",
                ));
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
                    validate_merchant_active(conn, Some(merchant_id))?;
                }
            }
            // 保单引用校验（issue #361）：kind 准入在行为层，此处只校验引用有效性。
            if let Some(policy_id) = &input.policy_id {
                let unchanged = input.existing_policy_id.as_deref() == Some(policy_id.as_str());
                if !unchanged {
                    validate_policy_active(conn, Some(policy_id))?;
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
    let policy_id = input.policy_id.clone();
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
        policy_id,
        refund_of_transaction_id: refund_of_id,
        note: input.note.clone(),
        date: input.date.clone(),
    })
}

/// 校验商户存在且未软删除（ADR-0028）：软删商户不可再被**新选择**——新建交易
/// （writer）与新建定时计划（`scheduled_transactions::engine::create_plan`）共用
/// 同一校验与文案（e2e 断言同一错误）。历史引用（编辑保持原商户、计划期次复制
/// 计划商户）不受此限制。
pub fn validate_merchant_active(conn: &Connection, merchant_id: Option<&str>) -> Result<()> {
    if let Some(id) = merchant_id {
        let active: bool = conn
            .query_row(
                "SELECT 1 FROM merchants WHERE id=?1 AND is_deleted=0",
                params![id],
                |_| Ok(true),
            )
            .optional()?
            .is_some();
        if !active {
            return Err(AppError::codedp(
                "merchant.not-active",
                format!("商户不存在或已删除: {id}"),
                &[id],
            ));
        }
    }
    Ok(())
}

/// 校验保单存在且未软删除（issue #361 / ADR-0051 决策 5）：软删保单不可再被
/// **新选择**——新建交易引用；历史引用（编辑保持原保单）不受此限制，
/// 与 [`validate_merchant_active`] 同款语义与文案风格。
pub fn validate_policy_active(conn: &Connection, policy_id: Option<&str>) -> Result<()> {
    if let Some(id) = policy_id {
        let active: bool = conn
            .query_row(
                "SELECT 1 FROM policies WHERE id=?1 AND is_deleted=0",
                params![id],
                |_| Ok(true),
            )
            .optional()?
            .is_some();
        if !active {
            return Err(AppError::codedp(
                "policy.not-active",
                format!("保单不存在或已删除: {id}"),
                &[id],
            ));
        }
    }
    Ok(())
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
         category_id,merchant_id,policy_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,0)",
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
            row.policy_id,
            row.refund_of_transaction_id,
            row.note,
            row.date,
            now,
            now,
            1,
            device_id(),
        ],
    )?;
    // 余额缓存写路径（issue #491 / ADR-0067）：新行落库后在同一事务内对受影响
    // 账户按口径表达式整体重算。本接缝是全部交易创建（手动/批量导入/余额调整/
    // buy/sell/定时引擎例外）的单一收口，挂此处即覆盖全部创建入口。
    let mut affected = vec![row.account_id.as_str()];
    if let Some(to_account_id) = &row.to_account_id {
        affected.push(to_account_id.as_str());
    }
    refresh_account_balances(conn, &affected)?;
    Ok(id)
}

/// 按 `id` 更新交易行字段：保留 `id`、`created_at` 与幂等身份
/// （`idempotency_key` / `dedup_hash`），`version` 递增，`updated_at` / `device_id` 刷新。
///
/// buy/sell 同样经本函数落交易行字段（其持仓/卖出关联副作用由调用方另行处理）。
pub fn update_row(conn: &Connection, id: &str, row: &NormalizedRow) -> Result<()> {
    // 旧账户引用先行读取（同事务）：修改可能移动账户，两侧都要整体重算。
    let (old_account_id, old_to_account_id): (String, Option<String>) = conn.query_row(
        "SELECT account_id, to_account_id FROM transactions WHERE id=?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    conn.execute(
        "UPDATE transactions \
         SET kind=?2, amount_cents=?3, currency_code=?4, amount_native_cents=?5, account_id=?6, \
         to_account_id=?7, category_id=?8, merchant_id=?9, policy_id=?10, refund_of_transaction_id=?11, note=?12, date=?13, \
         updated_at=?14, version=version+1, device_id=?15 \
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
            row.policy_id,
            row.refund_of_transaction_id,
            row.note,
            row.date,
            now_iso(),
            device_id(),
        ],
    )?;
    // 余额缓存写路径：旧/新账户引用并集整体重算（修改可能移动账户，ADR-0067）。
    let mut affected = vec![old_account_id.as_str(), row.account_id.as_str()];
    if let Some(old_to) = &old_to_account_id {
        affected.push(old_to.as_str());
    }
    if let Some(to_account_id) = &row.to_account_id {
        affected.push(to_account_id.as_str());
    }
    refresh_account_balances(conn, &affected)?;
    Ok(())
}

#[cfg(test)]
mod tests;
