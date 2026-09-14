//! 资金加权收益率（MoneyWeightedReturn，ADR-0115 / issue #1195）：投资粒度的
//! 年化内部收益率（XIRR，实际天数 / 365）读投影——由实际现金流与期末市值解出，
//! 回答「我的钱实际赚了多少」，是账本唯一的收益率口径。
//!
//! **现金流口径（ADR-0115 决策 2，词汇表「资金加权收益率」词条）**：
//! - 买入为负（确认金额含手续费，即 buy 行 `transactions.amount_cents` 权威金额）、
//!   卖出为正（净额，已扣手续费）、现金分红为正；
//! - 份额调整（split）零现金腿，不入现金流集；
//! - 基金转换（convert）在**单标的粒度**按转出批次的结转成本（convert 行金额
//!   锚点 `t.amount_cents`，ADR-0099）拆成正负两条：转出腿为正、转入腿为负；
//!   账户级与全账级不额外引入——两腿同账户同日同额反号，聚合时天然抵净。
//!
//! **归因端点 = 现金流归因与出资账户口径一致（ADR-0096 结算账户语义）**：
//! 流水归属 `t.account_id`——buy/sell 行的 `account_id` 恒为投资账户（写入守卫
//! 保证），出资账户只改结算端点不改归属行；dividend 行的 `account_id` 即到账
//! 账户。红利再投（DRIP）按方案 B 落账（dividend 到账投资账户 + 0 手续费 buy
//! 自同一投资账户出资，ADR-0109 修订；#1288），两腿在账户内自相抵、不构成外部
//! 投入——本投影逐行计入两腿，聚合时自然抵净，无特判。到账账户为非投资账户的
//! 分红按现金归因进入全账级合计（投资收益已落袋），不进任何投资账户的账户级行。
//!
//! **期间与期初价值（ADR-0115 决策 3）**：默认自该粒度首笔流水起算、期末为
//! 现值（`v_holdings` 当前市值，现价 + 当期汇率）；支持区间选择——区间开始时
//! 的存量持仓按**区间首日市值**折为一笔期初投入（负现金流），区间结束的存量
//! 持仓按**区间末日市值**折为期末现金流。边界市值 = 时点存量数量（时点持仓
//! 推算接缝）× ≤ 该日最新周线价格 × 同期汇率（周键正反向兜底）——与组合走势
//! 同一套历史折算纪律，不用当期汇率近似历史。它是收益率的输入假设，不改账务、
//! 不构成第二套持仓口径。
//!
//! **空值与币种（ADR-0115 决策 4）**：缺价或缺汇率的持仓跳过、不计入该标的
//! 收益率，其现金流也不计入所属账户与全账合计（沿用 Holding 空值语义）；按
//! 币种分组（组内同币）、不跨币种折算（与 ADR-0107 同口径）。
//!
//! **无解不给数（ADR-0115 代价 1）**：现金流多次变号可能无解或多解，此时
//! 收益率输出 `None`（前端显式标注无法计算），绝不猜一个解。
//!
//! **展示粒度（ADR-0115 决策 5）**：持仓页每行（账户 × 标的）单标的收益率、
//! 盈亏页账户级与全账级；同一算法两个消费面，不新开页面。

use std::collections::{BTreeMap, HashMap};

use chrono::NaiveDate;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::holdings::holdings_as_of_in;
use super::lots::QTY_GUARD_EPSILON;
use super::staleness::beijing_today;
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::error::{AppError, Result};

// ---------------------------------------------------------------------------
// XIRR 求解器（纯函数，无 IO）
// ---------------------------------------------------------------------------

