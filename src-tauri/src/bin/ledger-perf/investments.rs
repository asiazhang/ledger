//! generate 扩展：投资域画像（issue #460 / ADR-0062「投资域随后扩展」）。
//!
//! 覆盖四块落库面：
//! - 标的字典（[`INSTRUMENTS`] 固定 20 行：A 股/ETF/港股/场外基金，来源随
//!   产品通道标记——同步标的 `eastmoney`、场外基金 `manual`，ADR-0036/0038）；
//! - 价格线（[`insert_market_data`]）：周采样 `price_history` 全窗口 +
//!   `market_prices` 现价缓存——现价恒等于该标的最新历史点（「现价 = 最新
//!   历史点的即时映像」，词汇表 MarketPrice 词条），场外基金现价带净值日期；
//! - 标的交易（[`plan_buy`] / [`plan_sell`] / [`apply_trade`]）：buy/sell 先是
//!   核心域 `transactions` 行（kind=buy/sell），再落 `security_transactions`
//!   扩展、buy 建仓 `security_lots`、sell FIFO 匹配 `security_lot_sales` 并
//!   扣减批次剩余——匹配序（created_at, id 升序）与分摊/闭合公式镜像产品
//!   Writer（`investment::trade`），含部分卖出与清仓两种形态；
//! - 交易金额口径与产品一致：`amount_cents = 数量 × 单价（万分之一元）÷ 100
//!   ± 费`（分），基金以金额权威、单价反算（issue #302）。
//!
//! 币种纪律：标的只与**同币种**投资账户交易（A 股/ETF/基金 ↔ CNY 账户、
//! 港股 ↔ HKD 账户）——产品 Writer 的金额公式不做标的价币 → 账户币折算，
//! 跨币种交易行不是产品能产出的形态；港股市值经 `v_holdings` 同币直出、
//! 折算发生在净资产/可投资资产聚合层（消费 `exchange_rates` HKD→CNY 行）。
//!
//! 确定性：全部价格走势由种子化 PRNG 推导（周漂移 ±3% 内），id 走
//! [`time_ordered_id`]（表隔离 tag），无墙钟参与。

use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::Connection;

use super::generate::date_millis;
use super::generate::{DEVICE_ID, GenCounts, Window};
use super::rng::{Rng, time_ordered_id};

// ---------------------------------------------------------------------------
// 画像常量（约数以固定值落地；变更须同步 bin 头注释与 tests）
// ---------------------------------------------------------------------------

/// 标的字典行数（固定清单，见 [`INSTRUMENTS`]）。
pub(crate) const INSTRUMENT_TOTAL: usize = 20;
/// 清仓占卖出比例：约 1/4 卖出为清仓，其余为部分卖出（持仓视图非空的前提）。
const FULL_SELL_RATE: f64 = 0.25;
/// 部分卖出的数量比例区间（可卖量的 30%–70%，向下取整到手/两位小数）。
const PARTIAL_SELL_MIN: f64 = 0.30;
const PARTIAL_SELL_SPAN: f64 = 0.40;
/// 交易手续费率：股票/ETF 佣金 0.03%、基金申购 0.15%（金额分，按金额计）。
const STOCK_FEE_RATE: f64 = 0.000_3;
const FUND_BUY_FEE_RATE: f64 = 0.001_5;
/// 周漂移幅度：每周价格围绕前值 ±3% 内漂移（种子化）。
const WEEK_DRIFT: f64 = 0.06;
/// 价格下限（万分之一元 = 0.1 元），防漂移穿零。
const MIN_PRICE_UNITS: i64 = 1_000;
/// 价格刻度：万分之一元/分（与产品 `PRICE_UNITS_PER_FEN` 同值，bin 内不引
/// 产品私有常量，注释锚定同源）。
const PRICE_UNITS_PER_FEN: f64 = 100.0;

/// 标的固定清单：12 只 A 股 + 3 只港股 + 2 只 ETF + 3 只场外基金。
/// 基准价（万分之一元）为「约 2025 年末量级」的形似锚点，走势由其漂移。
pub(crate) struct InstrumentSpec {
    pub symbol: &'static str,
    pub itype: &'static str,
    pub name: &'static str,
    pub ccy: &'static str,
    pub market: &'static str,
    pub source: &'static str,
    pub base_price: i64,
}

