use rusqlite::{Connection, OptionalExtension};

use super::model::PolicyInput;
use ledger_infra::error::{AppError, Result};

pub(super) struct NormalizedInput {
    pub(super) insurer_id: String,
    pub(super) policy_number: String,
    pub(super) product_name: String,
    pub(super) start_date: String,
    pub(super) end_date: Option<String>,
    pub(super) coverage_amount_cents: Option<i64>,
    pub(super) coverage_currency_code: Option<String>,
    pub(super) note: Option<String>,
}

pub(super) fn validate_input(
    conn: &Connection,
    input: &PolicyInput,
    insurer_unchanged: bool,
) -> Result<NormalizedInput> {
    if !insurer_unchanged {
        let insurer_active: bool = conn
            .query_row(
                "SELECT 1 FROM insurers WHERE id=?1 AND is_deleted=0",
                rusqlite::params![input.insurer_id],
                |_| Ok(true),
            )
            .optional()?
            .is_some();
        if !insurer_active {
            return Err(AppError::codedp(
                "policy.insurer-not-found",
                format!("保险公司不存在或已删除: {}", input.insurer_id),
                &[&input.insurer_id],
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
        None => (None, None),
    };

    Ok(NormalizedInput {
        insurer_id: input.insurer_id.clone(),
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

pub(super) fn parse_date(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        AppError::codedp(
            "policy.date-invalid",
            format!("日期格式无效，应为 YYYY-MM-DD: {s}"),
            &[s],
        )
    })
}
