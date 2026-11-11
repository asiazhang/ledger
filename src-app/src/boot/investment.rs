//! 投资域读/写接缝（#2045 / #2046 / #2047）：投资概览/盈亏页签与走势/
//! 收益率取数、标的库与录价、买卖记账、明细页、按码添加基金/标的、
//! 信息同步——直调 `ledger-investment` 域入口与基础设施行情服务，
//! 不经 IPC。写成功后由视图广播投资失效信号（概览/持仓/标的重读）。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;
use crate::shell_support::write_entry_locked;

/// 当前持仓行集（投资域读路径直调，旧壳 `list_holdings` 命令同一执行协议）：
/// `v_holdings` 行 + 账户币逐行当期汇率软折算（`native_*_cents` 行级空值
/// 语义——缺现价或缺折算汇率行不计入合计，呈现面显式可见）。投资概览卡与
/// 财务自由度分子（持仓市值腿）同源不漂移。读连接是成对建连的只读侧。
pub fn portfolio_rows(db: &DbState) -> Result<Vec<ledger_investment::Holding>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::list_holdings(&conn)
}

/// 累计收益·折本位币单值（投资域读路径直调，旧壳 `cumulative_pnl_native_total`
/// 命令同一执行协议）：三腿逐行当期汇率折全局默认币种后求和（缺汇率码化
/// 上抛，域内同事务收口）。读连接是成对建连的只读侧（ADR-0117）。
pub fn cumulative_pnl_native_total(
    db: &DbState,
) -> Result<ledger_investment::CumulativePnlNativeTotal> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_cumulative_pnl_native_total(&conn)
}

/// 财务自由度总览（投资域读路径直调，旧壳 `financial_freedom` 命令同一执行
/// 协议）：分子（可投资资产两腿折算）与分母（预算域年化）跨表多语句，域内
/// `ensure_transaction` 同一读事务（分子分母恒同时点，tx_scope 快照纪律）。
/// 读连接是成对建连的只读侧（ADR-0117）。
pub fn financial_freedom(db: &DbState) -> Result<ledger_investment::FinancialFreedomOverview> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_financial_freedom(&conn)
}

/// 投资概览读数（投资页「概览」页签读接缝，旧壳 `investment_overview` 命令
/// 消费的同一域函数，不经 IPC；ADR-0131 全页折本位币单值，装配层只消费不另
/// 立口径）：可投资资产两腿 + 投资合计三项 + 缺价计数与引导判定多段取数，
/// 域内 `ensure_transaction` 同一读事务（#1699 同快照纪律）。读连接是成对
/// 建连的只读侧（ADR-0117）。
pub fn investment_overview(db: &DbState) -> Result<ledger_investment::InvestmentOverview> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_investment_overview(&conn)
}

/// 已实现盈亏汇总（投资页「盈亏」页签读接缝，旧壳 `realized_pnl_summary` 命
/// 令消费的同一域函数，不经 IPC；账户/标的筛选与两腿折算、`fx.rate-missing`
/// 码化上抛全在域内）：按年与按账户两张单值表多语句 + 汇率装载，域内
/// `ensure_transaction` 同一读事务（#1699 同快照纪律）。读连接是成对建连的
/// 只读侧（ADR-0117）。
pub fn realized_pnl_summary(
    db: &DbState,
    filter: &ledger_investment::PnlFilter,
) -> Result<ledger_investment::RealizedPnlSummary> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_realized_pnl_summary(&conn, filter)
}

/// 组合市值走势（走势卡读接缝，旧壳 `portfolio_value_trend` 命令消费的同一
/// 域函数，不经 IPC；区间裁剪与周采样全在域内，逐周数量×周线价×同期汇率
/// 多语句读闭包在域内 `ensure_transaction` 收同一读事务）。读连接是成对建
/// 连的只读侧（ADR-0117）。
pub fn portfolio_value_trend(
    db: &DbState,
    range: &ledger_investment::TrendRange,
) -> Result<ledger_investment::PortfolioValueTrend> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_portfolio_value_trend(&conn, range)
}

