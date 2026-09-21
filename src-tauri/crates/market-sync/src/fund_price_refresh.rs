//! 基金**现价刷新**单元（issue #1388 自 `fund_nav` 拆出；ADR-0122 决策 2/3 /
//! issue #1377 现价与历史解耦）：只刷现价、不承担历史——首刷深回填与缺周点深补
//! 归价格历史后台补全（[`super::fund_backfill::backfill_one_fund_history`]）。
//! 取数面（[`super::bulk`]）命中时整只零请求；未命中退逐只通道——新浪单只全
//! 历史面一次请求（与历史回填同面同通道，issue #1571），窗口由本地裁剪表达。
//!
//! 共享件（水位窗口、水位读）留守 `fund_nav`，本模块只收现价刷新的编排与取数
//! 面判定。

use chrono::NaiveDate;

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_investment::prices::{
    MarketPriceWrite, SINA_PRICE_SOURCE, price_value_to_cents, upsert_market_price,
    upsert_price_history,
};

use super::bulk::BulkNavPoint;
use super::channels::FetchFuture;
use super::fund_nav::{
    NavPoint, mark_constant_price_on_confirm, nav_window, read_fund_watermark, trim_to_window,
};
use super::http::KlineBar;
use super::session::ScopedSession;

/// 现价刷新逐只回退的无历史序列短窗（issue #1377）：首刷的历史由后台补全整根
/// 回填，现价刷新只为这类标的拿「最新公布净值」——一个月窗口、常数请求。
const REFRESH_RECENT_WINDOW_MONTHS: chrono::Months = chrono::Months::new(1);