pub(crate) const INSTRUMENTS: [InstrumentSpec; INSTRUMENT_TOTAL] = [
    InstrumentSpec {
        symbol: "600519.SH",
        itype: "stock",
        name: "贵州茅台",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 14_000_000,
    },
    InstrumentSpec {
        symbol: "300750.SZ",
        itype: "stock",
        name: "宁德时代",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 2_500_000,
    },
    InstrumentSpec {
        symbol: "600036.SH",
        itype: "stock",
        name: "招商银行",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 350_000,
    },
    InstrumentSpec {
        symbol: "600900.SH",
        itype: "stock",
        name: "长江电力",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 280_000,
    },
    InstrumentSpec {
        symbol: "601318.SH",
        itype: "stock",
        name: "中国平安",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 500_000,
    },
    InstrumentSpec {
        symbol: "000858.SZ",
        itype: "stock",
        name: "五粮液",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 1_200_000,
    },
    InstrumentSpec {
        symbol: "000333.SZ",
        itype: "stock",
        name: "美的集团",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 700_000,
    },
    InstrumentSpec {
        symbol: "000651.SZ",
        itype: "stock",
        name: "格力电器",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 400_000,
    },
    InstrumentSpec {
        symbol: "002594.SZ",
        itype: "stock",
        name: "比亚迪",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 2_500_000,
    },
    InstrumentSpec {
        symbol: "601012.SH",
        itype: "stock",
        name: "隆基绿能",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 200_000,
    },
    InstrumentSpec {
        symbol: "002415.SZ",
        itype: "stock",
        name: "海康威视",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 300_000,
    },
    InstrumentSpec {
        symbol: "600030.SH",
        itype: "stock",
        name: "中信证券",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 250_000,
    },
    InstrumentSpec {
        symbol: "00700.HK",
        itype: "stock",
        name: "腾讯控股",
        ccy: "HKD",
        market: "hk",
        source: "eastmoney",
        base_price: 3_800_000,
    },
    InstrumentSpec {
        symbol: "03690.HK",
        itype: "stock",
        name: "美团-W",
        ccy: "HKD",
        market: "hk",
        source: "eastmoney",
        base_price: 1_200_000,
    },
    InstrumentSpec {
        symbol: "01810.HK",
        itype: "stock",
        name: "小米集团-W",
        ccy: "HKD",
        market: "hk",
        source: "eastmoney",
        base_price: 200_000,
    },
    InstrumentSpec {
        symbol: "510300.SH",
        itype: "etf",
        name: "沪深300ETF",
        ccy: "CNY",
        market: "sh",
        source: "eastmoney",
        base_price: 40_000,
    },
    InstrumentSpec {
        symbol: "510500.SH",
        itype: "etf",
        name: "中证500ETF",
        ccy: "CNY",
        market: "sz",
        source: "eastmoney",
        base_price: 60_000,
    },
    InstrumentSpec {
        symbol: "005827",
        itype: "fund",
        name: "易方达蓝筹精选",
        ccy: "CNY",
        market: "unknown",
        source: "manual",
        base_price: 25_000,
    },
    InstrumentSpec {
        symbol: "003095",
        itype: "fund",
        name: "中欧医疗健康",
        ccy: "CNY",
        market: "unknown",
        source: "manual",
        base_price: 18_000,
    },
    InstrumentSpec {
        symbol: "000961",
        itype: "fund",
        name: "天弘沪深300ETF联接A",
        ccy: "CNY",
        market: "unknown",
        source: "manual",
        base_price: 15_000,
    },
];

/// 场外基金判定（金额权威语义的分流条件，issue #302）。
fn is_fund(itype: &str) -> bool {
    itype == "fund"
}

/// 生成后的标的行：id 连同币种（交易行/批次/价格线的币种锚）。
pub(crate) struct InstrumentRow {
    pub id: String,
    pub ccy: &'static str,
}

/// 插入标的字典（20 行，固定清单）。
pub(crate) fn insert_instruments(
    conn: &Connection,
    stamp: &str,
    millis: i64,
    counts: &mut GenCounts,
) -> Result<Vec<InstrumentRow>, String> {
    let sql = "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,\
         created_at,updated_at,version,device_id,source) VALUES (?1,?2,?3,?4,?5,?6,?7,?7,1,?8,?9)";
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let mut rows = Vec::with_capacity(INSTRUMENT_TOTAL);
    for (seq, spec) in INSTRUMENTS.iter().enumerate() {
        let id = time_ordered_id("instruments", seq as u64, millis);
        stmt.execute(rusqlite::params![
            id,
            spec.symbol,
            spec.itype,
            spec.name,
            spec.ccy,
            spec.market,
            stamp,
            DEVICE_ID,
            spec.source,
        ])
        .map_err(|e| e.to_string())?;
        rows.push(InstrumentRow { id, ccy: spec.ccy });
    }
    counts.instruments = rows.len();
    Ok(rows)
}

