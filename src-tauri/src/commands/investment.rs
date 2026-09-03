//! IPC 命令壳 · 投资（Instrument / Holding / Trade / Trend，#401 域目录化
//! ADR-0056）：标的字典、市场数据录入、基金接入、手动报价、持仓 / 走势 / 盈亏
//! 与买卖明细查询命令。
//!
//! 只做参数解包、事务边界与失效信号发射，不含业务语义；行为权威在
//! [`crate::investment`]（阶段 5 域目录归位，#401 / ADR-0056）。注册路径与
//! 前端调用保持不变。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：写路径对备份域
//! 零感知，置脏/到期检查由写入口闭包在提交点单点执行。「是否发」失效信号的
//! 判定单点在 signals 映射（ADR-0044 / issue #333），壳层只归一化证据并转发。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::currencies::{ExchangeRate, ExchangeRateInput};
use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::investment as investment_domain;
use crate::investment::{
    AddFundResult, Holding, InstrumentInput, InstrumentListFilter, InstrumentListResult,
    InstrumentPriceTrend, ManualPriceInput, ManualPriceResult, MarketPrice, MarketPriceInput,
    PnlFilter, PortfolioValueTrend, RealizedPnlSummary, TransactionTrade, TrendRange,
};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

#[tauri::command]
pub fn list_holdings(db: State<'_, DbState>) -> Result<Vec<Holding>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    investment_domain::list_holdings(&conn)
}

#[tauri::command]
pub fn instrument_price_trend(
    db: State<'_, DbState>,
    instrument_id: String,
    filter: Option<TrendRange>,
) -> Result<InstrumentPriceTrend> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    // 域入口单点（#401 域目录化）：BDD 步骤直调同一域函数，与 IPC 命令同一实现。
    investment_domain::query_instrument_price_trend(
        &conn,
        &instrument_id,
        &filter.unwrap_or_default(),
    )
}

#[tauri::command]
pub fn portfolio_value_trend(
    db: State<'_, DbState>,
    filter: Option<TrendRange>,
) -> Result<PortfolioValueTrend> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    // 域入口单点（#401 域目录化）：BDD 步骤直调同一域函数，与 IPC 命令同一实现。
    investment_domain::query_portfolio_value_trend(&conn, &filter.unwrap_or_default())
}

#[tauri::command]
pub fn realized_pnl_summary(
    db: State<'_, DbState>,
    filter: Option<PnlFilter>,
) -> Result<RealizedPnlSummary> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let filter = filter.unwrap_or(PnlFilter {
        account_id: None,
        instrument_id: None,
    });
    investment_domain::query_realized_pnl_summary(&conn, &filter)
}

#[tauri::command]
pub fn list_exchange_rates(db: State<'_, DbState>) -> Result<Vec<ExchangeRate>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    investment_domain::list_exchange_rates(&conn)
}

#[tauri::command]
pub fn create_exchange_rate(db: State<'_, DbState>, input: ExchangeRateInput) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| investment_domain::create_exchange_rate(conn, input))
}

#[tauri::command]
pub fn list_market_prices(db: State<'_, DbState>) -> Result<Vec<MarketPrice>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    investment_domain::list_market_prices(&conn)
}

#[tauri::command]
pub fn create_market_price(db: State<'_, DbState>, input: MarketPriceInput) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| investment_domain::create_market_price(conn, input))
}

#[tauri::command]
pub fn list_instruments(
    db: State<'_, DbState>,
    filter: Option<InstrumentListFilter>,
) -> Result<InstrumentListResult> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let filter = filter.unwrap_or_default();
    investment_domain::list_instruments(&conn, &filter)
}

#[tauri::command]
pub fn delete_instrument(db: State<'_, DbState>, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏。删除只动标的字典（及级联的
    // 价格行），不发失效信号——无流水引用的标的无持仓/走势消费方，前端标的
    // 列表本地重拉（issue #292 验收项）。
    db.write(|conn| investment_domain::delete_instrument(conn, &id))
}

