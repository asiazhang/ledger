mod crud;
mod fund;
mod holdings;
mod manual_price;
pub(crate) mod predicates;
mod reports;
#[cfg(test)]
mod tests;
mod trade;
mod trend;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{
    AddFundResult, ExchangeRate, ExchangeRateInput, Holding, InstrumentInput, InstrumentListFilter,
    InstrumentListResult, InstrumentPriceTrend, InstrumentType, ManualPriceInput,
    ManualPriceResult, MarketPrice, MarketPriceInput, PnlFilter, PortfolioValueTrend,
    RealizedPnlSummary, TransactionTrade, TrendRange,
};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

pub(crate) use reports::query_realized_pnl_summary;
// 投资交易对外出口收窄为 prepare/apply/revert 三件套（issue #72 / spec #69）：
// 校验归一化（prepare）、应用副作用（apply）、回退副作用（revert）各一个入口，
// 不再暴露 create/update/cleanup/reverse 等散落函数；行写入经交易行为层编排。
pub(crate) use trade::{Plan, apply, prepare, revert};

/// 价格刻度换算因子（ADR-0038）：1 分 = 100 万分之一元——
/// 金额（分）= 数量 × 单价（万分之一元）÷ 本因子；手续费分摊薄入每份成本时
/// 乘本因子归到价格刻度。与 `v_holdings` 视图表达式（V002）同口径，视图 SQL
/// 无法引用 Rust 常量，两侧以本词条注释互认，改其一必同步另一。
pub(crate) const PRICE_UNITS_PER_FEN: f64 = 100.0;

#[tauri::command]
pub fn list_holdings(db: tauri::State<'_, DbState>) -> Result<Vec<Holding>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::list_holdings(&conn)
}

#[tauri::command]
pub fn instrument_price_trend(
    db: tauri::State<'_, DbState>,
    instrument_id: String,
    filter: Option<TrendRange>,
) -> Result<InstrumentPriceTrend> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    instrument_price_trend_internal(&conn, &instrument_id, &filter.unwrap_or_default())
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行单标的走势查询（与
/// [`portfolio_value_trend_internal`] 同一先例，供 BDD 步骤复用与 IPC 命令同一实现）。
pub fn instrument_price_trend_internal(
    conn: &rusqlite::Connection,
    instrument_id: &str,
    filter: &TrendRange,
) -> Result<InstrumentPriceTrend> {
    trend::query_instrument_price_trend(conn, instrument_id, filter)
}

#[tauri::command]
pub fn portfolio_value_trend(
    db: tauri::State<'_, DbState>,
    filter: Option<TrendRange>,
) -> Result<PortfolioValueTrend> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    portfolio_value_trend_internal(&conn, &filter.unwrap_or_default())
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行组合走势查询（先例：
/// [`list_instruments_internal`]，供 BDD 步骤复用与 IPC 命令同一实现）。
pub fn portfolio_value_trend_internal(
    conn: &rusqlite::Connection,
    filter: &TrendRange,
) -> Result<PortfolioValueTrend> {
    trend::query_portfolio_value_trend(conn, filter)
}

#[tauri::command]
pub fn realized_pnl_summary(
    db: tauri::State<'_, DbState>,
    filter: Option<PnlFilter>,
) -> Result<RealizedPnlSummary> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let filter = filter.unwrap_or(PnlFilter {
        account_id: None,
        instrument_id: None,
    });
    query_realized_pnl_summary(&conn, &filter)
}

#[tauri::command]
pub fn list_exchange_rates(db: tauri::State<'_, DbState>) -> Result<Vec<ExchangeRate>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::list_exchange_rates(&conn)
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行汇率写入（先例：
/// [`list_instruments_internal`]，供 BDD 步骤复用同一实现）。
pub fn create_exchange_rate_internal(
    conn: &rusqlite::Connection,
    input: ExchangeRateInput,
) -> Result<String> {
    crud::create_exchange_rate(conn, input)
}

#[tauri::command]
pub fn create_exchange_rate(
    db: tauri::State<'_, DbState>,
    input: ExchangeRateInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| create_exchange_rate_internal(conn, input))
}

#[tauri::command]
pub fn list_market_prices(db: tauri::State<'_, DbState>) -> Result<Vec<MarketPrice>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crud::list_market_prices(&conn)
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行行情写入（先例：
/// [`list_instruments_internal`]，供 BDD 步骤复用同一实现）。
pub fn create_market_price_internal(
    conn: &rusqlite::Connection,
    input: MarketPriceInput,
) -> Result<String> {
    crud::create_market_price(conn, input)
}

#[tauri::command]
pub fn create_market_price(
    db: tauri::State<'_, DbState>,
    input: MarketPriceInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| create_market_price_internal(conn, input))
}