// ---------------------------------------------------------------------------
// 价格线：周采样历史 + 现价缓存
// ---------------------------------------------------------------------------

/// 价格线内存形态：周采样交易日 + 每标的的周价格序列（供标的交易取当日价）。
pub(crate) struct MarketData {
    /// 窗口起点（周序号换算基准）。
    start: NaiveDate,
    /// 每标的的周价格序列（万分之一元），第 i 个元素对应第 i 个采样周。
    walks: Vec<Vec<i64>>,
}

impl MarketData {
    /// 标的在某交易日的近似价格（取所在周的采样价；窗口外钳到端点周）。
    pub fn price_at(&self, instrument_idx: usize, date: NaiveDate) -> i64 {
        let base = INSTRUMENTS[instrument_idx].base_price;
        let weeks = self.walks[instrument_idx].len();
        if weeks == 0 {
            return base;
        }
        let raw = (date - self.start).num_days() / 7;
        let idx = raw.clamp(0, weeks as i64 - 1) as usize;
        self.walks[instrument_idx][idx]
    }
}

/// 生成价格线：每标的从基准价起周内漂移（±3%），落 `price_history`（周采样，
/// UNIQUE(instrument, week) 恰一行不冲突）；现价缓存取各标的最后一个采样点
/// （现价 = 最新历史点映像），场外基金现价带净值日期。
pub(crate) fn insert_market_data(
    conn: &Connection,
    rng: &mut Rng,
    instruments: &[InstrumentRow],
    window: Window,
    counts: &mut GenCounts,
) -> Result<MarketData, String> {
    // 周采样交易日与 fx_rate_history 同法：周一进位、取当周周五、钳到结束日。
    let start = window.start;
    let mut monday = start - Duration::days(i64::from(start.weekday().num_days_from_monday()));
    let mut week_dates = Vec::new();
    while monday <= window.end {
        week_dates.push((monday + Duration::days(4)).min(window.end));
        monday += Duration::days(7);
    }

    let history_sql = "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,\
         currency_code,source,created_at,updated_at,version,device_id)\
         VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?6,1,?7)";
    let mut history = conn.prepare(history_sql).map_err(|e| e.to_string())?;
    let price_sql = "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,\
         priced_at,nav_date,source,created_at,updated_at,version,device_id)\
         VALUES (?1,?2,?3,?4,?5,?6,'eastmoney',?7,?7,1,?8)";
    let mut price_stmt = conn.prepare(price_sql).map_err(|e| e.to_string())?;

    let end_millis = date_millis(&window.end);
    let mut walks = Vec::with_capacity(instruments.len());
    let mut seq = 0u64;
    let mut history_rows = 0usize;
    for (idx, row) in instruments.iter().enumerate() {
        let spec = &INSTRUMENTS[idx];
        let mut walk = Vec::with_capacity(week_dates.len());
        let mut price = spec.base_price;
        for week_date in &week_dates {
            let drifted = (price as f64 * (1.0 + (rng.next_f64() - 0.5) * WEEK_DRIFT)).round();
            price = (drifted as i64).max(MIN_PRICE_UNITS);
            walk.push(price);
            let created = format!("{week_date}T16:00:00Z");
            history
                .execute(rusqlite::params![
                    time_ordered_id("price_history", seq, date_millis(week_date)),
                    row.id,
                    week_date.to_string(),
                    price,
                    row.ccy,
                    created,
                    DEVICE_ID,
                ])
                .map_err(|e| e.to_string())?;
            seq += 1;
            history_rows += 1;
        }
        // 现价 = 最后一个采样点（MarketPrice 即时映像语义）。
        let last_date = week_dates[week_dates.len() - 1];
        let nav_date = if is_fund(spec.itype) {
            Some(last_date.to_string())
        } else {
            None
        };
        price_stmt
            .execute(rusqlite::params![
                time_ordered_id("market_prices", idx as u64, end_millis),
                row.id,
                price,
                row.ccy,
                last_date.to_string(),
                nav_date,
                format!("{}T16:00:00Z", last_date),
                DEVICE_ID,
            ])
            .map_err(|e| e.to_string())?;
        walks.push(walk);
    }
    counts.market_prices = instruments.len();
    counts.price_history = history_rows;
    Ok(MarketData { start, walks })
}

