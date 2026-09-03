//! 实物资产入参校验与归一化（issue #466 T1 建档 / issue #467 T2 编辑与更新估值）。
//!
//! 守卫语义（错误码 `physical-asset.<条件>`，kebab-case，ADR-0050）：
//! 缺名称 / 缺估值显式报错不落库；金额一律整数分且必须 > 0；日期可解析
//! （YYYY-MM-DD）且估值日期拒绝未来（估值是已发生的判断）；可选金额与
//! 币种成对（先例保单保额：存在时币种必填且须存在，缺省时币种忽略存空）。
//! 建档首条估值与更新估值同守卫（名称 / 成对 / 金额 / 日期四助手单点共用，
//! T2 拆出），编辑档案只走名称与成对守卫（无估值字段，结构性排除）。

use rusqlite::{Connection, OptionalExtension};

use super::model::{
    PhysicalAssetDisposeInput, PhysicalAssetInput, PhysicalAssetUpdateInput,
    PhysicalAssetValuationInput,
};
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

/// 可选金额与币种成对（建档/编辑购买价 / 处置价共用单点，先例保单保额）：
/// 金额存在时必须 > 0 且币种必填、币种须存在；金额缺省 → 币种忽略存空，
/// 不产生只有币种的半挂状态。错误码 `physical-asset.<condition>-*` 与文案
/// 按 condition（purchase / disposal）与 label（购买价 / 处置价）派生，
/// 调用面语义零变化（T2 拆出的共享助手家族收纳 T3 处置价）。
fn normalize_optional_price(
    conn: &Connection,
    price_cents: Option<i64>,
    currency_code: Option<&str>,
    condition: &str,
    label: &str,
    currency_label: &str,
) -> Result<(Option<i64>, Option<String>)> {
    match price_cents {
        Some(cents) => {
            if cents <= 0 {
                return Err(AppError::coded(
                    &format!("physical-asset.{condition}-price-positive"),
                    format!("{label}必须大于 0"),
                ));
            }
            let code = currency_code.ok_or_else(|| {
                AppError::coded(
                    &format!("physical-asset.{condition}-currency-required"),
                    format!("填写{label}时必须选择{currency_label}币种"),
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
    let date = match raw {
        Some(raw) => {
            let date = parse_date(raw)?;
            reject_future_date(date, raw, "估值")?;
            date
        }
        None => chrono::Local::now().date_naive(),
    };
    Ok(date.format("%Y-%m-%d").to_string())
}

/// 未来日期守卫（估值日期 / 处置日期共用单点）：日期晚于今天即拒绝——
/// 估值与处置都是已发生的判断（先例物品域 disposal-in-future）。错误码
/// `physical-asset.<field>-date-future`，文案 `{field}日期 {raw} 不能是未来`；
/// field 传域语言中文短名（估值 / 处置）。
fn reject_future_date(date: chrono::NaiveDate, raw: &str, field: &str) -> Result<()> {
    let today = chrono::Local::now().date_naive();
    if date > today {
        return Err(AppError::codedp(
            &format!("physical-asset.{field}-date-future"),
            format!("{field}日期 {raw} 不能是未来"),
            &[raw],
        ));
    }
    Ok(())
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

    let (purchase_price_cents, purchase_currency_code) = normalize_optional_price(
        conn,
        input.purchase_price_cents,
        input.purchase_currency_code.as_deref(),
        "purchase",
        "购买价",
        "购买",
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
    let (purchase_price_cents, purchase_currency_code) = normalize_optional_price(
        conn,
        input.purchase_price_cents,
        input.purchase_currency_code.as_deref(),
        "purchase",
        "购买价",
        "购买",
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

/// 处置校验结果（归一化后）：日期 / 成对两件套（形状与
/// [`NormalizedValuationInput`] 同族，字段名直读）。
pub(super) struct NormalizedDisposeInput {
    pub(super) disposal_date: String,
    pub(super) disposal_price_cents: Option<i64>,
    pub(super) disposal_currency_code: Option<String>,
}

/// 处置入参校验与归一化（issue #468 T3）：处置日期必填显式报错（不缺省今天
/// ——处置是显式动作，缺失即录入错误）、可解析、拒绝未来（处置是已发生的
/// 判断，先例估值与物品处置）、不早于购买日期（有购买日期时，先例物品域
/// `item.disposal-before-purchase`）；处置价与币种成对（处置价存在时币种
/// 必填且须存在、处置价必须 > 0，处置价缺省时币种忽略存空）。
pub(super) fn validate_dispose_input(
    conn: &Connection,
    purchase_date: Option<&str>,
    input: &PhysicalAssetDisposeInput,
) -> Result<NormalizedDisposeInput> {
    let raw_date = input.disposal_date.as_deref().ok_or_else(|| {
        AppError::coded("physical-asset.disposal-date-required", "处置日期不能为空")
    })?;
    let disposal_date = parse_date(raw_date)?;
    reject_future_date(disposal_date, raw_date, "处置")?;
    if let Some(purchase) = purchase_date {
        let purchase_date = parse_date(purchase)?;
        if disposal_date < purchase_date {
            return Err(AppError::codedp(
                "physical-asset.disposal-before-purchase",
                format!("处置日期 {disposal_date} 早于购买日期 {purchase_date}，无法处置"),
                &[&disposal_date.to_string(), &purchase_date.to_string()],
            ));
        }
    }
    let (disposal_price_cents, disposal_currency_code) = normalize_optional_price(
        conn,
        input.disposal_price_cents,
        input.disposal_currency_code.as_deref(),
        "disposal",
        "处置价",
        "处置",
    )?;
    Ok(NormalizedDisposeInput {
        disposal_date: disposal_date.format("%Y-%m-%d").to_string(),
        disposal_price_cents,
        disposal_currency_code,
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
