//! IPC 命令壳 · 投资（Instrument / Holding / Trade / Trend，#401 域目录化
//! ADR-0056）：标的字典、市场数据录入、基金接入、手动报价、持仓 / 走势 / 盈亏
//! 与买卖明细查询命令。
//!
//! 只做参数解包与统一写入口/读 helper 一行调用，不含业务语义；行为权威在
//! [`crate::investment`]（阶段 5 域目录归位，#401 / ADR-0056）。注册路径与
//! 前端调用保持不变。
//!
//! 写命令经壳层统一写入口 [`crate::shell_support::write_entry::write_entry`]（ADR-0073）：
//! 仪式（锁、事务、置脏、信号）内化单点，证据随闭包返回必达；读命令经
//! `run_db`（形状乙，spec #498 / #503）。
//! `add_fund_by_code` 的东财拉取（单请求叠加限流冷却重试最长可达分钟级）在
//! 命令体直接 `await`（async 生产入口，ADR-0125 决策 7 / issue #1413），连接锁外
//! 先行完成，任何形状下不进锁（慢闭包纪律）；`spawn_blocking` 包装已删。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
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
    AddFundResult, AddStockInstrumentResult, CurrencyCumulativePnl, Holding, Instrument,
    InstrumentInput, InstrumentListFilter, InstrumentListResult, InstrumentPriceTrend,
    ManualPriceInput, ManualPriceResult, MarketPrice, MarketPriceInput, MoneyWeightedReturnSummary,
    MwrRange, PnlFilter, PortfolioValueTrend, PriceStaleness, RealizedPnlSummary,
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

/// IPC 命令：价格过期检查（issue #1190）——打开投资页时的本地水位检查
/// （零网络请求）：有通道标的的现价水位超出阈值、或持仓标的缺现价时给出计数，
/// 供界面提示「价格可能已过期」并导向既有「同步标的信息」入口。只读本地库，
/// 不触发任何同步（ADR-0015 / ADR-0095 的显式触发口径不变）。
#[tauri::command]
pub async fn instrument_price_staleness(db: State<'_, DbState>) -> Result<PriceStaleness> {
    let conn = db.read_handle();
    read_entry("instrument_price_staleness", conn, move |conn| {
        investment_domain::instrument_price_staleness(conn)
    })
    .await
}