// ---------------------------------------------------------------------------
// 标的交易：买入建仓 / 卖出 FIFO 匹配
// ---------------------------------------------------------------------------

/// 批次内存态：id 与剩余量、已计成本（卖出闭合用）——与 `security_lots` 行同步。
struct LotState {
    id: String,
    /// 创建时刻（由买入日期推导，产品 FIFO 匹配的排序键）。
    created: String,
    /// 买入交易日（卖出日期的下界：产品因果序先买后卖）。
    buy_date: NaiveDate,
    initial: f64,
    remaining: f64,
    cost_per_unit: i64,
    /// 该批次此前各次匹配已计的成本分（Σ round(匹配量 × 每份成本 ÷ 100)），
    /// 耗尽匹配按「批次总成本 − 已计」闭合（与产品 Writer 同式）。
    cost_allocated: i64,
}

/// 持仓批次内存账本：投资账户槽位 × 标的的批次表 + 确定性 id 序号。
/// 卖出匹配只按 pair 内插入序（FIFO）遍历，无 HashMap 遍历参与。
pub(crate) struct Portfolio {
    /// slot × INSTRUMENT_TOTAL + 标的序 → 批次序列（插入序 = FIFO 序）。
    pairs: Vec<Vec<LotState>>,
    next_lot_seq: u64,
    next_sale_seq: u64,
    buys: usize,
    sells: usize,
}

impl Portfolio {
    /// 由投资账户槽位数构建空账本（账户下标映射由调用方 TxContext 持有）。
    pub fn new(slot_count: usize) -> Self {
        let pairs = (0..slot_count * INSTRUMENT_TOTAL)
            .map(|_| Vec::new())
            .collect();
        Portfolio {
            pairs,
            next_lot_seq: 0,
            next_sale_seq: 0,
            buys: 0,
            sells: 0,
        }
    }

    /// 买入/卖出笔数（生成摘要与测试参考）。
    pub fn trade_counts(&self) -> (usize, usize) {
        (self.buys, self.sells)
    }
}

/// 一笔买入的完整计划（交易行字段 + 建仓副作用字段）。
pub(crate) struct BuyTrade {
    pub slot: usize,
    pub instrument_idx: usize,
    pub ccy: &'static str,
    pub quantity: f64,
    /// 成交单价（万分之一元）；基金为金额权威下的反算价。
    pub price_units: i64,
    pub fee_cents: i64,
    /// 交易行金额（分）：非基金 = 数量×单价÷100 + 费；基金 = 确认单金额。
    pub amount_cents: i64,
    pub cost_per_unit: i64,
    pub date: NaiveDate,
}

/// 一笔卖出的 FIFO 匹配计划。
pub(crate) struct SellTrade {
    pub slot: usize,
    pub instrument_idx: usize,
    pub ccy: &'static str,
    pub quantity: f64,
    pub price_units: i64,
    pub fee_cents: i64,
    /// 交易行金额（分）= 毛收入 − 费。
    pub amount_cents: i64,
    /// 毛收入（分，费前）：闭合基准（末匹配吸收余数）。
    gross_cents: i64,
    pub date: NaiveDate,
    matches: Vec<LotMatch>,
}

struct LotMatch {
    /// 批次在其 pair 序列中的下标。
    pos: usize,
    quantity: f64,
    cost_per_unit: i64,
    initial: f64,
    /// 是否耗尽该批次（决定成本闭合方式，与产品 Writer 同式）。
    exhausts: bool,
}

