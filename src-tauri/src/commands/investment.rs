//! IPC 命令壳 · 投资（Instrument / Holding / Trade / Trend，#401 域目录化
//! ADR-0056）：标的字典、市场数据录入、基金接入、手动报价、持仓 / 走势 / 盈亏
//! 与买卖明细查询命令。
//!
//! 只做参数解包与统一写入口/读 helper 一行调用，不含业务语义；行为权威在
//! [`crate::investment`]（阶段 5 域目录归位，#401 / ADR-0056）。注册路径与
//! 前端调用保持不变。
//!
//! 写命令经壳层统一写入口 [`crate::write_entry::write_entry`]（ADR-0073）：
//! 仪式（锁、事务、置脏、信号）内化单点，证据随闭包返回必达；读命令经
//! `run_db`（形状乙，spec #498 / #503）。
//! `add_fund_by_code` 的东财拉取（单请求叠加限流冷却重试最长可达分钟级）
//! 在闭包内、连接锁外先行完成，任何形状下不进锁（慢闭包纪律）。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::currencies::{ExchangeRate, ExchangeRateInput};
use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::investment as investment_domain;
use crate::investment::{
    AddFundResult, Holding, InstrumentInput, InstrumentListFilter, InstrumentListResult,
    InstrumentPriceTrend, ManualPriceInput, ManualPriceResult, MarketPrice, MarketPriceInput,
    PnlFilter, PortfolioValueTrend, RealizedPnlSummary, TransactionTrade, TrendRange,
};
use crate::signals::{WriteEvidence, WriteOp};
use crate::write_entry::{Outcome, write_entry};

#[tauri::command]
pub async fn list_holdings(db: State<'_, DbState>) -> Result<Vec<Holding>> {
    let conn = db.conn.clone();
    run_db("list_holdings", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        investment_domain::list_holdings(&conn)
    })
    .await
}

#[tauri::command]
pub async fn instrument_price_trend(
    db: State<'_, DbState>,
    instrument_id: String,
    filter: Option<TrendRange>,
) -> Result<InstrumentPriceTrend> {
    let conn = db.conn.clone();
    // 域入口单点（#401 域目录化）：BDD 步骤直调同一域函数，与 IPC 命令同一实现。
    run_db("instrument_price_trend", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        investment_domain::query_instrument_price_trend(
            &conn,
            &instrument_id,
            &filter.unwrap_or_default(),
        )
    })
    .await
}

#[tauri::command]
pub async fn portfolio_value_trend(
    db: State<'_, DbState>,
    filter: Option<TrendRange>,
) -> Result<PortfolioValueTrend> {
    let conn = db.conn.clone();
    // 域入口单点（#401 域目录化）：BDD 步骤直调同一域函数，与 IPC 命令同一实现。
    run_db("portfolio_value_trend", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        investment_domain::query_portfolio_value_trend(&conn, &filter.unwrap_or_default())
    })
    .await
}

#[tauri::command]
pub async fn realized_pnl_summary(
    db: State<'_, DbState>,
    filter: Option<PnlFilter>,
) -> Result<RealizedPnlSummary> {
    let conn = db.conn.clone();
    run_db("realized_pnl_summary", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let filter = filter.unwrap_or(PnlFilter {
            account_id: None,
            instrument_id: None,
        });
        investment_domain::query_realized_pnl_summary(&conn, &filter)
    })
    .await
}

#[tauri::command]
pub async fn list_exchange_rates(db: State<'_, DbState>) -> Result<Vec<ExchangeRate>> {
    let conn = db.conn.clone();
    run_db("list_exchange_rates", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        investment_domain::list_exchange_rates(&conn)
    })
    .await
}

#[tauri::command]
pub async fn create_exchange_rate(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: ExchangeRateInput,
) -> Result<String> {
    let conn = db.conn.clone();
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
    let conn = db.conn.clone();
    run_db("list_market_prices", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        investment_domain::list_market_prices(&conn)
    })
    .await
}

#[tauri::command]
pub async fn create_market_price(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: MarketPriceInput,
) -> Result<String> {
    let conn = db.conn.clone();
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
    let conn = db.conn.clone();
    run_db("list_instruments", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let filter = filter.unwrap_or_default();
        investment_domain::list_instruments(&conn, &filter)
    })
    .await
}