/// 解年化内部收益率（XIRR）：给定现金流 `(t, cf)` 序列（`t` 为距首笔流水的
/// 年化时间 = 实际天数 / 365，`cf` 为带符号金额），求使
/// `Σ cf_i · (1+r)^(−t_i) = 0` 的 `r > −1`。
///
/// 求解形态是**确定性二分**（无随机种子、无发散迭代）：`r → −1⁺` 时最大 `t`
/// 项主导（NPV 符号 → 末笔流符号），`r → ∞` 时 `t = 0` 项主导（NPV 符号 →
/// 首笔流符号）；两端异号由介值定理保证区间内有根，二分恒收敛到机器精度。
/// 两端同号（首末流同号）则可能无解或多解——按 ADR-0115「无解不给数」返回
/// `None`，不猜解。
///
/// 退化输入同样返回 `None`：少于两笔、缺正流或缺负流（无符号变化）、全部现金
/// 流落在同一时点（NPV 与 r 无关，无唯一解）。零金额流不入方程。
pub fn xirr(flows: &[(f64, f64)]) -> Option<f64> {
    let flows: Vec<(f64, f64)> = flows.iter().copied().filter(|(_, cf)| *cf != 0.0).collect();
    let has_neg = flows.iter().any(|(_, cf)| *cf < 0.0);
    let has_pos = flows.iter().any(|(_, cf)| *cf > 0.0);
    if flows.len() < 2 || !has_neg || !has_pos {
        return None;
    }
    let t_max = flows
        .iter()
        .map(|(t, _)| *t)
        .fold(f64::NEG_INFINITY, f64::max);
    if t_max.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        // 全部同一时点（含 t 为 NaN）：NPV 与 r 无关，无唯一解。
        return None;
    }
    let npv = |r: f64| -> f64 { flows.iter().map(|(t, cf)| cf / (1.0 + r).powf(*t)).sum() };

    // 括弧端点：lo 贴 −1⁺，hi 取 +1e9（年化超出此量级的账本场景不存在；
    // NPV(hi) 已被 t=0 项主导，再增大不改变符号）。端点运算允许 ±inf
    // （极长年限下 (1e-9)^t 下溢为 0）：仅用于符号比较，不入结果。
    let mut lo = -1.0 + 1e-9;
    let mut hi = 1e9f64;
    let mut f_lo = npv(lo);
    let f_hi = npv(hi);
    if f_lo == 0.0 {
        return Some(lo);
    }
    if f_hi == 0.0 {
        return Some(hi);
    }
    if (f_lo > 0.0) == (f_hi > 0.0) {
        // 首末流同号：可能无解或多解，不给数（ADR-0115 代价 1）。
        return None;
    }
    // 纯二分至机器精度（mid 不再前进即停）：行为确定，同输入恒同输出。
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if mid <= lo || mid >= hi {
            break;
        }
        let f_mid = npv(mid);
        if f_mid == 0.0 {
            return Some(mid);
        }
        if (f_mid > 0.0) != (f_lo > 0.0) {
            hi = mid;
        } else {
            lo = mid;
            f_lo = f_mid;
        }
    }
    Some(0.5 * (lo + hi))
}

// ---------------------------------------------------------------------------
// IPC 投影类型
// ---------------------------------------------------------------------------

/// 收益率查询区间（可选起止 ISO 8601 日期，`None` 表示该侧不设界）：
/// 区间开始时的存量持仓按区间首日市值折为期初投入（ADR-0115 决策 3）。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MwrRange {
    /// 起始日期（含），ISO 8601。
    pub start_date: Option<String>,
    /// 截止日期（含），ISO 8601。
    pub end_date: Option<String>,
}

/// 单标的收益率行（持仓页每行，账户 × 标的粒度）：`rate` 为年化内部收益率
/// （小数，0.1234 = 12.34%）；无法计算（缺价跳过 / 现金流无解）为 `None`。
#[derive(Debug, Serialize)]
pub struct InstrumentMwr {
    pub account_id: String,
    pub instrument_id: String,
    /// 计价币种 = 账户币种（现金流与期末市值同币，组内不跨币种折算）。
    pub currency_code: String,
    pub rate: Option<f64>,
}