/// 计划一笔买入：随机标的 → 同币种投资账户 → 当日周价 → 手数/份额与费。
/// 返回 None 表示结构退化（无同币种投资账户，正常参数下不可达），调用方降级。
pub(crate) fn plan_buy(
    rng: &mut Rng,
    ccy_slots: &[(&'static str, Vec<usize>)],
    md: &MarketData,
    window: Window,
) -> Option<BuyTrade> {
    let inst = rng.below(INSTRUMENT_TOTAL as u64) as usize;
    let spec = &INSTRUMENTS[inst];
    let slots = ccy_slots
        .iter()
        .find(|(c, _)| *c == spec.ccy)
        .map(|(_, s)| s)?;
    let slot = *rng.pick(slots);
    let date = window.rand_date(rng);
    let price = md.price_at(inst, date);

    let (quantity, fee, amount, price_units, cost_per_unit) = if is_fund(spec.itype) {
        // 基金：确认单金额权威（整百元）；份额按当日行情价推导（两位小数），
        // 单价/每份成本由金额反算——反算价自然贴合价格线（若金额与份额独立
        // 抽样，反算单价会系统性偏离行情，未实现盈亏失真）。
        let amount = rng.range_i64(500, 5_000) * 100;
        let fee = (amount as f64 * FUND_BUY_FEE_RATE).round() as i64;
        let quantity =
            (((amount - fee) as f64 * PRICE_UNITS_PER_FEN / price as f64) * 100.0).floor() / 100.0;
        if quantity < 0.01 {
            // 行情极端高于确认金额档位（正常画像不可达）：返回 None 交调用方降级。
            return None;
        }
        let derived = ((amount - fee) as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
        let cost = (amount as f64 * PRICE_UNITS_PER_FEN / quantity).round() as i64;
        (quantity, fee, amount, derived, cost)
    } else {
        // 股票/ETF：1–2 手整百股，单价权威，金额 = 数量×单价÷100 + 佣金。
        let quantity = ((1 + rng.below(2)) * 100) as f64;
        let gross = (quantity * price as f64 / PRICE_UNITS_PER_FEN).round() as i64;
        let fee = (gross as f64 * STOCK_FEE_RATE).round() as i64;
        let amount = gross + fee;
        let cost = ((quantity * price as f64 + fee as f64 * PRICE_UNITS_PER_FEN) / quantity).round()
            as i64;
        (quantity, fee, amount, price, cost)
    };

    Some(BuyTrade {
        slot,
        instrument_idx: inst,
        ccy: spec.ccy,
        quantity,
        price_units,
        fee_cents: fee,
        amount_cents: amount,
        cost_per_unit,
        date,
    })
}

/// 计划一笔卖出：从有剩余量的（账户, 标的）组合中随机取一个，按可卖量的
/// 25% 概率清仓、否则部分卖出（30%–70%，整手/两位小数向下取整），FIFO 匹配。
/// 无任何可卖量时返回 None（调用方降级为买入）。
pub(crate) fn plan_sell(
    rng: &mut Rng,
    pf: &Portfolio,
    md: &MarketData,
    window: Window,
) -> Option<SellTrade> {
    let available: Vec<(usize, f64)> = pf
        .pairs
        .iter()
        .enumerate()
        .map(|(pair, lots)| (pair, lots.iter().map(|l| l.remaining).sum()))
        .filter(|(_, avail)| *avail > 0.0)
        .collect();
    if available.is_empty() {
        return None;
    }
    let &(pair, avail) = rng.pick(&available);
    let slot = pair / INSTRUMENT_TOTAL;
    let inst = pair % INSTRUMENT_TOTAL;

    // FIFO 候选（与产品 Writer 同序：created_at, id 升序）。
    let mut candidates: Vec<usize> = pf.pairs[pair]
        .iter()
        .enumerate()
        .filter(|(_, lot)| lot.remaining > 0.0)
        .map(|(pos, _)| pos)
        .collect();
    candidates.sort_by(|&a, &b| {
        let (la, lb) = (&pf.pairs[pair][a], &pf.pairs[pair][b]);
        (&la.created, &la.id).cmp(&(&lb.created, &lb.id))
    });
    let full = rng.chance(FULL_SELL_RATE);
    let fund = is_fund(INSTRUMENTS[inst].itype);
    let quantity = if full {
        avail
    } else {
        let frac = PARTIAL_SELL_MIN + rng.next_f64() * PARTIAL_SELL_SPAN;
        if fund {
            // 份额两位小数向下取整（可卖量恒两位小数，结果 ≤ 可卖量）。
            ((avail * frac) * 100.0).floor() / 100.0
        } else {
            // 整手向下取整；可卖量不足一手（碎股）时全卖。
            let hands = ((avail * frac) / 100.0).floor() as i64;
            if hands >= 1 {
                (hands * 100) as f64
            } else {
                avail
            }
        }
    };
    if quantity <= 0.0 {
        return None;
    }

    // 匹配集合只依赖数量与批次状态（与日期无关），先确定匹配，再取交易日：
    // 因果序——卖出日不早于任何一个被匹配批次的买入日（产品先买后卖）。
    let mut matches = Vec::new();
    let mut left = quantity;
    let mut matched_max_buy_date = window.start;
    for pos in &candidates {
        if left <= 0.0 {
            break;
        }
        let lot = &pf.pairs[pair][*pos];
        let exhausts = left >= lot.remaining;
        let matched = if exhausts { lot.remaining } else { left };
        if lot.buy_date > matched_max_buy_date {
            matched_max_buy_date = lot.buy_date;
        }
        matches.push(LotMatch {
            pos: *pos,
            quantity: matched,
            cost_per_unit: lot.cost_per_unit,
            initial: lot.initial,
            exhausts,
        });
        left -= matched;
    }
    let date = window.rand_date(rng).max(matched_max_buy_date);
    let price = md.price_at(inst, date);

    let gross = (quantity * price as f64 / PRICE_UNITS_PER_FEN).round() as i64;
    let fee = ((gross as f64 * STOCK_FEE_RATE).round() as i64).min(gross);
    let amount = gross - fee;

    Some(SellTrade {
        slot,
        instrument_idx: inst,
        ccy: INSTRUMENTS[inst].ccy,
        quantity,
        price_units: price,
        fee_cents: fee,
        amount_cents: amount,
        gross_cents: gross,
        date,
        matches,
    })
}

/// 标的交易副作用写入上下文：交易行落库后推导的确定性审计锚
/// （created 时间戳 + id 毫秒位），以及标的/账户 id（调用方从计划推导）。
pub(crate) struct SideEffects<'a> {
    pub instrument_id: &'a str,
    pub account_id: &'a str,
    pub created: &'a str,
    pub millis: i64,
}

/// 应用一笔买入副作用：`security_transactions` 扩发行 + 建仓批次。
fn apply_buy(
    conn: &Connection,
    txn_id: &str,
    trade: &BuyTrade,
    fx: &SideEffects,
    pf: &mut Portfolio,
) -> Result<(), String> {
    let created = fx.created;
    let millis = fx.millis;
    let instrument_id = fx.instrument_id;
    let account_id = fx.account_id;
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,\
         price_cents,fee_cents) VALUES (?1,?2,'buy',?3,?4,?5)",
        rusqlite::params![
            txn_id,
            instrument_id,
            trade.quantity,
            trade.price_units,
            trade.fee_cents,
        ],
    )
    .map_err(|e| e.to_string())?;
    let lot_id = time_ordered_id("security_lots", pf.next_lot_seq, millis);
    pf.next_lot_seq += 1;
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,\
         initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,\
         created_at,updated_at,version,device_id)\
         VALUES (?1,?2,?3,?4,?5,?5,?6,?7,?8,?8,1,?9)",
        rusqlite::params![
            lot_id,
            account_id,
            instrument_id,
            txn_id,
            trade.quantity,
            trade.cost_per_unit,
            trade.ccy,
            created,
            DEVICE_ID,
        ],
    )
    .map_err(|e| e.to_string())?;
    let pair = trade.slot * INSTRUMENT_TOTAL + trade.instrument_idx;
    pf.pairs[pair].push(LotState {
        id: lot_id,
        created: created.to_string(),
        buy_date: trade.date,
        initial: trade.quantity,
        remaining: trade.quantity,
        cost_per_unit: trade.cost_per_unit,
        cost_allocated: 0,
    });
    pf.buys += 1;
    Ok(())
}

