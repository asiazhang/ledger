//! 实物资产建档入参校验与归一化（issue #466 T1）。
//!
//! 守卫语义（错误码 `physical-asset.<条件>`，kebab-case，ADR-0050）：
//! 缺名称 / 缺估值显式报错不落库；金额一律整数分且必须 > 0；日期可解析
//! （YYYY-MM-DD）且估值日期拒绝未来（估值是已发生的判断）；可选金额与
//! 币种成对（先例保单保额：存在时币种必填且须存在，缺省时币种忽略存空）。

use rusqlite::{Connection, OptionalExtension};

use super::model::PhysicalAssetInput;
use crate::error::{AppError, Result};

// ---------------------------------------------------------------------------
// 校验与归一化（建档）
// ---------------------------------------------------------------------------

/// 校验结果（归一化后）：trim 非空、日期规范化、可选金额与币种成对落定。
pub(super) struct NormalizedInput {
    pub(super) name: String,
    pub(super) purchase_date: Option<String>,
    pub(super) purchase_price_cents: Option<i64>,
    pub(super) purchase_currency_code: Option<String>,
    pub(super) initial_valuation_cents: i64,
    pub(super) initial_valuation_currency_code: String,
    pub(super) initial_valuation_date: String,
}

/// 建档入参校验与归一化。估值日期缺省取今天（本地日期）；today 在域内取
/// 当前本地日期（外部语义即「建档当天的估值」，先例物品使用成本取今天实时）。
pub(super) fn validate_input(
    conn: &Connection,
    input: &PhysicalAssetInput,
) -> Result<NormalizedInput> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::coded(
            "physical-asset.name-required",
            "资产名称不能为空",
        ));
    }

    let purchase_date = match &input.purchase_date {
        Some(raw) => Some(parse_date(raw)?),
        None => None,
    };

    // 购买价与币种成对（先例保单保额）：购买价存在时币种必填且须存在；
    // 购买价缺省 → 币种忽略存空，不产生只有币种的半挂状态。
    let (purchase_price_cents, purchase_currency_code) = match input.purchase_price_cents {
        Some(cents) => {
            if cents <= 0 {
                return Err(AppError::coded(
                    "physical-asset.purchase-price-positive",
                    "购买价必须大于 0",
                ));
            }
            let code = input.purchase_currency_code.as_deref().ok_or_else(|| {
                AppError::coded(
                    "physical-asset.purchase-currency-required",
                    "填写购买价时必须选择购买币种",
                )
            })?;
            require_currency(conn, code)?;
            (Some(cents), Some(code.to_string()))
        }
        None => (None, None),
    };

    // 当前估值必填：缺失显式报错（不默认 0），且必须为正。
    let initial_valuation_cents = input
        .initial_valuation_cents
        .ok_or_else(|| AppError::coded("physical-asset.valuation-required", "当前估值不能为空"))?;
    if initial_valuation_cents <= 0 {
        return Err(AppError::coded(
            "physical-asset.valuation-positive",
            "当前估值必须大于 0",
        ));
    }

    let initial_valuation_currency_code = input
        .initial_valuation_currency_code
        .as_deref()
        .ok_or_else(|| {
            AppError::coded(
                "physical-asset.valuation-currency-required",
                "当前估值必须选择币种",
            )
        })?;
    require_currency(conn, initial_valuation_currency_code)?;

    // 估值日期：缺省今天；可补过去（事后整理），拒绝未来（估值是已发生的判断）。
    let today = chrono::Local::now().date_naive();
    let initial_valuation_date = match &input.initial_valuation_date {
        Some(raw) => {
            let date = parse_date(raw)?;
            if date > today {
                return Err(AppError::codedp(
                    "physical-asset.valuation-date-future",
                    format!("估值日期 {raw} 不能是未来"),
                    &[raw],
                ));
            }
            date
        }
        None => today,
    };

    Ok(NormalizedInput {
        name: name.to_string(),
        purchase_date: purchase_date.map(|d| d.format("%Y-%m-%d").to_string()),
        purchase_price_cents,
        purchase_currency_code,
        initial_valuation_cents,
        initial_valuation_currency_code: initial_valuation_currency_code.to_string(),
        initial_valuation_date: initial_valuation_date.format("%Y-%m-%d").to_string(),
    })
}

/// 币种须存在于币种字典（种子权威，只读参考表）。
fn require_currency(conn: &Connection, code: &str) -> Result<()> {
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
            "physical-asset.currency-not-found",
            format!("未知币种: {code}"),
            &[code],
        ));
    }
    Ok(())
}

/// 解析 YYYY-MM-DD 日期字符串；非法格式报错（估值日期与购买日期依赖日历日期）。
pub(super) fn parse_date(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        AppError::codedp(
            "physical-asset.date-invalid",
            format!("日期格式无效，应为 YYYY-MM-DD: {s}"),
            &[s],
        )
    })
}
