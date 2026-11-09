//! 出资项分解（写路径，issue #1860 / ADR-0138）：expense/income 出资分解子行的
//! 契约校验与落库。
//!
//! 职责：[`validate_funding_items`]（契约互斥 + kind 准入 + 逐条约束：正整数分、
//! 标签 ≤50 字、账户准入与 `account_id` 同口径、Σ == 交易金额）与子行落库三件
//! （[`insert_rows`] / [`replace_rows`] / [`item_account_ids`]）。与 ADR-0096 的
//! buy/sell 出资账户（`crate::write::funding`）是两个不同源概念：本模块无账户
//! 类型闭集（credit / debt 可出资），账户存在/未软删/币种一致经同一视图接缝
//! 判定。ADR 指针：ADR-0138 决策 1/2/4。陷阱：子行随主行存亡——软删不改子行，
//! 读侧按主行 `is_deleted` 过滤；修改为全量替换（单 ⇄ 多就地互转）。

use rusqlite::Connection;
use rusqlite::params;

use ledger_infra::error::{AppError, Result};

use crate::amount::TransactionKind;
use crate::seams::funding::lookup_funding_account;

/// 扣款标签上限（字 = Unicode 字符数，ADR-0138 决策 4）。
pub const LABEL_MAX_CHARS: usize = 50;

/// 归一化后的出资项（写路径自有类型，与模型层 `TransactionFundingInput` 解耦，
/// 先例 [`super::writer::Input`]）。数组顺序即落库顺序位（读回顺序是用户可见
/// 语义——对账单顺序）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingItem {
    pub account_id: String,
    pub amount_cents: i64,
    pub label: Option<String>,
}

impl From<&crate::model::TransactionFundingInput> for FundingItem {
    fn from(i: &crate::model::TransactionFundingInput) -> Self {
        Self {
            account_id: i.account_id.clone(),
            amount_cents: i.amount_cents,
            label: i.label.clone(),
        }
    }
}

impl From<&FundingItem> for crate::model::TransactionFundingInput {
    fn from(i: &FundingItem) -> Self {
        Self {
            account_id: i.account_id.clone(),
            amount_cents: i.amount_cents,
            label: i.label.clone(),
        }
    }
}

/// 校验出资分解契约（创建与修改共用；不落库、只判定）。
///
/// - 契约互斥（ADR-0138 决策 2）：非空分解 ⇒ `account_id` 必须缺省（码化报错）；
/// - kind 准入（决策 3）：仅 expense / income 可带；refund 显式覆盖同契约放行
///   （决策 5），其余 kind 携带即拒绝；
/// - 逐条约束（决策 4）：金额正整数分；标签可选 ≤50 字；账户准入与 `account_id`
///   同口径（存在、未软删、币种与交易一致、不折算），无账户类型闭集；允许同一
///   账户多条；Σ 全部条目 == 交易金额；
/// - 「保持历史引用」（修改路径，与 `existing_merchant_id` 同款语义）：提交分解
///   与该行当前分解逐项相同且币种未变时跳过逐条账户准入——历史分解行的出资
///   账户此后软删不阻止编辑其他字段。
///
/// `currency` 必须传**最终交易币种**（refund 继承原支出币种，调用方填值被忽略）。
/// `amount_cents` 为交易金额（主行正性已由归一化先行校验）。
pub struct FundingContract<'a> {
    pub kind: TransactionKind,
    pub account_id: Option<&'a str>,
    pub amount_cents: i64,
    pub currency: &'a str,
    pub funding: &'a [FundingItem],
    pub existing_funding: &'a [FundingItem],
    pub currency_unchanged: bool,
}
pub fn validate_funding_items(conn: &Connection, contract: FundingContract<'_>) -> Result<()> {
    let FundingContract {
        kind,
        account_id,
        amount_cents,
        currency,
        funding,
        existing_funding,
        currency_unchanged,
    } = contract;
    if funding.is_empty() {
        return Ok(());
    }
    if !matches!(
        kind,
        TransactionKind::Income | TransactionKind::Expense | TransactionKind::Refund
    ) {
        return Err(AppError::codedp(
            "transaction.funding-item-unsupported",
            format!("交易类型 {kind} 不能携带出资分解"),
            &[&kind.to_string()],
        ));
    }
    if account_id.is_some() {
        return Err(AppError::coded(
            "transaction.funding-account-conflict",
            "携带出资分解时不可指定 account_id",
        ));
    }
    let unchanged = currency_unchanged && *funding == *existing_funding;
    let mut total: i64 = 0;
    for item in funding {
        if item.amount_cents <= 0 {
            return Err(AppError::coded(
                "transaction.funding-amount-positive",
                "出资项金额必须大于 0",
            ));
        }
        if item
            .label
            .as_deref()
            .map(str::chars)
            .map(Iterator::count)
            .unwrap_or(0)
            > LABEL_MAX_CHARS
        {
            return Err(AppError::coded(
                "transaction.funding-label-too-long",
                "扣款标签长度不能超过 50 字",
            ));
        }
        if !unchanged {
            validate_item_account(conn, &item.account_id, currency)?;
        }
        total += item.amount_cents;
    }
    if total != amount_cents {
        return Err(AppError::codedp(
            "transaction.funding-sum-mismatch",
            format!("出资分解合计（{total}）与交易金额（{amount_cents}）不一致"),
            &[&total.to_string(), &amount_cents.to_string()],
        ));
    }
    Ok(())
}

