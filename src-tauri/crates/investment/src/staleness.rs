//! 价格过期检查（issue #1190）：打开投资页时的**本地水位检查**单点——零网络
//! 请求，只读本地现价缓存，回答「有哪些标的的行情 / 净值可能已经陈旧」。
//!
//! **水位不新造**：就是现价缓存里既有的两条时点事实（ADR-0038 / 词汇表
//! InstrumentInfoSync「水位增量语义」）——
//! - 行情通道（[`PriceChannel::Quote`]）标的：`market_prices.priced_at`，
//!   同步刷现价时写入的采集时刻；
//! - 净值通道（[`PriceChannel::FundNav`]）标的：`market_prices.nav_date`，
//!   最新公布单位净值的日期，兼任净值同步水位。
//!
//! **过期判定**：水位日（北京日历日，与行情日历同源）距今**超过**
//! [`PRICE_STALE_AFTER_DAYS`] 个自然日即计入过期；水位整个缺失时，只有
//! **持仓标的**计入（「持仓缺现价」——账本里真有一笔资产没有估值依据），
//! 纯建档未交易的行不惊动用户。已清仓标的另有退出臂：水位陈旧超过
//! [`PRICE_STALE_CLEARED_EXIT_DAYS`]（一年）即退出计数面——数据源已停止
//! 披露，同步永远修不了（见常量注记）。阈值取 3 个自然日的理由：周五同步
//! 的正常水位在周六 / 周日 / 周一是 1 / 2 / 3 天，均不提示；到周二（4 天）才
//! 提示——此时周一的行情 / 净值确实已可拉取而本地没有。
//!
//! **计数面 = 全库有通道标的**（含已清仓），与同步覆盖面（库内全部标的，
//! issue #827）同面：计数回答「同步能修好多少只」。**提示措辞只陈述标的自身**
//! ——不得声称「报表会按陈旧价格计算」，报表与净资产只吃持仓，那句话对已清仓
//! 标的不成立（同口径见词汇表「价格过期提示」）。文案与计数面同进同退。
//!
//! **检查面**：只含有价格写入需求的行（行情 / 净值），手动报价、无来源与恒定
//! 价格通道不在其列——「同步标的信息」修不了它们（手动通道的出路是录价，
//! issue #1193；手动通道的持仓缺价因此也不计入，不是漏算；恒定价格没有水位
//! 可言，提示的计数面整体豁免它，ADR-0126 决策 7）。判定复用价格通道派生单点
//! ([`derive_price_channel`]），不按类型与市场自行推断第二口径。
//!
//! **已知边界（接受）**：长假 / 长期停牌 / 基金停止披露净值期间，数据源没有
//! 新点、同步不落库，水位不会变新——提示在此期间持续存在，点同步也消不掉。
//! 接受的理由：此时本地价确实不是最新的，提示是诚实陈述；仓库没有「最近是否
//! 有交易日」的事实源，造一份即新口径，且「窗口内是否真有新数据」只有网络
//! 事实能答，与零网络请求冲突。阈值 3 个自然日已吸收普通周末与 T+1 延迟。
//! 持续存在面自 #1663 起有界：已清仓行水位陈旧超过一年即退出计数面（清盘
//! 类永久无新点的行不再永久噪声），残余的持续提示只剩持仓行（估值依据确实
//! 陈旧）与一年内的清仓行。
//!
//! **刻意不做**：自动同步、定时轮询（ADR-0015 / ADR-0095 的显式触发口径保留）
//! ——本检查只产出计数，提示与「去同步」入口由壳层接线。

use chrono::NaiveDate;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::channel::{PriceChannel, derive_price_channel};
use super::market::Market;
use super::model::InstrumentType;
use super::predicates::INVESTED_EXISTS;
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::error::Result;

/// 过期阈值（自然日）：水位日距今**超过**本值即视为过期。
///
/// 3 个自然日是「一个周末 + 一个交易日」的容量——周五的水位在随后的
/// 周六（1 天）/ 周日（2 天）/ 周一（3 天）都不提示，周二（4 天）起提示。
/// 放大到「一周」会漏掉整周的陈旧价，压到 1 天则每个周末都误报。
pub const PRICE_STALE_AFTER_DAYS: i64 = 3;

/// 已清仓标的的水位退出阈值（自然日）：水位距今超过本值的**非持仓**行不再
/// 计入过期（issue #1663）。
///
/// 一年没有新点即视作数据源已停止披露（基金清盘 / 标的长期停牌）——同步
/// 永远修不了它；且已清仓标的的陈旧价不影响报表与净资产（只吃持仓），提示
/// 对它没有可执行动作，持续提示只是永久噪声。**持仓行不豁免**：账本里真有
/// 一笔资产的估值依据陈旧，提示仍然诚实。与 [`PRICE_STALE_AFTER_DAYS`] 的
/// 分工：3 日是「提示」阈值（水位新鲜与否），本值是「退出」阈值（同步还
/// 修不修得了），后者只收窄前者的长尾，不改正常判定。
pub const PRICE_STALE_CLEARED_EXIT_DAYS: i64 = 365;

/// 价格过期检查结果（IPC 投影）：提示文案与判定共用同一份计数与阈值，
/// 文案里的天数不另抄一份常量。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceStaleness {
    /// 「同步标的信息」能修好的标的数：水位过期的行情 / 净值通道标的（已
    /// 清仓行水位陈旧超过一年退出，见 [`PRICE_STALE_CLEARED_EXIT_DAYS`]），
    /// 加水位缺失的持仓标的。
    pub stale_count: usize,
    /// 判定阈值（自然日），随结果透出供提示文案引用。
    pub threshold_days: i64,
}

