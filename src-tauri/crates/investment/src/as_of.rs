//! 边界市值装载器（as-of market value）：某截止日的「每标的 ≤ 该日最新周采样
//! 价格 + 全量汇率历史」，边界市值 = 时点存量数量 × 最新周线价格 × 同期汇率
//! （周键正反向兜底）——与组合走势同一套历史折算纪律（不用当期汇率近似历史），
//! 缺任一环即缺料（`None`，调用方按空值语义跳过或标不可算，不以零计入）。
//!
//! 消费方（口径同源、装载器单点）：
//! - 资金加权收益率的边界现金流（ADR-0115 决策 3，区间首日 / 区间末日 / 起算日
//!   折入三处共用）。
//!
//! 恒定价格标的（ADR-0126 决策 6）：常量价对任意截止日成立，周键取截止日所在
//! 自然周；写覆盖保证存量平坦序列（若有）不再被消费。

use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::Connection;

use ledger_infra::error::Result;

/// 某截止日的每标的 ≤ 该日最新周线价格行 + 全量汇率历史。
pub(crate) struct AsOfValues {
    /// 每标的 ≤ 截止日的最新周点 (trade_date, week_start, price_cents, currency)。
    latest_price: HashMap<String, (String, String, i64, String)>,
    /// 汇率历史：(base, quote) → week_start → rate（周粒度、币种对个位数，全量载入）。
    fx: HashMap<(String, String), HashMap<String, f64>>,
}

impl AsOfValues {
    pub(crate) fn load(conn: &Connection, as_of: NaiveDate) -> Result<Self> {
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
        // 恒定标的覆盖（ADR-0126 决策 6）：常量价对任意截止日成立，周键取截止
        // 日所在自然周；写覆盖保证存量平坦序列（若有）不再被消费——边界市值
        // 各消费面共用本装载器，取值口径单点。
        for constant in crate::constant_price::load_constant_prices(conn)? {
            latest_price.insert(
                constant.instrument_id,
                (
                    as_of.to_string(),
                    crate::constant_price::week_monday(as_of).to_string(),
                    constant.price_cents,
                    constant.currency_code,
                ),
            );
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
    /// 缺价格或缺汇率为 `None`（调用方按缺料跳过 / 标不可算，不以零计入）。
    pub(crate) fn market_value(
        &self,
        quantity: f64,
        instrument_id: &str,
        account_currency: &str,
    ) -> Option<i64> {
        let (_, week_start, price_cents, price_currency) = self.latest_price.get(instrument_id)?;
        let rate = self.fx_rate(price_currency, account_currency, week_start)?;
        // 金额分 = 数量 × 单价（万分之一元）÷ 换算因子（价格刻度，ADR-0038），
        // 再按同期汇率折算。
        let value = (quantity * *price_cents as f64 / crate::prices::PRICE_UNITS_PER_FEN).round();
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