/// 账户级收益率行（盈亏页账户粒度）：只覆盖投资账户（与盈亏页账户下拉同谓词）。
#[derive(Debug, Serialize)]
pub struct AccountMwr {
    pub account_id: String,
    pub account_name: String,
    pub currency_code: String,
    pub rate: Option<f64>,
}

/// 全账级按币种分组的收益率行：不做跨币种折算，各币种独立解年化。
#[derive(Debug, Serialize)]
pub struct CurrencyMwr {
    pub currency_code: String,
    pub rate: Option<f64>,
}

/// 资金加权收益率汇总（IPC 投影）：三个消费面共用同一现金流集与同一算法。
#[derive(Debug, Serialize)]
pub struct MoneyWeightedReturnSummary {
    pub by_instrument: Vec<InstrumentMwr>,
    pub by_account: Vec<AccountMwr>,
    pub total: Vec<CurrencyMwr>,
}

// ---------------------------------------------------------------------------
// 读投影
// ---------------------------------------------------------------------------

/// 收益率查询生产入口：期末不设界时以北京日历日为「今天」（行情 / 净值日历
/// 同源，先例：价格过期检查）。
pub fn query_money_weighted_return_summary(
    conn: &Connection,
    range: &MwrRange,
) -> Result<MoneyWeightedReturnSummary> {
    query_money_weighted_return_summary_on(conn, range, beijing_today())
}

/// [`query_money_weighted_return_summary`] 的可注入形态（时钟是测试的行为输入，
/// 先例：`instrument_price_staleness_on`）：给定今天与库内状态，结果唯一。
pub fn query_money_weighted_return_summary_on(
    conn: &Connection,
    range: &MwrRange,
    today: NaiveDate,
) -> Result<MoneyWeightedReturnSummary> {
    // 区间校验与解析一体：日期格式合法、起点不晚于终点，解析结果直接传给
    // 边界施加（不重复 parse）。
    let (start, end) = validate_range(range)?;

    // 1. 现金流集：按（账户 × 标的）配对装载（口径见模块头注）。
    let mut pairs: BTreeMap<(String, String), PairFlows> = BTreeMap::new();
    load_cash_flows(conn, range, &mut pairs)?;

    // 2. 边界现金流：期初投入（区间首日市值，负）与期末市值（区间末日 / 现值，正）；
    //    边界市值缺料而有存量的对整体跳过（ADR-0115 决策 4 空值语义）。
    apply_boundaries(conn, start, end, today, &mut pairs)?;

    // 3. 三个消费面同一算法：
    //    - 单标的：当前持仓对（v_holdings / 区间末日仍有存量）逐对解年化；
    //    - 账户级：投资账户名下未被跳过标的的流合集；
    //    - 全账级：按币种分组的全部未被跳过标的流合集（含到账非投资账户的分红）。
    let by_instrument: Vec<InstrumentMwr> = pairs
        .iter()
        .filter(|(_, p)| p.current_position && !p.unvalued)
        .map(|(key, p)| InstrumentMwr {
            account_id: key.0.clone(),
            instrument_id: key.1.clone(),
            currency_code: p.currency_code.clone(),
            rate: p.rate(),
        })
        .collect();

    let accounts: Vec<AccountRow> = query_all(
        conn,
        "SELECT id, name, currency_code, type FROM accounts WHERE is_deleted = 0",
        [],
    )?;
    let mut by_account: Vec<AccountMwr> = accounts
        .into_iter()
        .filter(|a| a.is_investment)
        .filter_map(|a| {
            let flows = merged_flows(&pairs, |key| key.0 == a.id)?;
            Some(AccountMwr {
                account_id: a.id,
                account_name: a.name,
                currency_code: a.currency_code,
                rate: rate_of(&flows),
            })
        })
        .collect();
    by_account.sort_by(|x, y| {
        x.account_name
            .cmp(&y.account_name)
            .then(x.currency_code.cmp(&y.currency_code))
    });

    let mut by_currency: BTreeMap<String, Vec<(NaiveDate, f64)>> = BTreeMap::new();
    for p in pairs.values() {
        if p.unvalued {
            continue;
        }
        by_currency
            .entry(p.currency_code.clone())
            .or_default()
            .extend(p.flows.iter().cloned());
    }
    let total: Vec<CurrencyMwr> = by_currency
        .into_iter()
        .filter(|(_, flows)| !flows.is_empty())
        .map(|(currency_code, flows)| CurrencyMwr {
            currency_code,
            rate: rate_of(&flows),
        })
        .collect();

    Ok(MoneyWeightedReturnSummary {
        by_instrument,
        by_account,
        total,
    })
}

