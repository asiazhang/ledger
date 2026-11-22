use chrono::NaiveDate;

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_investment::PriceChannel;
use ledger_investment::prices::{
    MarketPriceWrite, SINA_PRICE_SOURCE, price_value_to_cents, upsert_market_price,
};

use super::channels::FetchFuture;
use super::fund_nav::{
    FundIdentity, NavPoint, coverage_short_of_first_position, deep_backfill_window,
    mark_constant_price_on_confirm, nav_window, read_fund_coverage, read_fund_watermark,
    trim_to_window,
};
use super::session::ScopedSession;
use super::weekly::commit_price_history_weekly;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BackfillOutcome {
    pub(super) written: bool,
    pub(super) inconclusive: bool,
}

pub(super) async fn backfill_one_fund_history<Q, H, C>(
    session: &Q,
    fund: &FundIdentity<'_>,
    fetch_nav_history: &mut H,
    confirm_money_fund: &mut C,
) -> Result<BackfillOutcome>
where
    Q: ScopedSession,
    H: FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send,
    C: FnMut(&str) -> FetchFuture<bool> + Send,
{
    let today = super::incremental::beijing_today();
    let (watermark, has_history) = read_fund_watermark(session, fund.instrument_id).await?;
    let (earliest_history, first_position_date) =
        read_fund_coverage(session, fund.instrument_id).await?;
    let deep = !has_history
        || coverage_short_of_first_position(
            earliest_history.as_deref(),
            first_position_date.as_deref(),
        );
    let (start, end) = if deep {
        deep_backfill_window(first_position_date.as_deref(), today)
    } else {
        nav_window(watermark.as_deref(), today)
    };

    if fund.channel != PriceChannel::Constant {
        match confirm_money_fund(fund.code).await {
            Ok(true) => {
                let written = mark_constant_price_on_confirm(
                    session,
                    fund.instrument_id,
                    fund.currency,
                    today.format("%Y-%m-%d").to_string(),
                )
                .await?;
                return Ok(BackfillOutcome {
                    written,
                    inconclusive: false,
                });
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(
                    code = %fund.code, %error,
                    "官方披露判定源不可信，本轮整只不落（信号缺席时落取值位即万份收益冒充净值）"
                );
                return Ok(BackfillOutcome {
                    written: false,
                    inconclusive: true,
                });
            }
        }
    }

    let points: Vec<NavPoint> = fetch_nav_history(fund.code).await?;
    let collected: Vec<NavPoint> = trim_to_window(points, &start, &end);

    if collected.is_empty() {
        return Ok(BackfillOutcome {
            written: false,
            inconclusive: false,
        });
    }

    let Some(latest) = collected.iter().max_by_key(|p| p.date.as_str()) else {
        tracing::warn!(code = %fund.code, "净值点意外为空，跳过现价更新");
        return Ok(BackfillOutcome {
            written: false,
            inconclusive: false,
        });
    };

    let daily_points: Vec<(NaiveDate, f64)> = collected
        .iter()
        .filter_map(|p| Some((NaiveDate::parse_from_str(&p.date, "%Y-%m-%d").ok()?, p.nav)))
        .collect();
    let instrument_id = fund.instrument_id.to_string();
    let currency = fund.currency.to_string();
    let latest_date = latest.date.clone();
    let latest_nav = latest.nav;
    let written: bool = session
        .with_connection(move |conn| {
            ensure_transaction(conn, || {
                if !commit_price_history_weekly(
                    conn,
                    &instrument_id,
                    &currency,
                    SINA_PRICE_SOURCE,
                    &daily_points,
                )? {
                    return Ok(false);
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
                Ok(true)
            })
        })
        .await?;
    Ok(BackfillOutcome {
        written,
        inconclusive: false,
    })
}
