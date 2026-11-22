use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::extract::FromRef;
use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::{DbReadHandle, DbSlotPair, DbWriteHandle};
use ledger_infra::error::AppError;
use ledger_infra::events::SignalEmitter;
use ledger_investment::{Quote, StockRoute};
use rusqlite::Connection;

pub type QuoteFuture = Pin<Box<dyn Future<Output = Result<Quote, AppError>> + Send>>;
pub type FundQuoteFetcher = Arc<dyn Fn(&str) -> QuoteFuture + Send + Sync>;

pub type StockQuoteFetcher = Arc<dyn Fn(StockRoute, &str) -> QuoteFuture + Send + Sync>;

pub type EmitterSlot = Option<Arc<dyn SignalEmitter>>;

#[derive(Clone)]
pub struct ApiState {
    pub conn: Arc<Mutex<Connection>>,
    pub read_conn: Arc<Mutex<Connection>>,
    pub emitter: EmitterSlot,
    pub fund_fetch: Option<FundQuoteFetcher>,
    pub stock_fetch: Option<StockQuoteFetcher>,
    pub lock_gate: EncryptionGate,
    pub boot_gate: BootFailureGate,
}

#[derive(Clone)]
pub struct ReadConn(pub DbReadHandle);

impl FromRef<ApiState> for ReadConn {
    fn from_ref(state: &ApiState) -> Self {
        Self(state.read_handle())
    }
}

impl FromRef<ApiState> for DbWriteHandle {
    fn from_ref(state: &ApiState) -> Self {
        state.write_handle()
    }
}

impl FromRef<ApiState> for EmitterSlot {
    fn from_ref(state: &ApiState) -> Self {
        state.emitter.clone()
    }
}

impl ApiState {
    pub fn slots(&self) -> DbSlotPair {
        DbSlotPair::new(self.conn.clone(), self.read_conn.clone())
    }

    pub fn write_handle(&self) -> DbWriteHandle {
        self.slots().write_handle()
    }

    pub fn read_handle(&self) -> DbReadHandle {
        self.slots().read_handle()
    }
}

#[cfg(test)]
mod quote_seam_guard_tests {

    #[test]
    fn quote_fetch_seams_have_no_blocking_wrapper() {
        let funds = crate::test_support::scan::mask_non_code(include_str!("handlers/funds.rs"));
        let stocks = crate::test_support::scan::mask_non_code(include_str!("handlers/stocks.rs"));

        for (name, text) in [("funds.rs", &funds), ("stocks.rs", &stocks)] {
            assert!(
                !text.contains("spawn_blocking"),
                "{name} 的行情获取接缝不得回归 spawn_blocking 阻塞包装（ADR-0125 决策 7 / \
                 issue #1413）：async 生产入口在异步上下文直接 await"
            );
        }

        let fund_calls = funds.matches("fetch_fund_quote_production(").count();
        assert_eq!(
            fund_calls, 1,
            "基金接缝生产分支应恰直呼一次 fetch_fund_quote_production"
        );
        let stock_calls = stocks.matches("fetch_stock_quote_production(").count();
        assert_eq!(
            stock_calls, 1,
            "股票接缝生产分支应恰直呼一次 fetch_stock_quote_production"
        );
    }

    #[test]
    fn quote_fetcher_types_are_async_shaped() {
        let state = crate::test_support::scan::mask_non_code(include_str!("state.rs"));
        assert!(
            state.contains("pub type QuoteFuture = Pin<Box<dyn Future"),
            "行情获取接缝的 future 装箱类型应在位（QuoteFuture）——接缝闭包为 \
             async 形态（ADR-0125 决策 7 / issue #1413），类型退回同步签名即编译期 \
             拒绝异步生产入口"
        );
        assert!(
            !state.contains("Fn(&str) -> Result<Quote")
                && !state.contains("Fn(&str, &str) -> Result<Quote"),
            "行情获取接缝闭包类型不得退回同步签名（同步闭包是阻塞包装的载体，\
             ADR-0125 决策 7 / issue #1413）"
        );
    }
}