/// 资金加权收益率汇总（盈亏页收益率卡读接缝，旧壳
/// `money_weighted_return_summary` 命令消费的同一域函数，不经 IPC；现金流集
/// 与边界市值多语句读闭包在域内 `ensure_transaction` 收同一读事务）。旧壳
/// 调用不传区间（缺省 = 自首笔流水起算、截至今日现值），本壳同口。读连接
/// 是成对建连的只读侧（ADR-0117）。
pub fn money_weighted_return_summary(
    db: &DbState,
) -> Result<ledger_investment::MoneyWeightedReturnSummary> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::query_money_weighted_return_summary(
        &conn,
        &ledger_investment::MwrRange::default(),
    )
}

/// 标的全集（盈亏页标的筛选候选面）：旧壳盈亏筛选是远程搜索
/// `list_instruments`（每输入一发命令），本壳一次分页循环装全集后经统一模糊
/// 搜索语义本地过滤（[`crate::pinyin_select::filter_options`] 消费的同一域
/// 函数 `list_instruments`，search 语义同源）——筛选语义平价、无远程竞态；
/// 全集装载上限随 `page_size` 最大化并以 `total` 收核（半途即止会静默截断
/// 候选面，不做）。读连接是成对建连的只读侧（ADR-0117）。
pub fn instrument_rows(db: &DbState) -> Result<Vec<ledger_investment::Instrument>> {
    let conn = conn_lock(&db.read_conn)?;
    let page_size = ledger_investment::list_instruments(&conn, &Default::default())?
        .total
        .clamp(1, 500) as usize;
    let mut all = Vec::new();
    let mut page = 1usize;
    loop {
        let result = ledger_investment::list_instruments(
            &conn,
            &ledger_investment::InstrumentListFilter {
                page: Some(page),
                page_size: Some(page_size),
                ..Default::default()
            },
        )?;
        let fetched = result.items.len();
        all.extend(result.items);
        if all.len() >= result.total.max(0) as usize || fetched == 0 {
            break;
        }
        page += 1;
    }
    Ok(all)
}

/// 价格过期检查（#2046 持仓页签同步接缝的背景提示面；旧壳
/// `instrument_price_staleness` 命令消费的同一域函数，不经 IPC）：本地
/// 水位检查口径单点在域（计数与阈值都归域），本壳只搬运计数（旧壳
/// usePriceStaleness 同纪律）。读连接是成对建连的只读侧（ADR-0117）。
pub fn instrument_price_staleness(db: &DbState) -> Result<ledger_investment::PriceStaleness> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::instrument_price_staleness(&conn)
}

/// 读取一笔 buy/sell 交易的买卖明细（投资记账表单编辑回填，#2046；旧壳
/// `get_transaction_trade` 命令消费的同一域函数，不经 IPC）：标的/数量/价格/
/// 费用 + 标的字典带出字段（标的类型决定录入权威分流）。无明细（非 buy/sell）
/// 码化 NotFound。读连接是成对建连的只读侧（ADR-0117）。
pub fn transaction_trade(
    db: &DbState,
    transaction_id: &str,
) -> Result<ledger_investment::TransactionTrade> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::get_transaction_trade(&conn, transaction_id)
}

/// 投资明细列表页（投资页「明细」页签读接缝，旧壳 `list_investment_transactions`
/// 命令消费的同一域函数，不经 IPC）：五种投资 kind 行 + 四维过滤 + 服务端
/// offset 分页（items 与 total 同快照——读闭包收进域内读事务）。本页签页
/// 大小单点在壳（[`LEDGER_TAB_PAGE_SIZE`]），过滤维与页码由视图装配直传。
/// 读连接是成对建连的只读侧（ADR-0117）。
pub fn investment_transaction_page(
    db: &DbState,
    filter: &ledger_investment::InvestmentTransactionListFilter,
) -> Result<ledger_investment::InvestmentTransactionListResult> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::list_investment_transactions(&conn, filter)
}

/// 明细页签页大小（旧壳 LEDGER_TAB_PAGE_SIZE_DEFAULT 同值同源：固定 20）。
pub(crate) const LEDGER_TAB_PAGE_SIZE: usize = 20;