/// 价格过期检查生产入口：以北京日历日为「今天」。
pub fn instrument_price_staleness(conn: &Connection) -> Result<PriceStaleness> {
    instrument_price_staleness_on(conn, beijing_today())
}

/// [`instrument_price_staleness`] 的可注入形态（时钟是测试的行为输入，
/// 先例：`add_fund_by_code_with` 的注入接缝）：判定本身是与「今天」无关的
/// 纯读——给定今天与库内状态，结果唯一。
pub fn instrument_price_staleness_on(
    conn: &Connection,
    today: NaiveDate,
) -> Result<PriceStaleness> {
    let rows = query_all::<PriceWatermarkRow, _>(
        conn,
        &format!(
            "SELECT i.instrument_type,i.market,i.symbol,i.constant_unit_price,p.priced_at,p.nav_date, \
             CASE WHEN {INVESTED_EXISTS} THEN 1 ELSE 0 END AS invested \
             FROM instruments i \
             LEFT JOIN market_prices p ON p.instrument_id = i.id"
        ),
        [],
    )?;

    let stale_count = rows
        .iter()
        .filter(|row| row.is_stale(today, PRICE_STALE_AFTER_DAYS))
        .count();

    Ok(PriceStaleness {
        stale_count,
        threshold_days: PRICE_STALE_AFTER_DAYS,
    })
}

/// 一条标的的现价水位行（判定输入：通道判定所需三列 + 恒定单位价格 + 两条
/// 时点列 + 持仓标志）。
struct PriceWatermarkRow {
    kind: InstrumentType,
    // DB 读边界 parse 一次（市场闭集类型，issue #1673）。
    market: Market,
    symbol: String,
    /// 恒定单位价格（万分之一元，ADR-0126）：通道判定的持久化输入。
    constant_unit_price: Option<i64>,
    /// 行情通道水位（同步写入时刻，`market_prices.priced_at`）。
    priced_at: Option<String>,
    /// 净值通道水位（净值日期，`market_prices.nav_date`）。
    nav_date: Option<String>,
    /// 是否持仓标的（`INVESTED_EXISTS` 派生，缺价行是否惊动用户只由它决定）。
    invested: bool,
}

impl PriceWatermarkRow {
    /// 本行是否计入过期：无价格写入需求的行一律不计（同步修不了）——手动
    /// 报价与无来源没有可刷新的价格，恒定价格没有水位可言（价格不随时间
    /// 变，同步既修不了也不需要修，ADR-0126 决策 7，「持仓缺现价」一档一并
    /// 豁免）；有通道行按水位判定——水位缺失只有持仓行计入（持仓缺现价），
    /// 水位存在则看它距今是否超过阈值，已清仓行再受退出阈值收窄（超过一年
    /// 的陈旧视作数据源已停止披露，同步永远修不了，退出计数面，#1663）。
    fn is_stale(&self, today: NaiveDate, threshold_days: i64) -> bool {
        let channel = derive_price_channel(
            self.kind,
            self.market,
            &self.symbol,
            self.constant_unit_price,
        );
        let watermark = match channel {
            PriceChannel::Quote => self.priced_at.as_deref().and_then(quote_watermark_date),
            PriceChannel::FundNav => self.nav_date.as_deref().and_then(iso_date),
            PriceChannel::Constant | PriceChannel::Manual | PriceChannel::None => return false,
        };
        match watermark {
            Some(date) => {
                let days = today.signed_duration_since(date).num_days();
                if days <= threshold_days {
                    false
                } else if self.invested {
                    true
                } else {
                    days <= PRICE_STALE_CLEARED_EXIT_DAYS
                }
            }
            None => self.invested,
        }
    }
}

impl FromRow for PriceWatermarkRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PriceWatermarkRow {
            kind: row.get(0)?,
            market: row.get(1)?,
            symbol: row.get(2)?,
            constant_unit_price: row.get(3)?,
            priced_at: row.get(4)?,
            nav_date: row.get(5)?,
            invested: row.get::<_, i64>(6)? != 0,
        })
    }
}

/// 行情通道水位 → 北京日历日：同步写入的是 UTC ISO 时间戳
/// （`db::now_iso`，如 `2026-09-13T09:41:00Z`），先转北京时区再取日期；
/// 多端重放可能带来的纯日期形态（`YYYY-MM-DD`）按北京日历日直接读。
/// 两种形态都解析不出返回 `None`（按水位缺失处置，不静默当作新数据）。
fn quote_watermark_date(raw: &str) -> Option<NaiveDate> {
    let raw = raw.trim();
    match chrono::DateTime::parse_from_rfc3339(raw) {
        Ok(at) => Some(beijing_date(at.with_timezone(&chrono::Utc))),
        Err(_) => iso_date(raw),
    }
}

/// ISO 日期（`YYYY-MM-DD`）解析；非法返回 `None`。
fn iso_date(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").ok()
}

/// 北京日历日的「今天」（行情 / 净值日历同源）。
pub(super) fn beijing_today() -> NaiveDate {
    beijing_date(chrono::Utc::now())
}

/// [`beijing_today`] 的纯函数核：UTC 时刻加 8 小时后取日期部分即北京日历日
/// （16:00 UTC 是北京午夜边界）。与行情同步域的同一规则各自持有实现——
/// 两域互不依赖（同步域含 HTTP 栈），边界语义各自由测试钉住。
pub(super) fn beijing_date(now: chrono::DateTime<chrono::Utc>) -> NaiveDate {
    (now + chrono::Duration::hours(8)).date_naive()
}