#[tauri::command]
pub async fn delete_instrument(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<()> {
    // 删除只动标的字典（及级联的价格行），不发失效信号——无流水引用的标的无
    // 持仓/走势消费方，前端标的列表本地重拉（issue #292 验收项）；零信号身份
    // 仍经写入口流动，未来补信号时天然生效（ADR-0073 决策 3）。
    let conn = db.conn.clone();
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
pub async fn get_transaction_trade(db: State<'_, DbState>, id: String) -> Result<TransactionTrade> {
    let conn = db.conn.clone();
    run_db("get_transaction_trade", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        investment_domain::get_transaction_trade(&conn, &id)
    })
    .await
}

/// IPC 命令：按 6 位基金代码即拉添加场外基金（issue #301 / ADR-0038）。东财
/// 拉取（名称/分类/最新净值）在连接锁外完成（单请求叠加限流冷却重试最长可达
/// 分钟级，任何形状下不进锁）；落库与信号经统一写入口（ADR-0073），编排经
/// `investment::add_fund_by_code_with` 同一接缝（拉取已在锁外完成，注入闭包
/// 直接回放结果，与测试/BDD 同一套校验→拉取→落库实现）。落现价即广播价格
/// 失效信号（ADR-0031），未取到净值仅建标的零信号（零变化不广播）；「是否发」
/// 判定单点在 signals 映射（ADR-0044 / issue #333），入口只传递证据。
#[tauri::command]
pub async fn add_fund_by_code(
    db: tauri::State<'_, DbState>,
    app: tauri::AppHandle,
    code: String,
) -> Result<AddFundResult> {
    // 格式非法即刻拒绝，不发起网络请求。
    investment_domain::validate_fund_code(&code)?;
    let conn = db.conn.clone();
    write_entry(
        "add_fund_by_code",
        conn,
        Some(&app),
        WriteOp::AddFundByCode,
        move |conn| {
            // 网络拉取在锁外：单请求叠加限流冷却重试最长可达分钟级，不阻塞其它命令。
            let detail = crate::sync::fetch_fund_detail_production(&code)?;
            // 编排单点：经接缝以已拉取的详情驱动（注入闭包同值回放）。
            let mut fetch = |_: &str| Ok(detail.clone());
            investment_domain::add_fund_by_code_with(conn, &code, &mut fetch).map(|result| {
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
    // 手动创建入口守卫（类型白名单 + 名称必填，ADR-0036 决策 3）在先，写路径
    // 经统一写入口（ADR-0073）：成功即置脏（含同名标的信息更新的 upsert 分支）。
    let conn = db.conn.clone();
    write_entry(
        "create_instrument",
        conn,
        Some(&app),
        WriteOp::CreateInstrument,
        move |conn| investment_domain::create_instrument_manual(conn, input).map(Outcome::Silent),
    )
    .await
}

/// IPC 命令：手动报价（issue #291 / ADR-0036）。「日期 + 价格」单点录入，
/// 一条通道两个落点——现价缓存 upsert + 价格历史周采样幂等覆盖；回填早于
/// 最新价格点的旧价只沉淀历史、不动现价（最新点映像规则）。写路径与信号经
/// 统一写入口（ADR-0073）：成功即置脏。实际写入任一落点即广播价格失效信号
/// （生产者清单再添一处，ADR-0031 模式；「是否发」判定单点在 signals 映射，
/// ADR-0044 / issue #333），下游刷新由既有信号消费方完成，零变化不广播。录价
/// UI 入口只对同步覆盖不到的标的开放——判定收在 UI 侧，后端
/// 命令不设守卫（ADR-0036 决策 1 修订）。
#[tauri::command]
pub async fn record_manual_price(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: ManualPriceInput,
) -> Result<ManualPriceResult> {
    let conn = db.conn.clone();
    write_entry(
        "record_manual_price",
        conn,
        Some(&app),
        WriteOp::RecordManualPrice,
        move |conn| {
            investment_domain::record_manual_price(conn, &input).map(|outcome| {
                // 实际写入任一落点（`any_written` 归一化）即广播，零变化不广播。
                let evidence = WriteEvidence::PriceWritten(outcome.any_written());
                Outcome::Evidenced(outcome, evidence)
            })
        },
    )
    .await
}
