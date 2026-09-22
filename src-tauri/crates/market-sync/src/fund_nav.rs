//! 基金净值同步**共享件**（净值点形状、水位窗口、水位读、货基恒定净值与打标
//! 收尾单点）。
//!
//! **换源收口（issue #1571 / ADR-0130 决策 1）**：东财 lsjz 分页通道已整体删除——
//! 现价刷新的逐只回退与历史回填同用新浪单只全历史面（[`super::sina_fund`]，
//! 接线见 [`super::channels`]），原首刷专用的单请求全量通道（基金详情页数据
//! 文件，issue #1062）与建档确认点的档案/搜索面（issue #1212 / ADR-0039 修订）
//! 已随基金查询创建换源退役（issue #1568），ADR-0130 决策 1：不留「停用但可
//! 启用」的死代码。本模块不再发任何网络请求。
//!
//! 货币基金口径（issue #1342 / ADR-0126 决策 3 换源，issue #1563 接线）：货基的
//! 万份收益列不是单位净值（货基单位净值恒 [`MONEY_FUND_UNIT_NAV`]，收益以份额
//! 结转体现）。判定信号已改取证监会基金电子披露的自报形态（确认函数在
//! [`super::csrc`]，接线在逐只刷新与历史首刷两确认点）；确认前的打标收尾单点
//! [`mark_constant_price_on_confirm`] 同住本模块。
//!
//! - 水位窗口（[`nav_window`]）为纯函数，fixture 单测见 `tests/fund_nav.rs`；
//! - 逐只回退与历史回填两单元的编排分责见 [`super::fund_price_refresh`] 与
//!   [`super::fund_backfill`]（issue #1377 现价与历史解耦收口，issue #1388 拆分
//!   归位）；两者共用本模块的水位读与打标收尾件。

use chrono::NaiveDate;
use rusqlite::params;

use ledger_investment::PriceChannel;
use ledger_investment::constant_price::{ensure_constant_base_price, mark_constant_unit_price};
use ledger_investment::prices::{CSRC_PRICE_SOURCE, price_value_to_cents};

use ledger_infra::error::Result;

use super::session::ScopedSession;

/// 一个净值采样点：净值日期 + 单位净值（真实价格值，元）。
#[derive(Debug, Clone, PartialEq)]
pub struct NavPoint {
    pub(super) date: String,
    pub(super) nav: f64,
}

/// 基金单元的载体中立标的身份（spec #1677）：回填与现价刷新两单元的入参——
/// 编排侧从收集投影裁出所需字段传入，单元签名不再收编排模块的私有收集类型
/// `SyncInstrument`。
pub(super) struct FundIdentity<'a> {
    /// 标的 id（落库键）。
    pub(super) instrument_id: &'a str,
    /// 6 位真实代码（取数与判定门的键）。
    pub(super) code: &'a str,
    /// 报价币种。
    pub(super) currency: &'a str,
    /// 价格通道（收集时点的投影）：回填单元的恒定价格分支判定输入；通道判定
    /// 单点在投资域（issue #1060），本字段只透传不重派。
    pub(super) channel: PriceChannel,
}

impl FundIdentity<'_> {
    /// 从编排侧的收集投影裁出本单元所需字段（spec #1677 迁接）。
    pub(super) fn from_instrument(
        instrument: &super::incremental::SyncInstrument,
    ) -> FundIdentity<'_> {
        FundIdentity {
            instrument_id: &instrument.instrument_id,
            code: &instrument.symbol,
            currency: &instrument.currency,
            channel: instrument.channel,
        }
    }
}

/// 货币基金的恒定单位净值（issue #1342）：收益以份额结转体现，单位净值恒为
/// 1.0000——现价与恒定价兜底行都按此值收录，万份收益数值不参与。
pub(super) const MONEY_FUND_UNIT_NAV: f64 = 1.0;

/// 净值同步窗口（ADR-0038 决策 6）：有水位（现价缓存的净值日期）则从水位
/// 次日起（增量）；无水位为首刷，回填近两年。水位非法（理论不可达——写入侧
/// 恒为接口返回的 ISO 日期）按首刷兜底自愈，重复回填由周采样幂等覆盖吸收。
/// 起点只由入参水位决定；「是否首刷」由调用方
///（[`super::fund_backfill::backfill_one_fund_history`]）按有无历史序列判定后传 None
/// 表达（issue #1059），本函数不含那一层判据。
/// 返回 `(start, end)` 闭区间（ISO 日期）。
pub(super) fn nav_window(watermark: Option<&str>, today: NaiveDate) -> (String, String) {
    let start = match watermark.map(str::trim) {
        Some(w) if !w.is_empty() => match NaiveDate::parse_from_str(w, "%Y-%m-%d") {
            Ok(d) => d.succ_opt().unwrap_or(d),
            Err(_) => {
                tracing::warn!(watermark = %w, "净值水位非法，按首刷回填近两年");
                super::incremental::two_years_ago(today)
            }
        },
        _ => super::incremental::two_years_ago(today),
    };
    (
        start.format("%Y-%m-%d").to_string(),
        today.format("%Y-%m-%d").to_string(),
    )
}

/// 整只历史到窗口的本地裁剪（历史回填与现价刷新逐只回退共用，issue #1571）：
/// 新浪单只全历史面一次请求返回整只历史，窗口语义（首刷近两年 / 水位次日
/// 增量 / 现价短窗）由消费方用本函数裁剪表达——不依赖服务端窗口过滤行为。
/// 保留 `start..=end` 闭区间内的净值点（ISO 日期字符串比较，与 [`nav_window`]
/// 返回形态同源）。
pub(super) fn trim_to_window(points: Vec<NavPoint>, start: &str, end: &str) -> Vec<NavPoint> {
    points
        .into_iter()
        .filter(|p| p.date.as_str() >= start && p.date.as_str() <= end)
        .collect()
}