/// 应用一笔卖出副作用：`security_transactions` 扩发行 + 逐批次 FIFO 匹配
/// （`security_lot_sales` 行 + 批次剩余量扣减）。收入/成本/费用的分摊与
/// 已实现盈亏公式镜像产品 Writer：收入末匹配吸收余数、耗尽批次成本按
/// 「批次总成本 − 已计」闭合、费用按量 floor 分摊末匹配吸收余数。
fn apply_sell(
    conn: &Connection,
    txn_id: &str,
    trade: &SellTrade,
    fx: &SideEffects,
    pf: &mut Portfolio,
) -> Result<(), String> {
    let created = fx.created;
    let millis = fx.millis;
    let instrument_id = fx.instrument_id;
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,\
         price_cents,fee_cents) VALUES (?1,?2,'sell',?3,?4,?5)",
        rusqlite::params![
            txn_id,
            instrument_id,
            trade.quantity,
            trade.price_units,
            trade.fee_cents,
        ],
    )
    .map_err(|e| e.to_string())?;

    let pair = trade.slot * INSTRUMENT_TOTAL + trade.instrument_idx;
    let count = trade.matches.len();
    let mut proceeds_total = 0i64;
    let mut fee_total = 0i64;
    for (i, m) in trade.matches.iter().enumerate() {
        let last = i == count - 1;
        let proceeds = if last {
            trade.gross_cents - proceeds_total
        } else {
            let p = (m.quantity * trade.price_units as f64 / PRICE_UNITS_PER_FEN).round() as i64;
            proceeds_total += p;
            p
        };
        let fee_alloc = if last {
            trade.fee_cents - fee_total
        } else {
            let f = (trade.fee_cents as f64 * m.quantity / trade.quantity).floor() as i64;
            fee_total += f;
            f
        };
        let lot_cost = {
            let lot = &pf.pairs[pair][m.pos];
            if m.exhausts {
                (m.initial * m.cost_per_unit as f64 / PRICE_UNITS_PER_FEN).round() as i64
                    - lot.cost_allocated
            } else {
                (m.quantity * m.cost_per_unit as f64 / PRICE_UNITS_PER_FEN).round() as i64
            }
        };
        let realized = proceeds - lot_cost - fee_alloc;
        let sale_id = time_ordered_id("lot_sales", pf.next_sale_seq, millis);
        pf.next_sale_seq += 1;
        conn.execute(
            "INSERT INTO security_lot_sales (id,sell_transaction_id,lot_id,quantity,\
             cost_per_unit_cents,realized_pnl_cents,currency_code,created_at)\
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![
                sale_id,
                txn_id,
                pf.pairs[pair][m.pos].id,
                m.quantity,
                m.cost_per_unit,
                realized,
                trade.ccy,
                created,
            ],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity-?1,updated_at=?2,\
             version=version+1,device_id=?3 WHERE id=?4",
            rusqlite::params![m.quantity, created, DEVICE_ID, pf.pairs[pair][m.pos].id],
        )
        .map_err(|e| e.to_string())?;
        let lot = &mut pf.pairs[pair][m.pos];
        lot.remaining -= m.quantity;
        if !m.exhausts {
            lot.cost_allocated += lot_cost;
        }
    }
    pf.sells += 1;
    Ok(())
}