/// 标的列表页（投资页「标的」页签读接缝，旧壳 `list_instruments` 命令消费
/// 的同一域函数，不经 IPC）：搜索（统一模糊语义——锚点定位、COUNT 与 items
/// 读闭包在域内收同一读事务）+ 市场精确匹配 + 只看持仓 + anchor 分页定位。
/// 读连接是成对建连的只读侧（ADR-0117）。
pub fn instrument_page(
    db: &DbState,
    filter: &ledger_investment::InstrumentListFilter,
) -> Result<ledger_investment::InstrumentListResult> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_investment::list_instruments(&conn, filter)
}

/// 手动报价（投资页「标的」页签行内「录价」写入的域直调面；旧壳
/// `record_manual_price` 命令写路径的本壳等价半边，不经 IPC）：一条通道两个
/// 落点（现价缓存 upsert + 价格历史周采样幂等覆盖——回填旧价只沉淀历史，
/// 最新点映像规则在域）。写入经统一写入口直锁形态（issue #2087）：实际写入
/// 的提交点置脏由连接层单点结构性生效（整段形态，无需旧壳分段收尾的空写作
/// 业）；「是否广播」按写入见证（任一落点实际写入）由壳侧裁决（信号映射判
/// 定单点在 infra signals；本壳投影到投资族信号通道）。写连接是成对建连的写侧。
pub fn record_manual_price(
    db: &DbState,
    input: &ledger_investment::ManualPriceInput,
) -> Result<ledger_investment::ManualPriceResult> {
    write_entry_locked(db, |conn| {
        ledger_investment::record_manual_price(conn, input)
    })
}

/// 自建标的创建（旧壳 `create_instrument` 命令写路径的本壳等价半边，不经
/// IPC）：手动创建入口守卫（类型白名单 + 名称必填，ADR-0036 决策 3）在域
/// 写路径单正文，本地写编排（建档/复用改名 + op 追加）同收一事务。（代码，
/// 类型）幂等：同键复用并只更新名称。创建恒零价格证据（不落现价）——
/// 广播判定归调用方签名直接按零证据裁决（不广播，旧壳 Silent 写入口同判）。
pub fn create_instrument(
    db: &DbState,
    input: ledger_investment::InstrumentInput,
) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_investment::create_instrument_manual(conn, input)
    })
}

/// 自建标的删除（旧壳 `delete_instrument` 命令写路径的本壳等价半边，不经
/// IPC）：仅手动来源且无 buy/sell 流水引用（守卫在域写路径），同步来源与
/// 被引用标的拒删（级联：现价缓存/价格历史 CASCADE、批次被守卫覆盖）。
/// 删除是字典清理不是价格写入——零价格证据（不广播，旧壳 DeleteInstrument
/// 信号映射 NO_SIGNALS 同判）。
pub fn delete_instrument(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_investment::delete_instrument(conn, id))
}

/// 添加投资标的·场外基金通道（旧壳 `add_fund_by_code` 命令壳的本壳等价半
/// 边，不经 IPC）：格式校验即刻拒绝（不发网络请求）→ 行情拉取在连接锁外
/// await（单请求叠加限流冷却重试最长可达分钟级，任何形状下不进锁）→ 落库
/// 经域公开写入口（拉取已完成，注入闭包同步回放结果——与旧壳/BDD 同一套
/// 校验→拉取→落库实现）。落现价的证据随结果返回，广播判定归调用方签名。
pub async fn add_fund_by_code(
    db: &DbState,
    code: &str,
) -> Result<ledger_investment::AddFundResult> {
    ledger_investment::validate_fund_code(code)?;
    let quote = ledger_market_sync::fetch_fund_quote_production(code).await?;
    write_entry_locked(db, |conn| {
        let mut fetch = |_: &str, _: &str| Ok(quote.clone());
        ledger_investment::add_fund_by_code_with(conn, code, &mut fetch)
    })
}