#[tauri::command]
pub fn get_transaction_trade(db: State<'_, DbState>, id: String) -> Result<TransactionTrade> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    investment_domain::get_transaction_trade(&conn, &id)
}

/// IPC 命令：按 6 位基金代码即拉添加场外基金（issue #301 / ADR-0038）。东财
/// 拉取（名称/分类/最新净值）在连接锁外的后台线程完成；落库走连接层统一写入口
/// （ADR-0032），编排经 `investment::add_fund_by_code_with` 同一接缝（拉取已在线外完成，
/// 注入闭包直接回放结果，与测试/BDD 同一套校验→拉取→落库实现）。落现价即广播
/// 价格失效信号（ADR-0031，与两同步命令同一信号），未取到净值仅建标的零信号
///（零变化不广播）；「是否发」判定单点在 signals 映射（ADR-0044 / issue #333），
/// 壳层只归一化证据并转发。
#[tauri::command]
pub async fn add_fund_by_code(
    db: tauri::State<'_, DbState>,
    app: tauri::AppHandle,
    code: String,
) -> Result<AddFundResult> {
    // 格式非法即刻拒绝，不发起网络请求。
    investment_domain::validate_fund_code(&code)?;
    let conn = db.conn.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let span = tracing::info_span!("command", command = "add_fund_by_code");
        let _entered = span.enter();
        // 网络拉取在锁外：单请求叠加限流冷却重试最长可达分钟级，不阻塞其它命令。
        let detail = crate::sync::fetch_fund_detail_production(&code)?;
        // 编排单点：经接缝以已拉取的详情驱动（注入闭包同值回放）。
        let mut fetch = |_: &str| Ok(detail.clone());
        crate::db::write(&conn, |conn| {
            investment_domain::add_fund_by_code_with(conn, &code, &mut fetch)
        })
    })
    .await
    .map_err(|e| AppError::Io(format!("添加基金任务执行失败: {e}")))??;
    // 落现价即广播价格失效信号（ADR-0031），未取到净值零信号；
    // 「是否发」单点在 signals 映射（ADR-0044，#333），壳层只归一化证据并转发。
    emit_for(
        &app,
        WriteOp::AddFundByCode,
        WriteEvidence::PriceWritten(result.price_written),
    );
    Ok(result)
}

#[tauri::command]
pub fn create_instrument(db: State<'_, DbState>, input: InstrumentInput) -> Result<String> {
    // 手动创建入口守卫（类型白名单 + 名称必填，ADR-0036 决策 3）在先，写路径
    // 经连接层统一写入口（ADR-0032）：成功即置脏（含同名标的信息更新的 upsert 分支）。
    db.write(|conn| investment_domain::create_instrument_manual(conn, input))
}

/// IPC 命令：手动报价（issue #291 / ADR-0036）。「日期 + 价格」单点录入，
/// 一条通道两个落点——现价缓存 upsert + 价格历史周采样幂等覆盖；回填早于
/// 最新价格点的旧价只沉淀历史、不动现价（最新点映像规则）。写路径经连接层
/// 统一写入口（ADR-0032）：成功即置脏。实际写入任一落点即广播价格失效信号
/// （生产者清单再添一处，ADR-0031 模式；「是否发」判定单点在 signals 映射，
/// ADR-0044 / issue #333），下游刷新由既有信号消费方完成，零变化不广播。录价
/// UI 入口只对同步覆盖不到的标的开放——判定收在 UI 侧，后端
/// 命令不设守卫（ADR-0036 决策 1 修订）。
#[tauri::command]
pub fn record_manual_price(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: ManualPriceInput,
) -> Result<ManualPriceResult> {
    let outcome = db.write(|conn| investment_domain::record_manual_price(conn, &input))?;
    // 实际写入任一落点（`any_written` 归一化）即广播，零变化不广播；
    // 「是否发」单点在 signals 映射（ADR-0044，#333），壳层只归一化证据并转发。
    emit_for(
        &app,
        WriteOp::RecordManualPrice,
        WriteEvidence::PriceWritten(outcome.any_written()),
    );
    Ok(outcome)
}