/// 单条出资项的账户准入（存在、未软删、币种一致）：经出资账户视图接缝读行，
/// 缺行码化 NotFound（与商户/保单同款）；币种不一致码化冲突。不做账户类型
/// 闭集判定（credit / debt 可出资，ADR-0138 决策 4）。
fn validate_item_account(conn: &Connection, account_id: &str, currency: &str) -> Result<()> {
    let view = lookup_funding_account(conn, account_id)?.ok_or_else(|| {
        AppError::codedp_not_found(
            "funding.account-not-found",
            format!("出资账户不存在或已删除: {account_id}"),
            &[account_id],
        )
    })?;
    if view.currency_code != currency {
        return Err(AppError::codedp(
            "funding.currency-mismatch",
            format!(
                "出资账户币种（{}）与交易币种（{}）不一致",
                view.currency_code, currency
            ),
            &[&view.currency_code, currency],
        ));
    }
    Ok(())
}

/// 重放路径的出资子行币种复验（ADR-0138 影响节，与 [`crate::write::writer::validate_currency_consistency`]
/// 同题）：重放对主腿账户复验币种一致，出资子行端同不变量（「现金腿即账户币种」——
/// 余额表达式按子行 `amount_cents` 原样计入）不得因伪造/漂移载荷静默破例。逐条复用
/// [`validate_item_account`] 同一准入收口（存在/未软删/币种一致）；重放端存活已由
/// `validate_accounts_alive` 扩出资子行端先行判定（码保持 `account.not-found` 口径），
/// 故本函数实际只补币种差；失败码化上抛，由同步引擎挂 ParkedOp 挂起（与币种守卫同口径）。
pub fn validate_funding_item_currencies(
    conn: &Connection,
    funding: &[FundingItem],
    currency_code: &str,
) -> Result<()> {
    for item in funding {
        validate_item_account(conn, &item.account_id, currency_code)?;
    }
    Ok(())
}

/// 子行落库（创建路径）：数组顺序即顺序位（0 起）。
pub fn insert_rows(conn: &Connection, transaction_id: &str, funding: &[FundingItem]) -> Result<()> {
    for (sort, item) in funding.iter().enumerate() {
        conn.execute(
            "INSERT INTO transaction_fundings \
             (transaction_id,sort,account_id,amount_cents,label) VALUES (?1,?2,?3,?4,?5)",
            params![
                transaction_id,
                sort as i64,
                item.account_id,
                item.amount_cents,
                item.label,
            ],
        )?;
    }
    Ok(())
}

/// 子行全量替换（修改路径，ADR-0138 决策 6 全量语义）：先清后插，单 ⇄ 多就地
/// 互转同一路径。调用方必须在既有写事务内调用（与主行 UPDATE 同事务）。
pub fn replace_rows(
    conn: &Connection,
    transaction_id: &str,
    funding: &[FundingItem],
) -> Result<()> {
    conn.execute(
        "DELETE FROM transaction_fundings WHERE transaction_id=?1",
        params![transaction_id],
    )?;
    insert_rows(conn, transaction_id, funding)
}

/// 读一行的出资项账户 id 序列（顺序位序）：受影响账户并集推导与重放存活校验
/// 的输入（删除/修改路径在改行前读取）。
pub fn item_account_ids(conn: &Connection, transaction_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT account_id FROM transaction_fundings \
         WHERE transaction_id=?1 ORDER BY sort",
    )?;
    let ids = stmt
        .query_map(params![transaction_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// 读一行落库的出资项（顺序位序）：修改路径「保持历史引用」的比对基准。
pub fn read_rows(conn: &Connection, transaction_id: &str) -> Result<Vec<FundingItem>> {
    let mut stmt = conn.prepare(
        "SELECT account_id, amount_cents, label FROM transaction_fundings \
         WHERE transaction_id=?1 ORDER BY sort",
    )?;
    let items = stmt
        .query_map(params![transaction_id], |r| {
            Ok(FundingItem {
                account_id: r.get(0)?,
                amount_cents: r.get(1)?,
                label: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(items)
}