/// 添加投资标的·场内通道（沪/深/港/美股；旧壳 `add_instrument_by_code` 命
/// 令壳的本壳等价半边，不经 IPC）：查询阶段在连接锁外 await（通道解析 →
/// 查询单元解析 → 腾讯行情）→ 识别落库经同一接缝（类型自动识别，行情命
/// 中 → stock、类型特征 → etf）。查询未命中与临时不可达以码化错误上抛。
pub async fn add_instrument_by_code(
    db: &DbState,
    market: &str,
    code: &str,
) -> Result<ledger_investment::AddStockInstrumentResult> {
    let mut fetch = |code: &str, route: ledger_investment::StockRoute| {
        let code = code.to_string();
        async move { ledger_market_sync::fetch_stock_quote_production(route, &code).await }
    };
    let quote = ledger_investment::fetch_stock_quote_for_add(market, code, &mut fetch).await?;
    write_entry_locked(db, |conn| {
        ledger_investment::add_stock_instrument_with_quote(conn, &quote)
    })
}

/// 持仓标的同步编排接缝（#2046；旧壳 `sync_instrument_info` IPC 命令壳的
/// 本壳等价形，消费面在 [`crate::investment_sync`] 同步接缝，不经 IPC）：
/// 门面写槽裸作业会话交给投资同步编排（旧壳命令消费的同一域编排入口
/// [`ledger_market_sync::do_incremental_sync_channels`]），进度回调由壳侧
/// 注入（消费端把进度报到接缝信号的呈现位）。
///
/// 收尾裁决（旧壳 write_entry_segmented_async 的本壳等价半边）：跨分段写入
/// 见证随结果必达（成败同判——失败 ≠ 未写过，中途失败已落库的写入仍生效，
/// issue #1277）；分段裸作业不经写入口，旧壳「实际写过即置脏」的空写作业
/// 收尾本壳不重演（分段面置脏接通属设计裁决，偏差留痕见 #2087 交付报告）
/// ——「是否广播」直接按见证由壳侧裁决（信号映射判定单点在 infra signals：
/// SyncInstrumentInfo × 实际写过 → PRICES_CHANGED，本壳投影到投资族信号通
/// 道）。进度不是失效信号（issue #897），只进呈现位。
pub async fn sync_instrument_info(
    db: &DbState,
    mut progress: impl FnMut(ledger_market_sync::SyncProgress) + Send,
) -> SyncInstrumentInfoOutcome {
    let session =
        ledger_market_sync::FacadeWriteSession::new(db.write_handle(), "sync_instrument_info");
    // 通道束（旧壳 #1276 换装口径）：生产默认生产通道束（每次同步构造，与旧
    // 壳整段形态同口径；NSH 无测试注入口——双壳并存期同步联调走旧壳）。
    match ledger_market_sync::SyncFetchChannels::production() {
        Ok(mut channels) => {
            let mut witness = ledger_market_sync::WriteWitness::default();
            let outcome = ledger_market_sync::do_incremental_sync_channels(
                &session,
                &mut channels,
                &mut progress,
                &mut witness,
            )
            .await;
            let evidence = witness.any_written();
            match outcome {
                Ok(result) => SyncInstrumentInfoOutcome::Success(result, evidence),
                Err(error) => SyncInstrumentInfoOutcome::Failed(error, evidence),
            }
        }
        Err(error) => SyncInstrumentInfoOutcome::Failed(error, false),
    }
}

/// 同步一次的终态归一（旧壳命令收尾裁决的投影面）：成功摘要 / 失败错误
/// 二选一 + 跨分段累积的写入证据（成败同判的最小幸存事实）。
#[derive(Debug)]
pub enum SyncInstrumentInfoOutcome {
    /// 编排成功终态：结果统计（同步 N 只 / 跳过 M 只 / 结果文案）+ 是否实际
    /// 写过（信号广播消费面）。
    Success(ledger_market_sync::SyncInstrumentInfoResult, bool),
    /// 编排失败终态：错误原样（消费方呈现失败文案——旧壳失败错误在发射后
    /// 原样上抛的同位）+ 是否实际写过（失败路径证据必达，issue #1277）。
    Failed(ledger_infra::error::AppError, bool),
}
