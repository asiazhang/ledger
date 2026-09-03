//! 实物资产入参校验与归一化（issue #466 T1 建档 / issue #467 T2 编辑与更新估值）。
//!
//! 守卫语义（错误码 `physical-asset.<条件>`，kebab-case，ADR-0050）：
//! 缺名称 / 缺估值显式报错不落库；金额一律整数分且必须 > 0；日期可解析
//! （YYYY-MM-DD）且估值日期拒绝未来（估值是已发生的判断）；可选金额与
//! 币种成对（先例保单保额：存在时币种必填且须存在，缺省时币种忽略存空）。
//! 建档首条估值与更新估值同守卫（名称 / 成对 / 金额 / 日期四助手单点共用，
//! T2 拆出），编辑档案只走名称与成对守卫（无估值字段，结构性排除）。

use rusqlite::{Connection, OptionalExtension};

use super::model::{PhysicalAssetInput, PhysicalAssetUpdateInput, PhysicalAssetValuationInput};
use crate::error::{AppError, Result};

// ---------------------------------------------------------------------------
// 校验与归一化（建档 / 编辑 / 更新估值共守卫，issue #467 T2 拆出共享助手）
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

/// 名称守卫（建档 / 编辑共用单点）：trim 非空，返回归一化后的名称。
fn normalize_name(raw: &str) -> Result<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::coded(
            "physical-asset.name-required",
            "资产名称不能为空",
        ));
    }
    Ok(name.to_string())
}

/// 购买价与币种成对（建档 / 编辑共用单点，先例保单保额）：购买价存在时
/// 币种必填且须存在；购买价缺省 → 币种忽略存空，不产生只有币种的半挂状态。
fn normalize_purchase(
    conn: &Connection,
    price_cents: Option<i64>,
    currency_code: Option<&str>,
) -> Result<(Option<i64>, Option<String>)> {
    match price_cents {
        Some(cents) => {
            if cents <= 0 {
                return Err(AppError::coded(
                    "physical-asset.purchase-price-positive",
                    "购买价必须大于 0",
                ));
            }
            let code = currency_code.ok_or_else(|| {
                AppError::coded(
                    "physical-asset.purchase-currency-required",
                    "填写购买价时必须选择购买币种",
                )
            })?;
            require_currency(conn, code)?;
            Ok((Some(cents), Some(code.to_string())))
        }
        None => Ok((None, None)),
    }
}

/// 估值金额守卫（建档首条估值 / 更新估值共用单点）：必填显式报错（不默认 0）
/// 且必须为正；币种必填且须存在。
fn normalize_amount_and_currency(
    conn: &Connection,
    amount_cents: Option<i64>,
    currency_code: Option<&str>,
) -> Result<(i64, String)> {
    let cents = amount_cents
        .ok_or_else(|| AppError::coded("physical-asset.valuation-required", "当前估值不能为空"))?;
    if cents <= 0 {
        return Err(AppError::coded(
            "physical-asset.valuation-positive",
            "当前估值必须大于 0",
        ));
    }
    let code = currency_code.ok_or_else(|| {
        AppError::coded(
            "physical-asset.valuation-currency-required",
            "当前估值必须选择币种",
        )
    })?;
    require_currency(conn, code)?;
    Ok((cents, code.to_string()))
}

/// 估值日期守卫（建档首条估值 / 更新估值共用单点）：缺省今天；可补过去
/// （事后整理），拒绝未来（估值是已发生的判断）。today 在域内取当前本地日期。
fn normalize_valuation_date(raw: Option<&String>) -> Result<String> {
    let today = chrono::Local::now().date_naive();
    let date = match raw {
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
    Ok(date.format("%Y-%m-%d").to_string())
}

/// 建档入参校验与归一化。估值日期缺省取今天（本地日期）；today 在域内取
/// 当前本地日期（外部语义即「建档当天的估值」，先例物品使用成本取今天实时）。
pub(super) fn validate_input(
    conn: &Connection,
    input: &PhysicalAssetInput,
) -> Result<NormalizedInput> {
    let name = normalize_name(&input.name)?;

    let purchase_date = match &input.purchase_date {
        Some(raw) => Some(parse_date(raw)?),
        None => None,
    };

    let (purchase_price_cents, purchase_currency_code) = normalize_purchase(
        conn,
        input.purchase_price_cents,
        input.purchase_currency_code.as_deref(),
    )?;

    let (initial_valuation_cents, initial_valuation_currency_code) = normalize_amount_and_currency(
        conn,
        input.initial_valuation_cents,
        input.initial_valuation_currency_code.as_deref(),
    )?;
    let initial_valuation_date = normalize_valuation_date(input.initial_valuation_date.as_ref())?;

    Ok(NormalizedInput {
        name,
        purchase_date: purchase_date.map(|d| d.format("%Y-%m-%d").to_string()),
        purchase_price_cents,
        purchase_currency_code,
        initial_valuation_cents,
        initial_valuation_currency_code,
        initial_valuation_date,
    })
}

/// 编辑档案校验结果（归一化后）：仅名称与购买信息（无估值字段）。
pub(super) struct NormalizedUpdateInput {
    pub(super) name: String,
    pub(super) purchase_date: Option<String>,
    pub(super) purchase_price_cents: Option<i64>,
    pub(super) purchase_currency_code: Option<String>,
}

/// 编辑档案入参校验与归一化（issue #467 T2）：仅名称与购买信息，与建档
/// 同源守卫（名称 trim 非空、购买成对）；无估值字段，估值不经本入口变更。
pub(super) fn validate_update_input(
    conn: &Connection,
    input: &PhysicalAssetUpdateInput,
) -> Result<NormalizedUpdateInput> {
    let name = normalize_name(&input.name)?;
    let purchase_date = match &input.purchase_date {
        Some(raw) => Some(parse_date(raw)?.format("%Y-%m-%d").to_string()),
        None => None,
    };
    let (purchase_price_cents, purchase_currency_code) = normalize_purchase(
        conn,
        input.purchase_price_cents,
        input.purchase_currency_code.as_deref(),
    )?;
    Ok(NormalizedUpdateInput {
        name,
        purchase_date,
        purchase_price_cents,
        purchase_currency_code,
    })
}

/// 更新估值校验结果（归一化后）：金额 / 币种 / 日期三件套（形状与
/// [`NormalizedInput`] / [`NormalizedUpdateInput`] 同族，字段名直读）。
pub(super) struct NormalizedValuationInput {
    pub(super) amount_cents: i64,
    pub(super) currency_code: String,
    pub(super) valuation_date: String,
}

/// 更新估值入参校验与归一化（issue #467 T2）：金额必填且 > 0、币种必填且
/// 存在、日期缺省今天 / 可补过去 / 拒未来——与建档首条估值同源守卫。
pub(super) fn validate_valuation_input(
    conn: &Connection,
    input: &PhysicalAssetValuationInput,
) -> Result<NormalizedValuationInput> {
    let (amount_cents, currency_code) =
        normalize_amount_and_currency(conn, input.amount_cents, input.currency_code.as_deref())?;
    let valuation_date = normalize_valuation_date(input.valuation_date.as_ref())?;
    Ok(NormalizedValuationInput {
        amount_cents,
        currency_code,
        valuation_date,
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
