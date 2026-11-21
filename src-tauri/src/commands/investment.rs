#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_currencies::{ExchangeRate, ExchangeRateInput};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::{WriteEvidence, WriteOp};
use ledger_investment as investment_domain;
use ledger_investment::{
    AddFundResult, AddStockInstrumentResult, CumulativePnlNativeTotal, CurrencyCumulativePnl,
    Holding, InstrumentInput, InstrumentListFilter, InstrumentListResult, InvestmentOverview,
    InvestmentTransactionListFilter, InvestmentTransactionListResult, ManualPriceInput,
    ManualPriceResult, MarketPrice, MarketPriceInput, MoneyWeightedReturnSummary, MwrRange,
    PnlFilter, PortfolioValueTrend, PriceStaleness, RealizedPnlSummary, StockRoute,
    TransactionConvert, TransactionSplit, TransactionTrade, TrendRange,
};

#[tauri::command]
pub async fn list_holdings(db: State<'_, DbState>) -> Result<Vec<Holding>> {
    let conn = db.read_handle();
    read_entry("list_holdings", conn, move |conn| {
        investment_domain::list_holdings(conn)
    })
    .await
}

#[tauri::command]
pub async fn investment_overview(db: State<'_, DbState>) -> Result<InvestmentOverview> {
    let conn = db.read_handle();
    read_entry("investment_overview", conn, move |conn| {
        investment_domain::query_investment_overview(conn)
    })
    .await
}

#[tauri::command]
pub async fn instrument_price_staleness(db: State<'_, DbState>) -> Result<PriceStaleness> {
    let conn = db.read_handle();
    read_entry("instrument_price_staleness", conn, move |conn| {
        investment_domain::instrument_price_staleness(conn)
    })
    .await
}

#[tauri::command]
pub async fn portfolio_value_trend(
    db: State<'_, DbState>,
    filter: Option<TrendRange>,
) -> Result<PortfolioValueTrend> {
    let conn = db.read_handle();
    read_entry("portfolio_value_trend", conn, move |conn| {
        investment_domain::query_portfolio_value_trend(conn, &filter.unwrap_or_default())
    })
    .await
}

#[tauri::command]
pub async fn realized_pnl_summary(
    db: State<'_, DbState>,
    filter: Option<PnlFilter>,
) -> Result<RealizedPnlSummary> {
    let conn = db.read_handle();
    read_entry("realized_pnl_summary", conn, move |conn| {
        let filter = filter.unwrap_or(PnlFilter {
            account_id: None,
            instrument_id: None,
        });
        investment_domain::query_realized_pnl_summary(conn, &filter)
    })
    .await
}

#[tauri::command]
pub async fn cumulative_pnl_summary(db: State<'_, DbState>) -> Result<Vec<CurrencyCumulativePnl>> {
    let conn = db.read_handle();
    read_entry("cumulative_pnl_summary", conn, move |conn| {
        investment_domain::query_cumulative_pnl_summary(conn)
    })
    .await
}

#[tauri::command]
pub async fn cumulative_pnl_native_total(
    db: State<'_, DbState>,
) -> Result<CumulativePnlNativeTotal> {
    let conn = db.read_handle();
    read_entry("cumulative_pnl_native_total", conn, move |conn| {
        investment_domain::query_cumulative_pnl_native_total(conn)
    })
    .await
}

#[tauri::command]
pub async fn money_weighted_return_summary(
    db: State<'_, DbState>,
    range: Option<MwrRange>,
) -> Result<MoneyWeightedReturnSummary> {
    let conn = db.read_handle();
    read_entry("money_weighted_return_summary", conn, move |conn| {
        investment_domain::query_money_weighted_return_summary(conn, &range.unwrap_or_default())
    })
    .await
}

#[tauri::command]
pub async fn list_exchange_rates(db: State<'_, DbState>) -> Result<Vec<ExchangeRate>> {
    let conn = db.read_handle();
    read_entry("list_exchange_rates", conn, move |conn| {
        investment_domain::list_exchange_rates(conn)
    })
    .await
}