/// 基金分区的同步统计（与 [`super::incremental`] 的股票统计同源汇总）：
/// `synced` = 处理成功（含「已是最新、无新净值」）；`skipped` = 无法拉取
/// （首刷查无净值；名称充
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
/// 未命中（缺口 / 降级 / 停用期）退回逐只通道——新浪单只全历史面（与历史回填
/// 同面同通道，issue #1571），一次请求整只历史、窗口由本地裁剪表达：
/// 有历史序列者裁到水位次日增量，窗口内缺周点顺带落库（完整性由全历史面的
/// 声明总数核对保证，半截历史在取数层即报错）；无历史序列者裁到近一个月短窗
///（[`REFRESH_RECENT_WINDOW_MONTHS`]）的最新净值落现价，历史一行为零、
/// 不落采样点（历史归后台补全首刷）。取数失败（网络 / 报文不可信 / 声明总数
/// 未取全）上抛——全历史面 fail-closed（issue #1566 同款），不把不可信结果当
/// 「无净值」。
///
/// **判定门前置（issue #1563 / ADR-0126 决策 3 换源）**：未打标标的进入逐只通道
/// 即先查官方披露自报形态——确认即打标收尾（单向，幂等；建档常量价行缺失时兑底
/// 一行），**零逐只净值请求**；自下一轮同步起标的退出采集链路（编排层分区排除），
/// 判定不进日常热路径（确认后不产生逐轮请求）。缺信号（披露记录为普通净值形态
/// 或无记录）照常取数落库——缺信号不是「不是恒定标的」的反证，更不清空既有
/// 标记；披露源不可信（Err）则本轮整只不落——信号缺席时落取数面的取值位，
/// 万份收益就回冒充单位净值（#1342 错法）。
/// 单只结果累加进调用方的 `stats`。基金间的遍历、名称随行刷新与标的级进度推进归编排层
///（issue #897）。跳过语义与 [`super::fund_backfill::backfill_one_fund_history`] 一致：查无净值
///（全历史面可信空或窗口裁空）与披露源不可信计入 `skipped`，不报错不中断；
/// 取数失败上抛中断同步（fail-closed，不可信不落库）。
///
/// **前置条件**：`fund` 为净值通道（FundNav）的 6 位真实代码基金行——名称充
/// 代码行（查不到净值）由调用方计入跳过、零请求（issue #897 起跳过判定与分母
/// 口径同收编排层）；恒定价格标的由编排层分区排除、不经本函数（ADR-0126
/// 决策 4 / #1451），未打标货基的打标确认由上方官方披露判定门承担。
pub(super) async fn refresh_one_fund_price<Q, H, C>(
    session: &Q,
    fund: &super::incremental::SyncInstrument,
    latest_hint: Option<&BulkNavPoint>,
    fetch_nav_history: &mut H,
    confirm_money_fund: &mut C,
    stats: &mut FundSyncStats,
) -> Result<()>
where
    // 作用域会话接缝（issue #1275）：本函数读写库的唯一通道，签名层面取不到连接。
    Q: ScopedSession,
    // 新浪单只全历史通道（issue #1571 起与历史回填同通道）：6 位代码 → 整只
    // 历史单位净值。
    H: FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send,
    // 货基判定确认通道（issue #1563）：6 位代码 → 官方披露自报形态三态。
    C: FnMut(&str) -> FetchFuture<bool> + Send,
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
                    "批量面已报出新净值但水位落后超过一个自然周，逐只通道补齐缺失周点"
                );
            }
        }
    }
    // 判定门前置（issue #1563 / ADR-0126 决策 3 换源）：官方披露自报形态确认即
    // 打标收尾——不发起逐只净值请求，万份收益不得经取数面冒充单位净值落库
    //（#1342）。确认单向幂等：已标记行零触碰、缺信号不清空；披露源不可信则
    // 本轮整只不落（跳过，与被拦截同桶），下一窗口重试。
    match confirm_money_fund(&fund.symbol).await {
        Ok(true) => {
            let written = mark_constant_price_on_confirm(
                session,
                &fund.instrument_id,
                &fund.currency,
                today.format("%Y-%m-%d").to_string(),
            )
            .await?;
            if written {
                stats.written += 1;
            }
            stats.synced += 1;
            return Ok(());
        }
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(
                code = %fund.symbol, %error,
                "官方披露判定源不可信，本轮整只不落（信号缺席时落取值位即万份收益冒充净值）"
            );
            stats.skipped += 1;
            return Ok(());
        }
    }
    // 逐只回退的窗口按「现价刷新」收窄（issue #1377）：有历史序列者从水位次日
    // 增量；无历史序列者取近一个月短窗拿最新净值。窗口由本地裁剪表达——一次
    // 请求拿整只历史（与历史回填同面，issue #1571），不依赖服务端窗口过滤行为。
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
    let points: Vec<NavPoint> = fetch_nav_history(&fund.symbol).await?;
    let collected: Vec<NavPoint> = trim_to_window(points, &start, &end);

    if collected.is_empty() {
        if has_history {
            // 增量窗口内确实无新净值（T+1 正常空窗）：现价已是最新，处理成功
            // 但不落库、不计跳过。（全历史面的可信空与报错在取数层已区分开，
            // issue #1566。）
            stats.synced += 1;
        } else {
            // 查无净值（查无此码 / 新基金未公布首期 / 已终止基金末点在窗口外）：
            // 无法拉取，计入跳过。
            stats.skipped += 1;
        }
        return Ok(());
    }
    // 现价 = 窗口内最新公布单位净值（let-else 显式防线，#434 同款）：points
    // 非空由前文判空保证，此臂理论不可达；一旦前置防线被移除，此处记警告并
    // 跳过该只、不中断同步。
    let Some(latest) = collected.iter().max_by_key(|p| p.date.as_str()) else {
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
    let bars: Vec<KlineBar> = collected
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
                    // 窗口内缺失的近期周点顺带落库（完整性由全历史面的声明总数
                    // 核对保证；深度缺周点已在上方交后台补全）。
                    super::incremental::write_weekly_price_history(
                        conn,
                        &instrument_id,
                        &currency,
                        &bars,
                        SINA_PRICE_SOURCE,
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
                        // 价格来源随取数面换源如实记新浪（ADR-0130 决策 7 / issue #1571）——
                        // 存量 `eastmoney` 行保留为历史事实，不重写不迁移。
                        source: Some(SINA_PRICE_SOURCE),
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
/// **本臂不经货基判定门的前提由批量面取数层的行形态判别承接**（issue #1565 /
/// ADR-0130 决策 2/6）：新浪 `f_` 面含货基且把万份收益放在单位净值位，但取数层
/// 按「前一日单位净值位为空」判形为 `MoneyYield`、
/// [`super::sina_fund::SinaFundNavRow::into_nav_point`] 对它**不产出价格点**——
/// 它只进批量面的名称字典、不进净值表，于是 `latest_hint` 为 None、落逐只臂由
/// 官方披露判定门（[`super::csrc::confirm_money_fund_form`]）确认收尾，万份收益
/// 永不进本臂。若要给未打标标的的批量直落再加一道判定门，代价是每只未打标基金
/// 每次同步一次官方披露请求（与「批量命中零逐只请求」相悖）——本臂只收普通净值行
/// 的形态保证就是那道门的等价物。
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
                    // 价格来源随取数面换源如实记新浪（ADR-0130 决策 7 / issue #1565）——
                    // 存量 `eastmoney` 行保留为历史事实，不重写不迁移。
                    source: Some(SINA_PRICE_SOURCE),
                },
            )?;
            if has_history {
                upsert_price_history(
                    conn,
                    &instrument_id,
                    &hint_date,
                    price_cents,
                    &currency,
                    SINA_PRICE_SOURCE,
                )?;
            }
            Ok(())
        })
        .await?;
    Ok(())
}