/// 按计划落一笔标的交易的全部副作用（交易行已由主循环写入后调用）。
pub(crate) fn apply_trade(
    conn: &Connection,
    txn_id: &str,
    trade: TradeKind,
    fx: &SideEffects,
    pf: &mut Portfolio,
) -> Result<(), String> {
    match trade {
        TradeKind::Buy(t) => apply_buy(conn, txn_id, &t, fx, pf),
        TradeKind::Sell(t) => apply_sell(conn, txn_id, &t, fx, pf),
    }
}

/// 标的交易计划（买入/卖出的枚举形态，主循环统一持有）。
pub(crate) enum TradeKind {
    Buy(BuyTrade),
    Sell(Box<SellTrade>),
}

impl TradeKind {
    /// 交易行字段：金额（分）、日期、账户槽位。
    pub fn row_amount(&self) -> i64 {
        match self {
            TradeKind::Buy(t) => t.amount_cents,
            TradeKind::Sell(t) => t.amount_cents,
        }
    }

    pub fn row_date(&self) -> NaiveDate {
        match self {
            TradeKind::Buy(t) => t.date,
            TradeKind::Sell(t) => t.date,
        }
    }

    /// 交易行账户（账户表下标）：标的交易恒落在其投资账户上。
    pub fn account_slot(&self) -> usize {
        match self {
            TradeKind::Buy(t) => t.slot,
            TradeKind::Sell(t) => t.slot,
        }
    }

    pub fn ccy(&self) -> &'static str {
        match self {
            TradeKind::Buy(t) => t.ccy,
            TradeKind::Sell(t) => t.ccy,
        }
    }

    /// 标的序（调用方据此从标的表取 id 与批次坐标）。
    pub fn instrument_idx(&self) -> usize {
        match self {
            TradeKind::Buy(t) => t.instrument_idx,
            TradeKind::Sell(t) => t.instrument_idx,
        }
    }
}
