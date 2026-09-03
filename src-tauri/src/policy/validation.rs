//! 保单创建/编辑入参校验与归一化。

use rusqlite::{Connection, OptionalExtension};

use super::model::PolicyInput;
use crate::error::{AppError, Result};

// ---------------------------------------------------------------------------
// 校验与归一化（创建/编辑共用）
// ---------------------------------------------------------------------------

/// 校验结果（归一化后）：trim 非空、日期规范化、保额/币种成对落定。
pub(super) struct NormalizedInput {
    pub(super) merchant_id: String,
    pub(super) policy_number: String,
    pub(super) product_name: String,
    pub(super) start_date: String,
    pub(super) end_date: Option<String>,
    pub(super) coverage_amount_cents: Option<i64>,
    pub(super) coverage_currency_code: Option<String>,
    pub(super) note: Option<String>,
}

/// 创建/编辑共用的入参校验与归一化：
/// - 保司必须为在用商户（软删商户不可再被新档案选择，历史引用不受影响）；
///   `merchant_unchanged`（编辑路径保司未变）= 维持历史引用，跳过在用校验
///   （同 Writer 接缝 `existing_merchant_id` 语义）；
/// - 保单号/险种名称 trim 非空；
/// - 起止日可解析（YYYY-MM-DD），止日存在时不得早于起日（止日可空 = 长期/终身）；
/// - 保额与币种成对：保额存在时必须 > 0 且币种必填、须存在于币种表；
///   保额缺省时币种忽略存空（两者原子，不产生半挂状态）；
/// - 备注 trim，空串归 `None`。
pub(super) fn validate_input(
    conn: &Connection,
    input: &PolicyInput,
    merchant_unchanged: bool,
) -> Result<NormalizedInput> {
    // 保司：在用商户（ADR-0028 软删语义 + ADR-0051 决策 7）；未换保司 = 保持历史引用。
    if !merchant_unchanged {
        let merchant_active: bool = conn
            .query_row(
                "SELECT 1 FROM merchants WHERE id=?1 AND is_deleted=0",
                rusqlite::params![input.merchant_id],
                |_| Ok(true),
            )
            .optional()?
            .is_some();
        if !merchant_active {
            return Err(AppError::codedp(
                "policy.merchant-not-found",
                format!("保险公司不存在或已删除: {}", input.merchant_id),
                &[&input.merchant_id],
            ));
        }
    }

    let policy_number = input.policy_number.trim();
    if policy_number.is_empty() {
        return Err(AppError::coded("policy.number-required", "保单号不能为空"));
    }
    let product_name = input.product_name.trim();
    if product_name.is_empty() {
        return Err(AppError::coded(
            "policy.product-required",
            "险种名称不能为空",
        ));
    }

    let start_date = parse_date(&input.start_date)?;
    let end_date = match &input.end_date {
        Some(raw) => {
            let date = parse_date(raw)?;
            if date < start_date {
                return Err(AppError::codedp(
                    "policy.end-before-start",
                    format!("保障期间止日 {raw} 早于起日 {}", input.start_date),
                    &[raw, &input.start_date],
                ));
            }
            Some(date.format("%Y-%m-%d").to_string())
        }
        None => None,
    };

    let (coverage_amount_cents, coverage_currency_code) = match input.coverage_amount_cents {
        Some(cents) => {
            if cents <= 0 {
                return Err(AppError::coded("policy.amount-positive", "保额必须大于 0"));
            }
            let code = input.coverage_currency_code.as_deref().ok_or_else(|| {
                AppError::coded("policy.currency-required", "填写保额时必须选择保额币种")
            })?;
            let known: bool = conn
                .query_row(
                    "SELECT 1 FROM currencies WHERE code=?1",
                    rusqlite::params![code],
                    |_| Ok(true),
                )
                .optional()?
                .is_some();
            if !known {
                return Err(AppError::codedp(
                    "policy.currency-not-found",
                    format!("未知币种: {code}"),
                    &[code],
                ));
            }
            (Some(cents), Some(code.to_string()))
        }
        // 保额缺省 → 币种忽略存空（成对原子，不产生只有币种的半挂状态）。
        None => (None, None),
    };

    Ok(NormalizedInput {
        merchant_id: input.merchant_id.clone(),
        policy_number: policy_number.to_string(),
        product_name: product_name.to_string(),
        start_date: start_date.format("%Y-%m-%d").to_string(),
        end_date,
        coverage_amount_cents,
        coverage_currency_code,
        note: input
            .note
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
    })
}

/// 解析 YYYY-MM-DD 日期字符串；非法格式报错（保障期间依赖日历日期）。
pub(super) fn parse_date(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        AppError::codedp(
            "policy.date-invalid",
            format!("日期格式无效，应为 YYYY-MM-DD: {s}"),
            &[s],
        )
    })
}
