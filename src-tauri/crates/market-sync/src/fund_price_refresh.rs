use chrono::NaiveDate;

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};
use ledger_investment::prices::{
    MarketPriceWrite, SINA_PRICE_SOURCE, price_value_to_cents, upsert_market_price,
};

use super::bulk::BulkNavPoint;
use super::channels::FetchFuture;
use super::fund_nav::{
    FundIdentity, NavPoint, mark_constant_price_on_confirm, nav_window, read_fund_watermark,
    trim_to_window,
};
use super::session::ScopedSession;
use super::weekly::{commit_price_history_weekly, week_monday};

const REFRESH_RECENT_WINDOW_MONTHS: chrono::Months = chrono::Months::new(1);

pub(super) struct FundSyncStats {
    pub(super) synced: usize,
    pub(super) skipped: usize,
    pub(super) written: usize,
}

pub(super) async fn refresh_one_fund_price<Q, H, C>(
    session: &Q,
    fund: &FundIdentity<'_>,
    latest_hint: Option<&BulkNavPoint>,
    fetch_nav_history: &mut H,
    confirm_money_fund: &mut C,
    stats: &mut FundSyncStats,
) -> Result<()>
where
    Q: ScopedSession,
    H: FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send,
    C: FnMut(&str) -> FetchFuture<bool> + Send,
{
    let today = super::incremental::beijing_today();
    let (watermark, has_history) = read_fund_watermark(session, fund.instrument_id).await?;
    match bulk_decision(latest_hint, watermark.as_deref()) {
        BulkDecision::NothingNew => {
            stats.synced += 1;
            return Ok(());
        }
        BulkDecision::LandFromBulk(hint) => {
            land_bulk_point(session, fund, hint).await?;
            stats.synced += 1;
            stats.written += 1;
            return Ok(());
        }
        BulkDecision::PerInstrument => {
            if let Some(hint) = latest_hint {
                tracing::debug!(
                    code = %fund.code, watermark = ?watermark, bulk_date = %hint.date,
                    "批量面已报出新净值但水位落后超过一个自然周，逐只通道补齐缺失周点"
                );
            }
        }
    }
    match confirm_money_fund(fund.code).await {
        Ok(true) => {
            let written = mark_constant_price_on_confirm(
                session,
                fund.instrument_id,
                fund.currency,
                today.format("%Y-%m-%d").to_string(),
            )
            .await?;
            if written {
                stats.written += 1;
            }
            stats.synced += 1;
            return Ok(());
        }
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(
                code = %fund.code, %error,
                "官方披露判定源不可信，本轮整只不落（信号缺席时落取值位即万份收益冒充净值）"
            );
            stats.skipped += 1;
            return Ok(());
        }
    }
    let (start, end) = if has_history {
        nav_window(watermark.as_deref(), today)
    } else {
        let start = today
            .checked_sub_months(REFRESH_RECENT_WINDOW_MONTHS)
            .unwrap_or(today);
        (
            start.format("%Y-%m-%d").to_string(),
            today.format("%Y-%m-%d").to_string(),
        )
    };
    let points: Vec<NavPoint> = fetch_nav_history(fund.code).await?;
    let collected: Vec<NavPoint> = trim_to_window(points, &start, &end);

    if collected.is_empty() {
        if has_history {
            stats.synced += 1;
        } else {
            stats.skipped += 1;
        }
        return Ok(());
    }
    let Some(latest) = collected.iter().max_by_key(|p| p.date.as_str()) else {
        tracing::warn!(code = %fund.code, "净值点意外为空，跳过现价更新");
        return Ok(());
    };
    if watermark
        .as_deref()
        .is_some_and(|w| latest.date.as_str() <= w)
    {
        stats.synced += 1;
        return Ok(());
    }
    let daily_points: Vec<(NaiveDate, f64)> = collected
        .iter()
        .filter_map(|p| Some((NaiveDate::parse_from_str(&p.date, "%Y-%m-%d").ok()?, p.nav)))
        .collect();
    let instrument_id = fund.instrument_id.to_string();
    let currency = fund.currency.to_string();
    let latest_date = latest.date.clone();
    let latest_nav = latest.nav;
    session
        .with_connection(move |conn| {
            ensure_transaction(conn, || {
                if has_history {
                    commit_price_history_weekly(
                        conn,
                        &instrument_id,
                        &currency,
                        SINA_PRICE_SOURCE,
                        &daily_points,
                    )?;
                }
                upsert_market_price(
                    conn,
                    &MarketPriceWrite {
                        instrument_id: &instrument_id,
                        price_cents: price_value_to_cents(latest_nav),
                        currency_code: &currency,
                        priced_at: &latest_date,
                        nav_date: Some(&latest_date),
                        source: Some(SINA_PRICE_SOURCE),
                    },
                )?;
                Ok(())
            })
        })
        .await?;
    stats.synced += 1;
    stats.written += 1;
    Ok(())
}

enum BulkDecision<'a> {
    NothingNew,
    LandFromBulk(&'a BulkNavPoint),
    PerInstrument,
}

fn bulk_decision<'a>(
    latest_hint: Option<&'a BulkNavPoint>,
    watermark: Option<&str>,
) -> BulkDecision<'a> {
    let Some(hint) = latest_hint else {
        return BulkDecision::PerInstrument;
    };
    if hint.is_not_newer_than(watermark) {
        return BulkDecision::NothingNew;
    }
    if week_gap_needs_per_instrument(watermark, &hint.date) {
        return BulkDecision::PerInstrument;
    }
    BulkDecision::LandFromBulk(hint)
}

fn week_gap_needs_per_instrument(watermark: Option<&str>, bulk_date: &str) -> bool {
    let parse = |date: &str| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok();
    let (Some(watermark), Some(bulk_date)) = (watermark.and_then(parse), parse(bulk_date)) else {
        return true;
    };
    week_monday(bulk_date) - week_monday(watermark) > chrono::Duration::days(7)
}

async fn land_bulk_point<Q: ScopedSession>(
    session: &Q,
    fund: &FundIdentity<'_>,
    hint: &BulkNavPoint,
) -> Result<()> {
    let price_cents = price_value_to_cents(hint.nav);
    let day = NaiveDate::parse_from_str(hint.date.trim(), "%Y-%m-%d")
        .map_err(|e| AppError::Parse(format!("非法净值日期 {}: {e}", hint.date)))?;
    let nav = hint.nav;
    let instrument_id = fund.instrument_id.to_string();
    let currency = fund.currency.to_string();
    let hint_date = hint.date.clone();
    session
        .with_connection(move |conn| {
            ensure_transaction(conn, || {
                upsert_market_price(
                    conn,
                    &MarketPriceWrite {
                        instrument_id: &instrument_id,
                        price_cents,
                        currency_code: &currency,
                        priced_at: &hint_date,
                        nav_date: Some(&hint_date),
                        source: Some(SINA_PRICE_SOURCE),
                    },
                )?;
                if ledger_investment::backfill::has_any_history(conn, &instrument_id)? {
                    commit_price_history_weekly(
                        conn,
                        &instrument_id,
                        &currency,
                        SINA_PRICE_SOURCE,
                        &[(day, nav)],
                    )?;
                }
                Ok(())
            })
        })
        .await?;
    Ok(())
}
