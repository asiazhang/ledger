use rusqlite::Connection;
use rusqlite::OptionalExtension;

use ledger_infra::closed_set;
use ledger_infra::error::{AppError, Result};

const WEEK_START_EXPR: &str = "date(?,'-6 days','weekday 1')";

closed_set! {
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FxRateSource {
    Series => "series",
    Explicit => "explicit",
}
err_label = "折算来源",
err_code = "fx.source-unknown",
}

impl rusqlite::types::FromSql for FxRateSource {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        FxRateSource::parse(value.as_str()?)
            .map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
    }
}

impl utoipa::PartialSchema for FxRateSource {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::Schema> {
        utoipa::openapi::RefOr::T(utoipa::openapi::Schema::Object(
            utoipa::openapi::ObjectBuilder::new()
                .schema_type(utoipa::openapi::Type::String)
                .enum_values(Some(FxRateSource::ALL.map(|s| s.as_str().to_string())))
                .description(Some(
                    "折算来源（闭集：series=汇率历史序列；explicit=调用方显式给定）",
                ))
                .build(),
        ))
    }
}

impl utoipa::ToSchema for FxRateSource {}

impl serde::Serialize for FxRateSource {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for FxRateSource {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FxRateSource::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FxEditBaseline {
    pub amount_cents: i64,
    pub currency_code: String,
    pub date: String,
    pub conversion: NativeConversion,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeConversion {
    pub native_cents: i64,
    pub fx_rate_used: Option<f64>,
    pub fx_rate_source: Option<FxRateSource>,
}

impl NativeConversion {
    pub const fn zero_cash_leg() -> Self {
        NativeConversion {
            native_cents: 0,
            fx_rate_used: None,
            fx_rate_source: None,
        }
    }
}

pub fn default_currency_code(conn: &Connection) -> Result<String> {
    super::base_currency::current_base_currency(conn)
}

fn lookup_exchange_rate(conn: &Connection, base_code: &str, quote_code: &str) -> Result<f64> {
    lookup_exchange_rate_opt(conn, base_code, quote_code)?
        .ok_or_else(|| missing_current_rate_error(base_code, quote_code))
}

fn lookup_exchange_rate_opt(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
) -> Result<Option<f64>> {
    if base_code == quote_code {
        return Ok(Some(1.0));
    }
    if let Ok(rate) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![base_code, quote_code],
        |r| r.get::<_, f64>(0),
    ) {
        if rate <= 0.0 {
            return Err(AppError::codedp(
                "fx.rate-non-positive",
                format!("汇率 {base_code}->{quote_code} 非正: {rate}"),
                &[base_code, quote_code, &rate.to_string()],
            ));
        }
        return Ok(Some(rate));
    }
    if let Ok(rev) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![quote_code, base_code],
        |r| r.get::<_, f64>(0),
    ) {
        if rev <= 0.0 {
            return Err(AppError::codedp(
                "fx.reverse-rate-non-positive",
                format!("反向汇率 {quote_code}->{base_code} 非正: {rev}"),
                &[quote_code, base_code, &rev.to_string()],
            ));
        }
        return Ok(Some(1.0 / rev));
    }
    Ok(None)
}

fn missing_current_rate_error(base_code: &str, quote_code: &str) -> AppError {
    AppError::codedp(
        "fx.rate-missing",
        format!(
            "未找到 {base_code} -> {quote_code} 的汇率（正反向均无），可在设置页「币种」页签同步汇率后重试"
        ),
        &[base_code, quote_code],
    )
}

pub fn convert_to_native_current(
    conn: &Connection,
    amount_cents: i64,
    currency_code: &str,
) -> Result<i64> {
    let target = default_currency_code(conn)?;
    if currency_code == target {
        return Ok(amount_cents);
    }
    let rate = lookup_exchange_rate(conn, currency_code, &target)?;
    Ok((amount_cents as f64 * rate).round() as i64)
}

pub fn try_convert_to_native_current(
    conn: &Connection,
    amount_cents: i64,
    currency_code: &str,
) -> Result<Option<i64>> {
    let target = default_currency_code(conn)?;
    if currency_code == target {
        return Ok(Some(amount_cents));
    }
    Ok(lookup_exchange_rate_opt(conn, currency_code, &target)?
        .map(|rate| (amount_cents as f64 * rate).round() as i64))
}

pub fn convert_to_native_on_trade_date(
    conn: &Connection,
    amount_cents: i64,
    currency_code: &str,
    trade_date: &str,
    explicit_rate: Option<f64>,
) -> Result<NativeConversion> {
    let target = default_currency_code(conn)?;
    if currency_code == target {
        if let Some(rate) = explicit_rate {
            return Err(AppError::codedp(
                "fx.explicit-rate-direction-mismatch",
                format!(
                    "显式汇率方向不符：{currency_code} 即本位币，本笔无折算，不应显式给定汇率（{rate}）"
                ),
                &[currency_code],
            ));
        }
        return Ok(NativeConversion {
            native_cents: amount_cents,
            fx_rate_used: None,
            fx_rate_source: None,
        });
    }
    let (rate, source) = match explicit_rate {
        Some(rate) => {
            if !rate.is_finite() || rate <= 0.0 {
                return Err(AppError::codedp(
                    "fx.explicit-rate-non-positive",
                    format!("显式汇率必须大于 0: {rate}"),
                    &[&rate.to_string()],
                ));
            }
            (rate, FxRateSource::Explicit)
        }
        None => (
            lookup_fx_history_rate(conn, currency_code, &target, trade_date)?,
            FxRateSource::Series,
        ),
    };
    Ok(NativeConversion {
        native_cents: (amount_cents as f64 * rate).round() as i64,
        fx_rate_used: Some(rate),
        fx_rate_source: Some(source),
    })
}

pub fn convert_to_native_on_edit(
    conn: &Connection,
    amount_cents: i64,
    currency_code: &str,
    trade_date: &str,
    explicit_rate: Option<f64>,
    baseline: Option<&FxEditBaseline>,
) -> Result<NativeConversion> {
    if explicit_rate.is_none()
        && let Some(old) = baseline
        && old.amount_cents == amount_cents
        && old.currency_code == currency_code
        && old.date == trade_date
    {
        return Ok(old.conversion);
    }
    convert_to_native_on_trade_date(conn, amount_cents, currency_code, trade_date, explicit_rate)
}

fn lookup_fx_history_rate(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    trade_date: &str,
) -> Result<f64> {
    let week_of = |base: &str, quote: &str| {
        conn.query_row(
            &format!(
                "SELECT rate FROM fx_rate_history WHERE base_code=?1 AND quote_code=?2 \
                 AND week_start = {WEEK_START_EXPR}"
            ),
            rusqlite::params![base, quote, trade_date],
            |r| r.get::<_, f64>(0),
        )
        .optional()
    };
    if let Some(rate) = week_of(base_code, quote_code)? {
        if rate <= 0.0 {
            return Err(AppError::codedp(
                "fx.rate-non-positive",
                format!("汇率 {base_code}->{quote_code} 非正: {rate}"),
                &[base_code, quote_code, &rate.to_string()],
            ));
        }
        return Ok(rate);
    }
    if let Some(rev) = week_of(quote_code, base_code)? {
        if rev <= 0.0 {
            return Err(AppError::codedp(
                "fx.reverse-rate-non-positive",
                format!("反向汇率 {quote_code}->{base_code} 非正: {rev}"),
                &[quote_code, base_code, &rev.to_string()],
            ));
        }
        return Ok(1.0 / rev);
    }
    Err(missing_week_error(conn, base_code, quote_code, trade_date))
}

fn missing_week_error(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    trade_date: &str,
) -> AppError {
    let trade_week: Option<String> = conn
        .query_row(
            &format!("SELECT {WEEK_START_EXPR}"),
            rusqlite::params![trade_date],
            |r| r.get(0),
        )
        .unwrap_or(None);
    let current_week: Option<String> = conn
        .query_row(
            "SELECT date('now','localtime','-6 days','weekday 1')",
            [],
            |r| r.get(0),
        )
        .ok();
    let not_yet_published = matches!((&trade_week, &current_week), (Some(w), Some(c)) if w >= c);
    let week = trade_week.unwrap_or_else(|| trade_date.to_string());
    if not_yet_published {
        AppError::codedp(
            "fx.rate-missing",
            format!(
                "未找到 {base_code} -> {quote_code} 在 {week} 当周的汇率：该周尚未发布，可在设置页「币种」页签同步汇率后重试"
            ),
            &[base_code, quote_code],
        )
    } else {
        AppError::codedp(
            "fx.rate-missing",
            format!("未找到 {base_code} -> {quote_code} 在 {week} 当周的汇率：该周历史空缺"),
            &[base_code, quote_code],
        )
    }
}