/// 区间校验与解析：日期格式合法且起点不晚于终点（与走势查询同一纪律），
/// 返回解析后的起止日期供边界施加复用。
fn validate_range(range: &MwrRange) -> Result<(Option<NaiveDate>, Option<NaiveDate>)> {
    let parse = |label: &str, raw: &Option<String>| -> Result<Option<NaiveDate>> {
        raw.as_deref()
            .map(|s| {
                NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
                    AppError::codedp(
                        "instrument.mwr-date-invalid",
                        format!("{label}日期格式无效: {s}"),
                        &[s],
                    )
                })
            })
            .transpose()
    };
    let start = parse("起始", &range.start_date)?;
    let end = parse("截止", &range.end_date)?;
    if let (Some(s), Some(e)) = (start, end)
        && s > e
    {
        return Err(AppError::coded(
            "instrument.mwr-range-invalid",
            "起始日期不能晚于截止日期",
        ));
    }
    Ok((start, end))
}

/// 一条现金流的轻量行（SQL → 内存的中立形态）。
struct FlowRow {
    account_id: String,
    date: String,
    kind: String,
    amount_cents: i64,
    instrument_id: String,
    to_instrument_id: Option<String>,
}

impl FromRow for FlowRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(FlowRow {
            account_id: row.get(0)?,
            date: row.get(1)?,
            kind: row.get(2)?,
            amount_cents: row.get(3)?,
            instrument_id: row.get(4)?,
            to_instrument_id: row.get(5)?,
        })
    }
}