#[tauri::command]
pub async fn create_exchange_rate(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: ExchangeRateInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_exchange_rate",
        conn,
        Some(&app),
        WriteOp::CreateExchangeRate,
        move |conn| investment_domain::create_exchange_rate(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn list_market_prices(db: State<'_, DbState>) -> Result<Vec<MarketPrice>> {
    let conn = db.read_handle();
    read_entry("list_market_prices", conn, move |conn| {
        investment_domain::list_market_prices(conn)
    })
    .await
}

#[tauri::command]
pub async fn create_market_price(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: MarketPriceInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_market_price",
        conn,
        Some(&app),
        WriteOp::CreateMarketPrice,
        move |conn| investment_domain::create_market_price(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn list_instruments(
    db: State<'_, DbState>,
    filter: Option<InstrumentListFilter>,
) -> Result<InstrumentListResult> {
    let conn = db.read_handle();
    read_entry("list_instruments", conn, move |conn| {
        let filter = filter.unwrap_or_default();
        investment_domain::list_instruments(conn, &filter)
    })
    .await
}

#[tauri::command]
pub async fn delete_instrument(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    let conn = db.write_handle();
    write_entry(
        "delete_instrument",
        conn,
        Some(&app),
        WriteOp::DeleteInstrument,
        move |conn| investment_domain::delete_instrument(conn, &id).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn list_investment_transactions(
    db: State<'_, DbState>,
    filter: Option<InvestmentTransactionListFilter>,
) -> Result<InvestmentTransactionListResult> {
    let conn = db.read_handle();
    read_entry("list_investment_transactions", conn, move |conn| {
        let filter = filter.unwrap_or_default();
        investment_domain::list_investment_transactions(conn, &filter)
    })
    .await
}

#[tauri::command]
pub async fn get_transaction_trade(db: State<'_, DbState>, id: String) -> Result<TransactionTrade> {
    let conn = db.read_handle();
    read_entry("get_transaction_trade", conn, move |conn| {
        investment_domain::get_transaction_trade(conn, &id)
    })
    .await
}

#[tauri::command]
pub async fn get_transaction_convert(
    db: State<'_, DbState>,
    id: String,
) -> Result<TransactionConvert> {
    let conn = db.read_handle();
    read_entry("get_transaction_convert", conn, move |conn| {
        investment_domain::get_transaction_convert(conn, &id)
    })
    .await
}

#[tauri::command]
pub async fn get_transaction_split(db: State<'_, DbState>, id: String) -> Result<TransactionSplit> {
    let conn = db.read_handle();
    read_entry("get_transaction_split", conn, move |conn| {
        investment_domain::get_transaction_split(conn, &id)
    })
    .await
}

#[tauri::command]
pub async fn add_fund_by_code(
    db: tauri::State<'_, DbState>,
    app: tauri::AppHandle,
    code: String,
) -> Result<AddFundResult> {
    investment_domain::validate_fund_code(&code)?;
    let conn = db.write_handle();
    let quote = ledger_market_sync::fetch_fund_quote_production(&code).await?;
    write_entry(
        "add_fund_by_code",
        conn,
        Some(&app),
        WriteOp::AddFundByCode,
        move |conn| {
            let mut fetch = |_: &str, _: &str| Ok(quote.clone());
            investment_domain::add_fund_by_code_with(conn, &code, &mut fetch).map(|result| {
                let evidence = WriteEvidence::PriceWritten(result.price_written);
                Outcome::Evidenced(result, evidence)
            })
        },
    )
    .await
}

#[tauri::command]
pub async fn add_instrument_by_code(
    db: tauri::State<'_, DbState>,
    app: tauri::AppHandle,
    market: String,
    code: String,
) -> Result<AddStockInstrumentResult> {
    let conn = db.write_handle();
    let mut fetch = |code: &str, route: StockRoute| {
        let code = code.to_string();
        async move { ledger_market_sync::fetch_stock_quote_production(route, &code).await }
    };
    let quote = investment_domain::fetch_stock_quote_for_add(&market, &code, &mut fetch).await?;
    write_entry(
        "add_instrument_by_code",
        conn,
        Some(&app),
        WriteOp::AddInstrumentByCode,
        move |conn| {
            investment_domain::add_stock_instrument_with_quote(conn, &quote).map(|result| {
                let evidence = WriteEvidence::PriceWritten(result.price_written);
                Outcome::Evidenced(result, evidence)
            })
        },
    )
    .await
}

#[tauri::command]
pub async fn create_instrument(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: InstrumentInput,
) -> Result<String> {
    let conn = db.write_handle();
    write_entry(
        "create_instrument",
        conn,
        Some(&app),
        WriteOp::CreateInstrument,
        move |conn| investment_domain::create_instrument_manual(conn, input).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn record_manual_price(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: ManualPriceInput,
) -> Result<ManualPriceResult> {
    let conn = db.write_handle();
    write_entry(
        "record_manual_price",
        conn,
        Some(&app),
        WriteOp::RecordManualPrice,
        move |conn| {
            investment_domain::record_manual_price(conn, &input).map(|outcome| {
                let evidence = WriteEvidence::PriceWritten(outcome.any_written());
                Outcome::Evidenced(outcome, evidence)
            })
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use crate::test_support::scan::matching_brace_end;

    fn command_body<'a>(masked: &'a str, signature: &str) -> &'a str {
        let anchor = masked
            .find(signature)
            .unwrap_or_else(|| panic!("命令 {signature} 应在位"));
        let open = anchor + masked[anchor..].find('{').expect("命令体应有大括号");
        let end = matching_brace_end(masked, open).expect("命令体花括号应配对");
        &masked[open..end]
    }

    #[test]
    fn fund_fetch_happens_before_write_entry_in_add_fund_by_code() {
        let text = crate::test_support::scan::mask_non_code(include_str!("investment.rs"));
        let body = command_body(&text, "pub async fn add_fund_by_code(");
        let write_at = body
            .find("write_entry(")
            .expect("落库应经统一写入口 write_entry");
        let fetch_count = body.matches("fetch_fund_quote_production(").count();
        assert_eq!(fetch_count, 1, "行情拉取入口在命令体内应恰出现一次");
        let fetch_at = body
            .find("fetch_fund_quote_production(")
            .expect("行情拉取入口应在命令体内");
        assert!(
            fetch_at < write_at,
            "行情拉取（单请求叠加限流冷却重试最长可达分钟级）必须在 write_entry \
             之前完成——移回统一写入口闭包即在连接锁内执行网络等待，阻塞全应用 \
             IPC/HTTP 读写（ADR-0069 决策 4 / issue #1282）"
        );
        assert!(
            !body.contains("spawn_blocking"),
            "add_fund_by_code 不得回归 spawn_blocking 阻塞包装（ADR-0125 决策 7 / \
             issue #1413）：async 生产入口在命令体直接 await"
        );
    }

    #[test]
    fn instrument_query_uses_async_production_entry_without_blocking_wrapper() {
        let text = crate::test_support::scan::mask_non_code(include_str!("investment.rs"));
        let body = command_body(&text, "pub async fn add_instrument_by_code(");
        assert_eq!(
            body.matches("fetch_stock_quote_production(").count(),
            1,
            "股票生产拉取入口在命令体内应恰出现一次（查询闭包直呼生产入口，接线不被绕过）"
        );
        assert!(
            body.contains("fetch_stock_quote_for_add("),
            "查询编排应经投资域接缝 fetch_stock_quote_for_add（spec #690 唯一接缝）"
        );
        assert!(
            !body.contains("spawn_blocking"),
            "add_instrument_by_code 不得回归 spawn_blocking 阻塞包装（ADR-0125 决策 7 / \
             issue #1413）：async 生产入口在命令体直接 await"
        );
    }
}