#[tauri::command]
pub async fn instrument_price_trend(
    db: State<'_, DbState>,
    instrument_id: String,
    filter: Option<TrendRange>,
) -> Result<InstrumentPriceTrend> {
    let conn = db.read_handle();
    // 域入口单点（#401 域目录化）：BDD 步骤直调同一域函数，与 IPC 命令同一实现。
    read_entry("instrument_price_trend", conn, move |conn| {
        investment_domain::query_instrument_price_trend(
            conn,
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
    let conn = db.read_handle();
    // 域入口单点（#401 域目录化）：BDD 步骤直调同一域函数，与 IPC 命令同一实现。
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

/// IPC 命令：按币种分组的累计收益（issue #1077）——未实现盈亏 + 已实现盈亏两腿
/// 相加，覆盖持仓页签合计区与首页投资卡；只读聚合，无写入路径。
#[tauri::command]
pub async fn cumulative_pnl_summary(db: State<'_, DbState>) -> Result<Vec<CurrencyCumulativePnl>> {
    let conn = db.read_handle();
    read_entry("cumulative_pnl_summary", conn, move |conn| {
        investment_domain::query_cumulative_pnl_summary(conn)
    })
    .await
}

/// IPC 命令：资金加权收益率（ADR-0115 / issue #1195）——三个消费面（持仓页单
/// 标的 / 盈亏页账户级与全账级）共用的只读投影，可选区间（区间开始存量持仓按
/// 区间首日市值折为期初投入，收益率的输入假设、不改账务）。无写入路径。
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

/// IPC 命令：按 id 精确取标的（issue #709）——走势页签 focus 消费的只读解析
/// 路径（现有标的列表过滤仅支持搜索词/市场/类型/持仓，无按 id 路径）；完整
/// 标的对象与列表行同投影，清仓/无持仓标的照常返回（走势不依赖持仓）。
#[tauri::command]
pub async fn get_instrument(db: State<'_, DbState>, id: String) -> Result<Instrument> {
    let conn = db.read_handle();
    read_entry("get_instrument", conn, move |conn| {
        investment_domain::get_instrument(conn, &id)
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
pub async fn get_transaction_trade(db: State<'_, DbState>, id: String) -> Result<TransactionTrade> {
    let conn = db.read_handle();
    read_entry("get_transaction_trade", conn, move |conn| {
        investment_domain::get_transaction_trade(conn, &id)
    })
    .await
}

/// IPC 命令：取一笔基金转换的两腿明细（ADR-0099 / issue #979）——转换表单
/// 编辑模式回填「A → B」全量信息的数据源（扩展表投影，非转换交易 NotFound）。
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

/// IPC 命令：取一笔份额调整的明细（ADR-0106 / issue #1052）——交易列表
/// 「只读详情」呈现标的与**带符号**份额增量 Δ 的数据源（扩展表投影，
/// 非 split 交易 NotFound）。
#[tauri::command]
pub async fn get_transaction_split(db: State<'_, DbState>, id: String) -> Result<TransactionSplit> {
    let conn = db.read_handle();
    read_entry("get_transaction_split", conn, move |conn| {
        investment_domain::get_transaction_split(conn, &id)
    })
    .await
}

/// IPC 命令：按 6 位基金代码即拉添加场外基金（issue #301 / ADR-0038）。
/// 格式校验即刻拒绝（不发网络请求）→ 东财拉取（名称/分类/最新净值）在命令体
/// 直接 `await`（连接锁外，单请求叠加限流冷却重试最长可达分钟级，任何形状下
/// 不进锁，慢闭包纪律；async 形态 ADR-0125 决策 7 / issue #1413：生产入口已
/// async 化，`spawn_blocking` 包装与 JoinError 归一化删除）→ 落库与信号经统一
/// 写入口（ADR-0073），编排经 `investment::add_fund_by_code_with` 同一接缝
///（拉取已在锁外完成，注入闭包同步回放结果，与测试/BDD 同一套校验→拉取→落库
/// 实现；接缝闭包不承载网络等待，同步形状即「连接不跨网络等待」的结构保证）。
/// 落现价即广播价格失效信号（ADR-0031），未取到净值仅建标的零信号（零变化不
/// 广播）；「是否发」判定单点在 signals 映射（ADR-0044 / issue #333），入口只
/// 传递证据。
#[tauri::command]
pub async fn add_fund_by_code(
    db: tauri::State<'_, DbState>,
    app: tauri::AppHandle,
    code: String,
) -> Result<AddFundResult> {
    // 格式非法即刻拒绝，不发起网络请求。
    investment_domain::validate_fund_code(&code)?;
    let conn = db.write_handle();
    // 网络拉取在锁外：单请求叠加限流冷却重试最长可达分钟级，不阻塞其它命令
    // （慢闭包纪律）；async 生产入口直接 await，无阻塞包装。
    let quote = ledger_market_sync::fetch_fund_quote_production(&code).await?;
    // 落库阶段经统一写入口：拉取已完成，闭包纯落库（编排单点：经接缝以已拉取
    // 的报价驱动，注入闭包同步回放；统一注入签名为（代码，市场），场外基金
    // 无交易所市场，市场位不消费）。
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

/// IPC 命令：按代码添加投资标的·场内通道（issue #697 / spec #690 / ADR-0081）。
/// 市场必选的录入通道（沪 sh/深 sz/港 hk/美股 us；场外基金通道走
/// `add_fund_by_code`，不在本命令）→ 查询阶段在连接锁外完成（通道解析 → 候选
/// 遍历 → 东财行情，单请求叠加限流冷却重试最长可达分钟级，任何形状下不进锁，
/// 慢闭包纪律）→ 识别落库经统一写入口（ADR-0073）：类型自动识别（行情命中 →
/// stock、类型特征 → etf，识别单点在投资域）后经创建增强同一落库接缝回填权威
/// 名称与最新价。落现价即广播价格失效信号（ADR-0031），停牌未取到价仅建标的
/// 零信号；查询未命中与临时不可达均显式报错不建档（兑底手动建档由前端对话框
/// 内 `create_instrument` 承接，不在本命令）。
#[tauri::command]
pub async fn add_instrument_by_code(
    db: tauri::State<'_, DbState>,
    app: tauri::AppHandle,
    market: String,
    code: String,
) -> Result<AddStockInstrumentResult> {
    let conn = db.write_handle();
    // 查询阶段在锁外：网络往返不进锁（慢闭包纪律）；生产拉取闭包直接接 async
    // 生产入口（与同步域同一 HTTP 层：主机池/重试/限流），查询编排（通道解析 →
    // 候选遍历）在本命令体 await（ADR-0125 决策 7 / issue #1413：注入闭包返回
    // future，`spawn_blocking` 包装与 JoinError 归一化删除），未命中/临时错误以
    // 码化错误上抛给对话框分流。
    let mut fetch = |code: &str, market: &str| {
        // 统一注入签名（ADR-0103）：（代码，市场）——闭包同步段拷贝入参为自有
        // 数据，future 无借用（与通道束闭包同款约定）。
        let code = code.to_string();
        let market = market.to_string();
        async move { ledger_market_sync::fetch_stock_quote_production(&market, &code).await }
    };
    let quote = investment_domain::fetch_stock_quote_for_add(&market, &code, &mut fetch).await?;
    // 识别落库阶段经统一写入口：类型 = 行情 kind_hint（识别语义在投资域单点），
    // 证据随闭包返回必达（价格失效信号广播判定）。
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
    // 手动创建入口守卫（类型白名单 + 名称必填，ADR-0036 决策 3）在先，写路径
    // 经统一写入口（ADR-0073）：成功即置脏（含同名标的信息更新的 upsert 分支）。
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
    let conn = db.write_handle();
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

#[cfg(test)]
mod tests {
    use crate::test_support::scan::matching_brace_end;

    /// 命令体提取（掩码文本）：签名锚点后首个 `{` 起经
    /// [`matching_brace_end`] 配对到命令体结束（#1433 上收，手写同型的单一实现）。
    fn command_body<'a>(masked: &'a str, signature: &str) -> &'a str {
        let anchor = masked
            .find(signature)
            .unwrap_or_else(|| panic!("命令 {signature} 应在位"));
        let open = anchor + masked[anchor..].find('{').expect("命令体应有大括号");
        let end = matching_brace_end(masked, open).expect("命令体花括号应配对");
        &masked[open..end]
    }

    /// `add_fund_by_code` 的东财拉取必须发生在连接锁外（慢闭包纪律，ADR-0069
    /// 决策 4 / issue #1282）：生产拉取入口 [`ledger_market_sync::fetch_fund_quote_production`]
    /// 不经数据库连接，命令体把拉取放在 `write_entry` 之前即结构上不可能持锁；
    /// 拉取被移回统一写入口闭包（锁内）时本守门即红。IPC 命令路径无报价注入
    /// 接缝（生产入口直呼 `fetch_fund_quote_production`，行为测试无从注入慢
    /// 拉取观察锁竞争），与先例 #959/#961 同口径以源码扫描守门（系统化持锁
    /// 守门衔接 #1276）；词法器具单点住 `test_support::scan`（#1433）。
    #[test]
    fn fund_fetch_happens_before_write_entry_in_add_fund_by_code() {
        let text = crate::test_support::scan::mask_non_code(include_str!("investment.rs"));
        let body = command_body(&text, "pub async fn add_fund_by_code(");
        let write_at = body
            .find("write_entry(")
            .expect("落库应经统一写入口 write_entry");
        let fetch_count = body.matches("fetch_fund_quote_production(").count();
        assert_eq!(fetch_count, 1, "东财拉取入口在命令体内应恰出现一次");
        let fetch_at = body
            .find("fetch_fund_quote_production(")
            .expect("东财拉取入口应在命令体内");
        assert!(
            fetch_at < write_at,
            "东财拉取（单请求叠加限流冷却重试最长可达分钟级）必须在 write_entry \
             之前完成——移回统一写入口闭包即在连接锁内执行网络等待，阻塞全应用 \
             IPC/HTTP 读写（ADR-0069 决策 4 / issue #1282）"
        );
        // 阻塞包装禁令（ADR-0125 决策 7 / issue #1413）：拉取在异步命令体内直接
        // await，`spawn_blocking` 阻塞包装不得回归——回归即把网络等待挪回阻塞池
        // 线程，异步上下文里重新出现阻塞资源（#1403 同款纪律退化面）。同层的
        // JoinError 归一化错误消息（「任务执行失败」）断言已按断言强度删除
        // （issue #1443）：字面量在掩码文本上不可达、永真无变红路径，且该消息
        // 是阻塞包装 JoinError 归一化的伴生形态——本决策要拦的回归是「包装 +
        // 归一化」整体回归，`spawn_blocking` 令牌断言已覆盖，消息断言无独有
        // 守护责任。
        assert!(
            !body.contains("spawn_blocking"),
            "add_fund_by_code 不得回归 spawn_blocking 阻塞包装（ADR-0125 决策 7 / \
             issue #1413）：async 生产入口在命令体直接 await"
        );
    }

    /// `add_instrument_by_code` 的查询阶段同款守门（ADR-0125 决策 7 / issue #1413）：
    /// 生产拉取闭包直接接 async 生产入口（`fetch_stock_quote_production`），查询
    /// 编排（`fetch_stock_quote_for_add`）在命令体 await——阻塞包装（同步闭包 +
    /// `spawn_blocking`）回归即红，JoinError 归一化消息断言按断言强度删除
    /// （#1443）。行为分支触真实网络、测试面不可达，以源码扫描守门（先例
    /// #959/#961，ADR-0087）；词法器具单点住 `test_support::scan`（#1433）。
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