/// 装载现金流：buy/sell/dividend/convert 四类交易行（split 零现金腿不入），
/// 软删账户与软删流水排除（与 Holding / 已实现盈亏读口径对齐，issue #217 定案）；
/// 区间过滤 `(start, end]`——区间开始时点的存量价值由期初投入现金流承载，
/// 起始日（含）之前的流水不重复入集。
fn load_cash_flows(
    conn: &Connection,
    range: &MwrRange,
    pairs: &mut BTreeMap<(String, String), PairFlows>,
) -> Result<()> {
    let mut conditions: Vec<String> = vec!["t.is_deleted = 0".into(), "a.is_deleted = 0".into()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(start) = &range.start_date {
        params.push(Box::new(start.clone()));
        conditions.push(format!("t.date > ?{}", params.len()));
    }
    if let Some(end) = &range.end_date {
        params.push(Box::new(end.clone()));
        conditions.push(format!("t.date <= ?{}", params.len()));
    }
    let sql = format!(
        "SELECT t.account_id, t.date, t.kind, t.amount_cents, st.instrument_id, st.to_instrument_id \
         FROM transactions t \
         JOIN security_transactions st ON st.transaction_id = t.id \
         JOIN accounts a ON a.id = t.account_id \
         WHERE t.kind IN ('buy','sell','dividend','convert') AND {} \
         ORDER BY t.date",
        conditions.join(" AND ")
    );
    let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows: Vec<FlowRow> = query_all(conn, &sql, params_ref.as_slice())?;

    for row in rows {
        let date = parse_date(&row.date)?;
        if row.kind == "convert" {
            // 转换两腿（单标的粒度）：转出腿为正、转入腿为负，值 = 结转成本
            // （convert 行金额锚点，ADR-0099 / ADR-0115 决策 2）。
            let out_leg = row.instrument_id.clone();
            pairs
                .entry((row.account_id.clone(), out_leg))
                .or_default()
                .push(date, row.amount_cents as f64);
            if let Some(in_leg) = row.to_instrument_id.clone() {
                pairs
                    .entry((row.account_id.clone(), in_leg))
                    .or_default()
                    .push(date, -(row.amount_cents as f64));
            }
            continue;
        }
        // 买入为负（确认金额含手续费）、卖出为正（净额）、分红为正。
        let signed = match row.kind.as_str() {
            "buy" => -(row.amount_cents as f64),
            "sell" | "dividend" => row.amount_cents as f64,
            _ => continue,
        };
        pairs
            .entry((row.account_id.clone(), row.instrument_id.clone()))
            .or_default()
            .push(date, signed);
    }
    Ok(())
}

/// 一个（账户 × 标的）对的现金流集与边界状态。
#[derive(Default)]
struct PairFlows {
    /// 带日期现金流（金额单位分，符号即方向）。
    flows: Vec<(NaiveDate, f64)>,
    /// 计价币种 = 账户币种（现金流与期末市值同币，组内可直接比较）。
    currency_code: String,
    /// 当前有持仓（`v_holdings` 有行 / 区间末日仍有存量）：单标的行的展示资格。
    current_position: bool,
    /// 边界缺价或缺汇率：该对整体跳过（行不给数、流不入合计）。
    unvalued: bool,
}

impl PairFlows {
    fn push(&mut self, date: NaiveDate, amount: f64) {
        self.flows.push((date, amount));
    }

    /// 以该对现金流解年化：时间为实际天数 / 365，自首笔流水起算。
    fn rate(&self) -> Option<f64> {
        rate_of(&self.flows)
    }
}

/// 合并若干对的现金流（整体跳过的对除外）：账户级消费面共用。
fn merged_flows(
    pairs: &BTreeMap<(String, String), PairFlows>,
    pred: impl Fn(&(String, String)) -> bool,
) -> Option<Vec<(NaiveDate, f64)>> {
    let mut out: Vec<(NaiveDate, f64)> = Vec::new();
    for (key, p) in pairs {
        if p.unvalued || !pred(key) {
            continue;
        }
        out.extend(p.flows.iter().cloned());
    }
    if out.is_empty() { None } else { Some(out) }
}

/// 给定现金流解年化：时间为实际天数 / 365，自首笔流水起算（空集 `None`）。
fn rate_of(flows: &[(NaiveDate, f64)]) -> Option<f64> {
    let first = flows.iter().map(|(d, _)| *d).min()?;
    let converted: Vec<(f64, f64)> = flows
        .iter()
        .map(|(d, cf)| {
            (
                (d.signed_duration_since(first).num_days() as f64) / 365.0,
                *cf,
            )
        })
        .collect();
    xirr(&converted)
}

fn parse_date(raw: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
        AppError::codedp(
            "instrument.mwr-date-invalid",
            format!("流水日期格式无效: {raw}"),
            &[raw],
        )
    })
}

/// 账户参考行（账户级消费面的名称 / 币种 / 投资账户谓词）。
struct AccountRow {
    id: String,
    name: String,
    currency_code: String,
    is_investment: bool,
}

impl FromRow for AccountRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let kind: String = row.get(3)?;
        Ok(AccountRow {
            id: row.get(0)?,
            name: row.get(1)?,
            currency_code: row.get(2)?,
            is_investment: kind == "investment",
        })
    }
}

// ---------------------------------------------------------------------------
// 边界现金流：期初投入（区间首日市值）与期末市值（区间末日 / 现值）
// ---------------------------------------------------------------------------