#[tauri::command]
pub fn list_instruments(
    db: tauri::State<'_, DbState>,
    filter: Option<InstrumentListFilter>,
) -> Result<InstrumentListResult> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let filter = filter.unwrap_or_default();
    crud::list_instruments(&conn, &filter)
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行标的列表查询
/// （先例：`search::search_transactions_internal`，供 BDD 步骤复用同一实现）。
pub fn list_instruments_internal(
    conn: &rusqlite::Connection,
    filter: &InstrumentListFilter,
) -> Result<InstrumentListResult> {
    crud::list_instruments(conn, filter)
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行标的删除（先例：
/// [`list_instruments_internal`]，供 BDD 步骤复用同一实现；守卫在核心函数内，
/// 与 IPC 命令同一路径）。
pub fn delete_instrument_internal(conn: &rusqlite::Connection, id: &str) -> Result<()> {
    crud::delete_instrument(conn, id)
}

#[tauri::command]
pub fn delete_instrument(db: tauri::State<'_, DbState>, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏。删除只动标的字典（及级联的
    // 价格行），不发失效信号——无流水引用的标的无持仓/走势消费方，前端标的
    // 列表本地重拉（issue #292 验收项）。
    db.write(|conn| delete_instrument_internal(conn, &id))
}

#[tauri::command]
pub fn get_transaction_trade(
    db: tauri::State<'_, DbState>,
    id: String,
) -> Result<TransactionTrade> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    trade::get_transaction_trade(&conn, &id)
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行新建标的（先例：
/// [`list_instruments_internal`]，供 BDD 步骤复用同一实现）。
pub fn create_instrument_internal(
    conn: &rusqlite::Connection,
    input: InstrumentInput,
) -> Result<String> {
    crud::create_instrument(conn, input)
}

/// 手动创建入口守卫（IPC 命令入口层，ADR-0036 决策 3）：类型白名单收窄为
/// 债券/ETF/其他三类——股票字典归全量同步修、基金唯一创建入口归按代码即拉
/// （issue #301 / ADR-0038），白名单让手动字典与两条自动通道永不相交；名称必填
/// （自建标的主身份是名称）。守卫属 UI 侧政策，核心创建函数
/// [`create_instrument_internal`] 保持通用：AI HTTP 创建端点（ADR-0037）五类全开、
/// 名称可选，不经本守卫。同一接缝供 IPC 命令与 BDD 步骤复用。
pub fn create_instrument_manual_internal(
    conn: &rusqlite::Connection,
    input: InstrumentInput,
) -> Result<String> {
    match input.kind {
        InstrumentType::Bond | InstrumentType::Etf | InstrumentType::Other => {}
        InstrumentType::Stock => {
            return Err(AppError::coded(
                "instrument.stock-manual-forbidden",
                "股票类标的不支持手动创建：股票字典由「全量同步」从东方财富维护",
            ));
        }
        InstrumentType::Fund => {
            return Err(AppError::coded(
                "instrument.fund-manual-forbidden",
                "基金类标的不支持手动创建：请用「添加基金」输入 6 位代码自动回填",
            ));
        }
    }
    if input.name.as_deref().is_none_or(|n| n.trim().is_empty()) {
        return Err(AppError::coded(
            "instrument.name-required",
            "标的名称不能为空",
        ));
    }
    create_instrument_internal(conn, input)
}

/// 测试/e2e 入口：按代码即拉核心接缝（注入详情获取函数，离线驱动；生产接网络
/// 层的编排见 [`add_fund_by_code`] 命令，同一实现）。
pub use fund::add_fund_by_code_with;

/// AI 创建端点 fund 增强落库接缝（issue #304 / ADR-0039 决策 3，HTTP 层专用）：
/// 东财命中走 `persist_fund_detail`（与按代码即拉同一落库实现），网络不可达
/// 降级走 `create_fund_degraded`；校验与 6 位判定同出口。
pub(crate) use fund::{
    FundCreateOutcome, create_fund_degraded, is_six_digit_code, persist_fund_detail,
    validate_fund_code,
};

/// IPC 命令：按 6 位基金代码即拉添加场外基金（issue #301 / ADR-0038）。东财
/// 拉取（名称/分类/最新净值）在连接锁外的后台线程完成；落库走连接层统一写入口
/// （ADR-0032），编排经 [`add_fund_by_code_with`] 同一接缝（拉取已在线外完成，
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
    fund::validate_fund_code(&code)?;
    let conn = db.conn.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let span = tracing::info_span!("command", command = "add_fund_by_code");
        let _entered = span.enter();
        // 网络拉取在锁外：单请求叠加限流冷却重试最长可达分钟级，不阻塞其它命令。
        let detail = crate::commands::sync::fund::fetch_fund_detail_production(&code)?;
        // 编排单点：经接缝以已拉取的详情驱动（注入闭包同值回放）。
        let mut fetch = |_: &str| Ok(detail.clone());
        crate::db::write(&conn, |conn| {
            fund::add_fund_by_code_with(conn, &code, &mut fetch)
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
pub fn create_instrument(db: tauri::State<'_, DbState>, input: InstrumentInput) -> Result<String> {
    // 手动创建入口守卫（类型白名单 + 名称必填，ADR-0036 决策 3）在先，写路径
    // 经连接层统一写入口（ADR-0032）：成功即置脏（含同名标的信息更新的 upsert 分支）。
    db.write(|conn| create_instrument_manual_internal(conn, input))
}

/// 测试/e2e 入口：绕过 Tauri State 直接对连接执行手动报价（先例：
/// [`create_instrument_manual_internal`]，供 BDD 步骤复用同一实现）。
pub fn record_manual_price_internal(
    conn: &rusqlite::Connection,
    input: &ManualPriceInput,
) -> Result<ManualPriceResult> {
    manual_price::record_manual_price(conn, input)
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
    db: tauri::State<'_, DbState>,
    app: tauri::AppHandle,
    input: ManualPriceInput,
) -> Result<ManualPriceResult> {
    let outcome = db.write(|conn| manual_price::record_manual_price(conn, &input))?;
    // 实际写入任一落点（`any_written` 归一化）即广播，零变化不广播；
    // 「是否发」单点在 signals 映射（ADR-0044，#333），壳层只归一化证据并转发。
    emit_for(
        &app,
        WriteOp::RecordManualPrice,
        WriteEvidence::PriceWritten(outcome.any_written()),
    );
    Ok(outcome)
}
