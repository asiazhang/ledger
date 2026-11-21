use super::http::{ForegroundGuard, Pacer, build_client, lock_pacer, shared_pacer};
use super::tencent::{TENCENT_QUOTE_HOSTS, fetch_tencent_batch, tencent_query_key};
use ledger_infra::error::{AppError, Result};
use ledger_investment::{Quote, StockRoute};

pub(super) async fn fetch_stock_quote(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    route: StockRoute,
    code: &str,
) -> Result<Quote> {
    let key = tencent_query_key(route, code);
    let quotes = fetch_tencent_batch(client, pacer, hosts, &key).await?;
    match quotes.into_iter().find(|quote| quote.code == code) {
        Some(quote) => Ok(quote.into_quote()),
        None => Err(AppError::codedp(
            "sync.stock-not-found",
            format!("查无股票代码 {code}，请核对后重试"),
            &[code],
        )),
    }
}

pub async fn fetch_stock_quote_production(route: StockRoute, code: &str) -> Result<Quote> {
    let _foreground = ForegroundGuard::enter();
    let client = build_client()?;
    let pacer = shared_pacer();
    let mut pacer = lock_pacer(&pacer).await;
    fetch_stock_quote(&client, &mut pacer, TENCENT_QUOTE_HOSTS, route, code).await
}