/// 对每个（账户 × 标的）对施加边界现金流：
/// - 区间开始有存量 → 期初投入 `−(区间首日市值)`（负现金流，ADR-0115 决策 3）；
/// - 区间结束有存量 → 期末现金流 `+(末日市值)`；不设界时取 `v_holdings` 现值
///   （现价 + 当期汇率，Holding 空值语义：缺价 / 缺汇率为 NULL）。
///
/// 边界市值缺料（历史价格或同期汇率缺失）而有存量 → 该对标 `unvalued`，整体
/// 跳过。同时装载当前持仓标志与各对的计价币种（账户币种）。
fn apply_boundaries(
    conn: &Connection,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    today: NaiveDate,
    pairs: &mut BTreeMap<(String, String), PairFlows>,
) -> Result<()> {
    // 当前持仓对的现值与账户币种（期末不设界时的期末市值来源；v_holdings
    // 内部已排除软删账户）。同一对多币种 lot 时 SUM 各行市值（均为账户币）。
    let mut current: HashMap<(String, String), (Option<i64>, String)> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT v.account_id, v.instrument_id, SUM(v.market_value_cents), a.currency_code \
             FROM v_holdings v JOIN accounts a ON a.id = v.account_id \
             GROUP BY v.account_id, v.instrument_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (account_id, instrument_id, value, currency) = row?;
            current.insert((account_id, instrument_id), (value, currency));
        }
    }

    // 区间模式（任一侧设界）才需要历史市值；现值模式直接消费 v_holdings。
    // 起止侧的日期与其历史市值装载器成对持有（设界即已装载，免双重判空）。
    let start_boundary = match start {
        Some(date) => Some((date, AsOfValues::load(conn, date)?)),
        None => None,
    };
    let end_boundary = match end {
        Some(date) => Some((date, AsOfValues::load(conn, date)?)),
        None => None,
    };
    let end_date = end.unwrap_or(today);

    let mut keys: Vec<(String, String)> = pairs.keys().cloned().collect();
    for key in current.keys() {
        if !pairs.contains_key(key) {
            keys.push(key.clone());
        }
    }
    for key in keys {
        let pair = pairs.entry(key.clone()).or_default();
        if let Some((_, currency)) = current.get(&key) {
            pair.currency_code = currency.clone();
        }
        if pair.currency_code.is_empty() {
            // 无 v_holdings 行的对（已清仓 / 分红到非投资账户）：币种取账户币种。
            pair.currency_code = account_currency_of(conn, &key.0)?;
        }

        // 期初投入：区间开始时有存量 → 折为期初市值（负现金流）；市值缺料即跳过。
        if let Some((start_date, values)) = &start_boundary {
            let quantity =
                holdings_as_of_in(conn, Some(&key.0), Some(&key.1), &start_date.to_string())?;
            if quantity > QTY_GUARD_EPSILON {
                match values.market_value(quantity, &key.1, &pair.currency_code) {
                    Some(value) => pair.flows.push((*start_date, -(value as f64))),
                    None => {
                        pair.unvalued = true;
                        continue;
                    }
                }
            }
        }

        // 期末市值：设界取区间末日历史市值；不设界取现值（空值语义）。
        if let Some((boundary_date, values)) = &end_boundary {
            let quantity =
                holdings_as_of_in(conn, Some(&key.0), Some(&key.1), &boundary_date.to_string())?;
            if quantity > QTY_GUARD_EPSILON {
                pair.current_position = true;
                match values.market_value(quantity, &key.1, &pair.currency_code) {
                    Some(value) => pair.flows.push((*boundary_date, value as f64)),
                    None => pair.unvalued = true,
                }
            }
        } else if let Some((value, _)) = current.get(&key) {
            // 现值模式：v_holdings 有行即当前持仓；市值为 NULL 即缺价 / 缺汇率。
            pair.current_position = true;
            match value {
                Some(v) => pair.flows.push((end_date, *v as f64)),
                None => pair.unvalued = true,
            }
        }
    }
    Ok(())
}

