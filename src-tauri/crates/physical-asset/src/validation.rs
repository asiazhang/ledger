use rusqlite::{Connection, OptionalExtension};

use super::model::{
    PhysicalAssetDisposeInput, PhysicalAssetInput, PhysicalAssetUpdateInput,
    PhysicalAssetValuationInput,
};
use ledger_infra::error::{AppError, Result};

pub(super) struct NormalizedInput {
    pub(super) name: String,
    pub(super) purchase_date: Option<String>,
    pub(super) purchase_price_cents: Option<i64>,
    pub(super) purchase_currency_code: Option<String>,
    pub(super) initial_valuation_cents: i64,
    pub(super) initial_valuation_currency_code: String,
    pub(super) initial_valuation_date: String,
}

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

fn normalize_valuation_date(raw: Option<&String>) -> Result<String> {
    let date = match raw {
        Some(raw) => {
            let date = parse_date(raw)?;
            reject_future_date(date, raw, &FutureDateKind::Valuation)?;
            date
        }
        None => chrono::Local::now().date_naive(),
    };
    Ok(date.format("%Y-%m-%d").to_string())
}

enum FutureDateKind {
    Valuation,
    Disposal,
}

impl FutureDateKind {
    fn slug(&self) -> &'static str {
        match self {
            FutureDateKind::Valuation => "valuation",
            FutureDateKind::Disposal => "disposal",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            FutureDateKind::Valuation => "估值",
            FutureDateKind::Disposal => "处置",
        }
    }
}

fn reject_future_date(date: chrono::NaiveDate, raw: &str, kind: &FutureDateKind) -> Result<()> {
    let today = chrono::Local::now().date_naive();
    if date > today {
        return Err(AppError::codedp(
            &format!("physical-asset.{}-date-future", kind.slug()),
            format!("{}日期 {raw} 不能是未来", kind.label()),
            &[raw],
        ));
    }
    Ok(())
}

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

pub(super) struct NormalizedUpdateInput {
    pub(super) name: String,
    pub(super) purchase_date: Option<String>,
    pub(super) purchase_price_cents: Option<i64>,
    pub(super) purchase_currency_code: Option<String>,
}

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

pub(super) struct NormalizedValuationInput {
    pub(super) amount_cents: i64,
    pub(super) currency_code: String,
    pub(super) valuation_date: String,
}

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

pub(super) struct NormalizedDisposeInput {
    pub(super) disposal_date: String,
    pub(super) disposal_price_cents: Option<i64>,
    pub(super) disposal_currency_code: Option<String>,
}

pub(super) fn validate_dispose_input(
    conn: &Connection,
    purchase_date: Option<&str>,
    input: &PhysicalAssetDisposeInput,
) -> Result<NormalizedDisposeInput> {
    let raw_date = input.disposal_date.as_deref().ok_or_else(|| {
        AppError::coded("physical-asset.disposal-date-required", "处置日期不能为空")
    })?;
    let disposal_date = parse_date(raw_date)?;
    reject_future_date(disposal_date, raw_date, &FutureDateKind::Disposal)?;
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

pub(super) fn parse_date(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        AppError::codedp(
            "physical-asset.date-invalid",
            format!("日期格式无效，应为 YYYY-MM-DD: {s}"),
            &[s],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn future_date_reject_code_uses_stable_slug_aligned_with_templates() {
        let tomorrow = chrono::Local::now().date_naive() + chrono::Duration::days(1);
        let raw = tomorrow.format("%Y-%m-%d").to_string();

        let err = normalize_valuation_date(Some(&raw)).unwrap_err();
        assert!(err.is_code("physical-asset.valuation-date-future"));
        assert_eq!(err.to_string(), format!("估值日期 {raw} 不能是未来"));

        let err = reject_future_date(tomorrow, &raw, &FutureDateKind::Disposal).unwrap_err();
        assert!(err.is_code("physical-asset.disposal-date-future"));
        assert_eq!(err.to_string(), format!("处置日期 {raw} 不能是未来"));

        let yesterday = chrono::Local::now().date_naive() - chrono::Duration::days(1);
        let past = yesterday.format("%Y-%m-%d").to_string();
        assert!(normalize_valuation_date(Some(&past)).is_ok());
    }
}
