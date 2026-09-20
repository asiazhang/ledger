//! 基金**现价刷新**单元（issue #1388 自 `fund_nav` 拆出；ADR-0122 决策 2/3 /
//! issue #1377 现价与历史解耦）：只刷现价、不承担历史——首刷深回填与缺周点深补
//! 归价格历史后台补全（[`super::fund_backfill::backfill_one_fund_history`]）。
//! 取数面（[`super::bulk`]）命中时整只零请求；未命中退逐只短窗、页数封顶。
//!
//! 共享件（lsjz 报文解析、水位窗口、分页器、水位读）留守 `fund_nav`，本模块
//! 只收现价刷新的编排与取数面判定。

use chrono::NaiveDate;

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_investment::constant_price::{ensure_constant_base_price, mark_constant_unit_price};
use ledger_investment::prices::{
    EASTMONEY_PRICE_SOURCE, MarketPriceWrite, price_value_to_cents, upsert_market_price,
    upsert_price_history,
};

use super::bulk::BulkNavPoint;
use super::channels::FetchFuture;
use super::fund_nav::{
    MONEY_FUND_UNIT_NAV, NavPage, NavQuery, fetch_nav_pages, nav_window, read_fund_watermark,
};
use super::http::KlineBar;
use super::session::ScopedSession;

/// 现价刷新逐只回退的页数封顶（issue #1377）：现价刷新不是历史采集，逐只回退只
/// 服务「把现价刷到最新」与顺带落上近几个缺周点；封顶 2 页（≈ 8 周净值日）内
/// 窗口完整才落库，更深的缺口整只不落、交价格历史后台补全在下一窗口深采——
/// 水位不得推进到未落周点之前（否则缺周点永久漏采，见 [`refresh_one_fund_price`]）。
const REFRESH_MAX_NAV_PAGES: u64 = 2;

/// 现价刷新逐只回退的无历史序列短窗（issue #1377）：首刷的历史由后台补全整根
/// 回填，现价刷新只为这类标的拿「最新公布净值」——一个月窗口单页量级、常数请求。
const REFRESH_RECENT_WINDOW_MONTHS: chrono::Months = chrono::Months::new(1);

/// 基金分区的同步统计（与 [`super::incremental`] 的股票统计同源汇总）：
/// `synced` = 处理成功（含「已是最新、无新净值」）；`skipped` = 无法拉取
/// （首刷查无净值；**空响应/被拦截**——不是「无新净值」，issue #1059；名称充
/// 代码行的跳过计数由编排层派生，不入本结构）；`written` = 实际落库净值的只数
///（价格失效信号判定依据，零变化不广播）。
pub(super) struct FundSyncStats {
    pub(super) synced: usize,
    pub(super) skipped: usize,
    pub(super) written: usize,
}