/// 账户币种（无 v_holdings 行的对回退取用）。
fn account_currency_of(conn: &Connection, account_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id = ?1 AND is_deleted = 0",
        [account_id],
        |r| r.get(0),
    )
    .map_err(|_| {
        AppError::codedp(
            "instrument.mwr-account-missing",
            format!("收益现金流指向的账户不存在或已删除: {account_id}"),
            &[account_id],
        )
    })
}

/// 历史市值装载器：某截止日的每标的 ≤ 该日最新周线价格行 + 全量汇率历史。
/// 边界市值 = 时点存量数量 × 最新周线价格 × 同期汇率（周键正反向兜底）——
/// 与组合走势同一套历史折算纪律（不用当期汇率近似历史），缺任一环即缺料。
struct AsOfValues {
    /// 每标的 ≤ 截止日的最新周点 (trade_date, week_start, price_cents, currency)。
    latest_price: HashMap<String, (String, String, i64, String)>,
    /// 汇率历史：(base, quote) → week_start → rate（周粒度、币种对个位数，全量载入）。
    fx: HashMap<(String, String), HashMap<String, f64>>,
}

impl AsOfValues {
    fn load(conn: &Connection, as_of: NaiveDate) -> Result<Self> {
        let mut latest_price: HashMap<String, (String, String, i64, String)> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT instrument_id, trade_date, week_start, price_cents, currency_code \
                 FROM price_history WHERE trade_date <= ?1 ORDER BY instrument_id, trade_date",
            )?;
            let rows = stmt.query_map([as_of.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?;
            for row in rows {
                let (instrument_id, trade_date, week_start, price_cents, currency) = row?;
                // 升序扫描后写覆盖：即每标的 ≤ 截止日的最新周点。
                latest_price.insert(
                    instrument_id,
                    (trade_date, week_start, price_cents, currency),
                );
            }
        }
        let mut fx: HashMap<(String, String), HashMap<String, f64>> = HashMap::new();
        {
            let mut stmt = conn
                .prepare("SELECT base_code, quote_code, week_start, rate FROM fx_rate_history")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, f64>(3)?,
                ))
            })?;
            for row in rows {
                let (base, quote, week, rate) = row?;
                fx.entry((base, quote)).or_default().insert(week, rate);
            }
        }
        Ok(AsOfValues { latest_price, fx })
    }

    /// 边界市值（账户币种，分）= 存量数量 × 最新周线价格 × 同期汇率；
    /// 缺价格或缺汇率为 `None`（调用方按缺料跳过，不以零计入）。
    fn market_value(
        &self,
        quantity: f64,
        instrument_id: &str,
        account_currency: &str,
    ) -> Option<i64> {
        let (_, week_start, price_cents, price_currency) = self.latest_price.get(instrument_id)?;
        let rate = self.fx_rate(price_currency, account_currency, week_start)?;
        // 金额分 = 数量 × 单价（万分之一元）÷ 100（ADR-0038 刻度），再按同期汇率折算。
        let value = (quantity * *price_cents as f64 / 100.0).round();
        Some((value * rate).round() as i64)
    }

    /// 同期汇率（正查失败则反查取倒数），同币种为 1；与走势查询同思路。
    fn fx_rate(&self, base: &str, quote: &str, week_start: &str) -> Option<f64> {
        if base == quote {
            return Some(1.0);
        }
        if let Some(rate) = self
            .fx
            .get(&(base.to_string(), quote.to_string()))
            .and_then(|w| w.get(week_start))
        {
            return Some(*rate);
        }
        self.fx
            .get(&(quote.to_string(), base.to_string()))
            .and_then(|w| w.get(week_start))
            .map(|rev| 1.0 / rev)
    }
}