/// 覆盖不足判据（issue #1534）：最早历史周点所在周晚于首笔持仓流水日所在周
/// 即覆盖不足——已有近两年历史但覆盖不到持仓期起点的存量基金要深回填。
/// 无持仓流水（`None`）不存在「覆盖不足」：覆盖目标就是近两年（默认窗口，
/// 行为不变）；最早周点缺失或不可解析时保守判缺（宁可多采一轮，不静默放过
/// 深度缺口），与队列的缺周点判据 `week_behind` 同一品味。
pub(super) fn coverage_short_of_first_position(
    earliest_history: Option<&str>,
    first_position_date: Option<&str>,
) -> bool {
    let Some(first) = first_position_date.map(str::trim) else {
        return false;
    };
    let Ok(first) = NaiveDate::parse_from_str(first, "%Y-%m-%d") else {
        return false;
    };
    let Some(earliest) =
        earliest_history.and_then(|d| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok())
    else {
        return true;
    };
    super::weekly::week_monday(first) < super::weekly::week_monday(earliest)
}

/// 深回填窗口（issue #1534）：基金价格历史的覆盖深度由「近两年」放宽到
/// 「首笔持仓流水日所在周」——起点 = 近两年起点与首笔持仓流水日所在周周一
/// 的较早者（无持仓流水 / 不可解析 = 近两年，行为不变），终点 = 今天。基金
/// 首刷与深回填（覆盖不足的存量补齐）同用本窗口；增量窗口仍走 [`nav_window`]。
/// 首刷已走单请求全量通道（ADR-0038 决策 6 修订 / issue #1062），放宽深度
/// 不增加网络请求，只多落周采样行。
pub(super) fn deep_backfill_window(
    first_position_date: Option<&str>,
    today: NaiveDate,
) -> (String, String) {
    let mut start = super::incremental::two_years_ago(today);
    if let Some(d) = first_position_date
        .map(str::trim)
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    {
        start = start.min(super::weekly::week_monday(d));
    }
    (
        start.format("%Y-%m-%d").to_string(),
        today.format("%Y-%m-%d").to_string(),
    )
}

/// 恒定价格标的的打标收尾单点（ADR-0126 决策 3/5；issue #1563 判定门两确认点
/// 与排队竞态窗共用）：回填恒定单位价格标记（单向幂等）并兜底建档常量价
///（1.0000、净值日期空），返回是否实际落价（调用方据此计入价格写入见证）。
/// 取代三处逐字重复的打标块；写入单点 [`ledger_investment::constant_price`]，
/// 本函数不新增第二份落库 SQL。
pub(super) async fn mark_constant_price_on_confirm<Q: ScopedSession>(
    session: &Q,
    instrument_id: &str,
    currency_code: &str,
    priced_at: String,
) -> Result<bool> {
    let cents = price_value_to_cents(MONEY_FUND_UNIT_NAV);
    let instrument_id = instrument_id.to_string();
    let currency_code = currency_code.to_string();
    session
        .with_connection(move |conn| {
            mark_constant_unit_price(conn, &instrument_id, cents)?;
            ensure_constant_base_price(
                conn,
                &instrument_id,
                cents,
                &currency_code,
                &priced_at,
                // 恒定价格的确认源是官方披露自报形态（issue #1563 / #1568，
                // ADR-0130 决策 7：来源键随换源如实记 csrc）。
                CSRC_PRICE_SOURCE,
            )
        })
        .await
}

/// 基金分区水位与首刷判据的共享读（issue #1377 自两单元抽出）：水位 = 现价缓存
/// 的净值日期；首刷判据 = 磁盘上没有任何历史序列（issue #1059）。两条读经作用域
/// 会话短暂取一次连接完成（issue #1275）；抓取前不再触碰连接。
pub(super) async fn read_fund_watermark<Q: ScopedSession>(
    session: &Q,
    instrument_id: &str,
) -> Result<(Option<String>, bool)> {
    let instrument_id = instrument_id.to_string();
    session
        .with_connection(move |conn| {
            let watermark: Option<String> = conn
                .query_row(
                    "SELECT nav_date FROM market_prices WHERE instrument_id=?1",
                    params![instrument_id],
                    |r| r.get(0),
                )
                .ok();
            let has_history = ledger_investment::backfill::has_any_history(conn, &instrument_id)?;
            Ok((watermark, has_history))
        })
        .await
}

/// 基金深回填判据的共享读（issue #1534）：最早历史周点（当前覆盖起点）与
/// 首笔持仓流水日（覆盖目标深度，投资域单点 [`ledger_investment::holdings::first_position_date`]）。
/// 经作用域会话短暂取一次连接完成（issue #1275）。
pub(super) async fn read_fund_coverage<Q: ScopedSession>(
    session: &Q,
    instrument_id: &str,
) -> Result<(Option<String>, Option<String>)> {
    let instrument_id = instrument_id.to_string();
    session
        .with_connection(move |conn| {
            let earliest_history: Option<String> = conn
                .query_row(
                    "SELECT MIN(trade_date) FROM price_history WHERE instrument_id=?1",
                    params![instrument_id],
                    |r| r.get(0),
                )
                .map_err(ledger_infra::error::AppError::from)?;
            let first_position =
                ledger_investment::holdings::first_position_date(conn, &instrument_id)?;
            Ok((earliest_history, first_position))
        })
        .await
}