/// 单只 fund 标的的现价刷新单元（ADR-0122 决策 2/3 / issue #1377）：只刷现价、
/// 不承担历史——首刷深回填与缺周点深补归价格历史后台补全
///（[`super::fund_backfill::backfill_one_fund_history`]）。取数面（[`super::bulk`]）命中时整只零请求：
/// 无新净值（批量面最新净值日期不晚于水位）即「已是最新」；新净值直落现价缓存，
/// 有历史序列者连当周采样点一并落库（[`land_bulk_point`]，周采样语义不变）。
/// 未命中（缺口 / 降级 / 停用期）退回逐只通道，窗口按「现价刷新」收窄——
/// 有历史序列者从水位次日增量但页数封顶（[`REFRESH_MAX_NAV_PAGES`]）：窗口
/// 完整才整只落库；更深缺口整只不落、交后台补全——**水位不得推进到未落周点
/// 之前**，否则缺周点在队列判定（水位判缺）里永久消失。无历史序列者取近一个月
/// 短窗（[`REFRESH_RECENT_WINDOW_MONTHS`]）的最新净值落现价，历史一行为零、
/// 不落采样点（历史归后台补全首刷）。
/// 单只结果累加进调用方的 `stats`；页级推进经注入的 `on_page` 回调透传
///（issue #1061）。基金间的遍历、名称随行刷新与标的级进度推进归编排层
///（issue #897）。跳过语义与 [`super::fund_backfill::backfill_one_fund_history`] 一致：查无净值与
/// 空响应/被拦截计入 `skipped`，不报错不中断；单只网络失败上抛中断同步。
///
/// **前置条件**：`fund` 为净值通道（FundNav）的 6 位真实代码基金行——名称充
/// 代码行（查不到净值）由调用方计入跳过、零请求（issue #897 起跳过判定与分母
/// 口径同收编排层）；恒定价格标的由编排层分区排除、不经本函数（ADR-0126
/// 决策 4 / #1451），货基的打标确认由下方数据源自报口径分支承担。
pub(super) async fn refresh_one_fund_price<Q, N, P>(
    session: &Q,
    fund: &super::incremental::SyncInstrument,
    latest_hint: Option<&BulkNavPoint>,
    fetch_nav: &mut N,
    stats: &mut FundSyncStats,
    on_page: &mut P,
) -> Result<()>
where
    // 作用域会话接缝（issue #1275）：本函数读写库的唯一通道，签名层面取不到连接。
    Q: ScopedSession,
    N: FnMut(&NavQuery) -> FetchFuture<NavPage> + Send,
    // 页级推进回调（issue #1061）：(已完成页, 总页数)。只在本页抓取返回之后发出
    //（抓取内部的退避/重试等待不产生推进）；单页（pages ≤ 1）不发——增量常态
    // 的事件形状与频率不变。
    P: FnMut(u64, u64) + Send,
{
    let today = super::incremental::beijing_today();
    let (watermark, has_history) = read_fund_watermark(session, fund).await?;
    // 取数面命中时整只零请求（ADR-0121 取数面把「要不要发逐只请求」的判断从
    // 「每标的一次请求」降为「整市场一次请求」）——逐只通道只在缺周点补齐与
    // 批量面未覆盖时接管（issue #1377：首刷不再接管，历史归后台补全）。
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
                    code = %fund.symbol, watermark = ?watermark, bulk_date = %hint.date,
                    "批量面已报出新净值但水位落后超过一个自然周，逐只短窗补齐缺失周点（封顶页数内）"
                );
            }
        }
    }
    // 逐只回退的窗口按「现价刷新」收窄（issue #1377）：有历史序列者从水位次日
    // 增量（封顶页数内窗口完整才落库）；无历史序列者取近一个月短窗拿最新净值。
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
    let collected = fetch_nav_pages(
        fetch_nav,
        &fund.symbol,
        &start,
        &end,
        REFRESH_MAX_NAV_PAGES,
        on_page,
    )
    .await?;

    // 打标确认（ADR-0126 决策 3；建档/日常刷新任一次确认即回填）：可信页自报
    // 货基口径即回填恒定单位价格标记（单向，幂等）。确认即收尾——不再落采集
    // 价格（现价覆盖与周点停落，读侧自此按常量取值，平坦序列不再生长；建档
    // 常量价行缺失时由兜底补一行）；响应缺信号不走此路，不清空既有标记。
    if collected.money_fund {
        let instrument_id = fund.instrument_id.clone();
        let currency = fund.currency.clone();
        let cents = price_value_to_cents(MONEY_FUND_UNIT_NAV);
        let priced_at = collected
            .points
            .iter()
            .map(|p| p.date.clone())
            .max()
            .unwrap_or_else(|| today.format("%Y-%m-%d").to_string());
        let written = session
            .with_connection(move |conn| {
                mark_constant_unit_price(conn, &instrument_id, cents)?;
                ensure_constant_base_price(
                    conn,
                    &instrument_id,
                    cents,
                    &currency,
                    &priced_at,
                    EASTMONEY_PRICE_SOURCE,
                )
            })
            .await?;
        if written {
            stats.written += 1;
        }
        stats.synced += 1;
        return Ok(());
    }

    if collected.points.is_empty() {
        if collected.blocked {
            // 空响应（Data 缺省 / 非对象）= 疑似被拦截或异常：结果不可信，不
            // 得计入成功（issue #1059）。与「窗口内确实无新净值」在统计上分
            // 开——此路计入跳过；日志带出原形态，便于与 T+1 正常空窗对照。
            tracing::warn!(
                code = %fund.symbol, has_history, watermark = ?watermark,
                "历史净值返回空响应（疑似被拦截/异常），本轮无法判定是否有新净值"
            );
            stats.skipped += 1;
        } else if has_history {
            // 增量窗口内确实无新净值（T+1 正常空窗）：现价已是最新，处理成功
            // 但不落库、不计跳过。
            stats.synced += 1;
        } else {
            // 查无净值（查无此码 / 新基金未公布首期）：无法拉取，计入跳过。
            stats.skipped += 1;
        }
        return Ok(());
    }
    if !collected.is_complete() {
        // 窗口不完整（深度缺周点超出封顶页数或页被拦截，issue #1377）：整只
        // 不落库并在日志标注——水位不得推进到未落周点之前（否则缺周点永久
        // 漏采），交价格历史后台补全在下一窗口深采；现价刷新不承担深补。
        tracing::warn!(
            code = %fund.symbol, has_history,
            blocked = collected.blocked, truncated = collected.truncated,
            "现价刷新短窗不完整（深度缺周点或被拦截），整只不落库、交后台补全深采"
        );
        stats.skipped += 1;
        return Ok(());
    }
    let points = collected.points;
    // 现价 = 窗口内最新公布单位净值（let-else 显式防线，#434 同款）：points
    // 非空由前文判空保证，此臂理论不可达；一旦前置防线被移除，此处记警告并
    // 跳过该只、不中断同步。
    let Some(latest) = points.iter().max_by_key(|p| p.date.as_str()) else {
        tracing::warn!(code = %fund.symbol, "净值点意外为空，跳过现价更新");
        return Ok(());
    };
    // 无新净值防线：短窗拿到的最新净值不新于水位即「已是最新」（无历史序列者
    // 的常态——按代码即拉已落过同一净值），零写入不虚增价格失效信号。
    if watermark
        .as_deref()
        .is_some_and(|w| latest.date.as_str() <= w)
    {
        stats.synced += 1;
        return Ok(());
    }
    // 当周采样点（仅有历史序列者）与现价落库同在一只一个事务里（ADR-0122
    // 决策 8 / issue #1373 同形体）：写失败整体回滚，不留半根。落库经作用域
    // 会话短暂取一次连接（issue #1275 / #1412 async 形态）；事务经
    // [`ensure_transaction`]（ADR-0033 嵌套感知）。
    let bars: Vec<KlineBar> = points
        .iter()
        .map(|p| KlineBar {
            date: p.date.clone(),
            close: p.nav,
        })
        .collect();
    let instrument_id = fund.instrument_id.clone();
    let currency = fund.currency.clone();
    let latest_date = latest.date.clone();
    let latest_nav = latest.nav;
    session
        .with_connection(move |conn| {
            ensure_transaction(conn, || {
                if has_history {
                    // 窗口内缺失的近期周点顺带落库（封顶页数保证了窗口完整；
                    // 深度缺周点已在上方交后台补全）。
                    super::incremental::write_weekly_price_history(
                        conn,
                        &instrument_id,
                        &currency,
                        &bars,
                        EASTMONEY_PRICE_SOURCE,
                    )?;
                }
                upsert_market_price(
                    conn,
                    &MarketPriceWrite {
                        instrument_id: &instrument_id,
                        price_cents: price_value_to_cents(latest_nav),
                        currency_code: &currency,
                        // 基金现价时点 = 净值日期（现价的行情日期就是净值本身对应的日期）；
                        // nav_date 兼任下次同步的水位。
                        priced_at: &latest_date,
                        nav_date: Some(&latest_date),
                        source: Some(EASTMONEY_PRICE_SOURCE),
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

/// 取数面命中时逐只净值通道的角色（ADR-0121 取数面 / ADR-0122 决策 2；现价
/// 刷新侧的判据，issue #1377 起首刷不再在现价刷新路径——首刷归后台补全）。
enum BulkDecision<'a> {
    /// 批量面证明没有新净值（最新净值日期不晚于水位）：现价已是最新，整只零请求。
    NothingNew,
    /// 批量面报出比水位更新的当周净值：它的最新单位净值就是当日现价与该周采样点
    /// （周采样要的正是「该周最后一个有报价交易日的价格」），直接落库、整只零请求。
    LandFromBulk(&'a BulkNavPoint),
    /// 逐只通道接管：水位落后批量面最新净值所在自然周超过一周（缺周点补齐——
    /// 批量面的单点落不了中间缺失的周点）或批量面未覆盖（缺口）。
    PerInstrument,
}

/// 取数面命中时的三分类判据（ADR-0122 决策 2「逐只采集只服务首刷与缺周点补齐」
/// 的现价刷新半边，issue #1377）：这是「这次刷新用几次请求」的判定单点——判定为
/// 前两类即零逐只请求，请求量因此不随基金数增长。
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

/// 缺周点补齐判据（ADR-0122 决策 2）：水位落后批量面最新净值所在自然周**超过
/// 一周**（例如应用数周未开）时，中间缺失的周点只有逐只窗口能补——逐只通道接管。
/// 同一周或只差一周（常态的跨周）不触发：批量面的最新净值就是当周采样点，上一周
/// 的采样点已在上一周落库。水位缺失或不可解析时保守接管（宁可多一次请求，
/// 不静默丢周点）。
fn week_gap_needs_per_instrument(watermark: Option<&str>, bulk_date: &str) -> bool {
    let parse = |date: &str| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok();
    let (Some(watermark), Some(bulk_date)) = (watermark.and_then(parse), parse(bulk_date)) else {
        return true;
    };
    super::incremental::week_monday(bulk_date) - super::incremental::week_monday(watermark)
        > chrono::Duration::days(7)
}

/// 把批量取数面的最新单位净值直接落库（ADR-0122 决策 2 / issue #1377）：现价
/// 缓存（现价 = 单位净值、`priced_at` = `nav_date` = 净值日期，与逐只通道同形）
/// 外加当周采样点（每周至多一条、整周覆盖幂等——落库单点与逐只通道共用
/// `upsert_price_history`）。只在批量面报出比水位更新的净值时调用（见
/// [`bulk_decision`]）。
///
/// 当周采样点只落**已有历史序列**的标的：无历史序列者落单点会让「有历史序列」
/// 冒充「历史完整」，永久破坏首刷判据（ADR-0038 决策 6）——首刷的历史由后台
/// 补全整根回填（issue #1377）。
async fn land_bulk_point<Q: ScopedSession>(
    session: &Q,
    fund: &super::incremental::SyncInstrument,
    hint: &BulkNavPoint,
) -> Result<()> {
    let price_cents = price_value_to_cents(hint.nav);
    let instrument_id = fund.instrument_id.clone();
    let currency = fund.currency.clone();
    let hint_date = hint.date.clone();
    session
        .with_connection(move |conn| {
            let has_history = ledger_investment::backfill::has_any_history(conn, &instrument_id)?;
            upsert_market_price(
                conn,
                &MarketPriceWrite {
                    instrument_id: &instrument_id,
                    price_cents,
                    currency_code: &currency,
                    priced_at: &hint_date,
                    nav_date: Some(&hint_date),
                    source: Some(EASTMONEY_PRICE_SOURCE),
                },
            )?;
            if has_history {
                upsert_price_history(
                    conn,
                    &instrument_id,
                    &hint_date,
                    price_cents,
                    &currency,
                    EASTMONEY_PRICE_SOURCE,
                )?;
            }
            Ok(())
        })
        .await?;
    Ok(())
}
