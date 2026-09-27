//! 周键汇率取数单点（issue #1849 收口；就近窗口 #1844，父 spec #1843）：读侧
//! 对汇率历史（FxRateHistory）周键索引的取数收口本模块——
//!
//! - [`fx_rate_at_week`]：单周取汇率原语（同币种恒等 / 正查 / 反查取倒数），
//!   组合走势（[`crate::trend`]）与边界市值（[`crate::as_of`]）的同期折算
//!   消费单点；
//! - [`nearest_week_fx_rate`]：按事件日期就近取汇率（±8 周窗口
//!   [`FX_NEAREST_WINDOW_WEEKS`] 兜底）——事件周精确命中；未命中在窗口内取
//!   最近可用周；窗口内无任何点返回 `None` 缺料信号，调用方按空值语义显式
//!   标注或不计入，不以零折算。
//!
//! 就近窗口是盈亏页逐腿事件周折算新增的唯一折算语义（父 spec #1843）：写路径
//! 折算守既有纪律（精确周、整周无点报错、不滑周），组合走势与资金加权收益率
//! 消费单周原语、不经窗口。等距并列（事件周前后同距各有一点）取较早一周——
//! 「按实现时点汇率」语义下，事件时点可得的汇率只有已过去的那一周；该理由只
//! 辖并列裁决，不辖窗口允许向更晚方向取数——窗口兜底本身是票面（#1844）语义。
//!
//! dead_code 豁免（#1849 收窄）：模块级豁免随单周原语接线（trend / as_of 消费
//! [`fx_rate_at_week`]）退役；[`nearest_week_fx_rate`] 与
//! [`FX_NEAREST_WINDOW_WEEKS`] 的生产消费方是盈亏页逐腿事件周折算（父 spec
//! #1843 实施票），接线前仅域单测消费，条目级豁免随该票撤去（先例：
//! market-sync sina_fund #1566 同款豁免）。

use std::collections::HashMap;

use chrono::NaiveDate;

/// 就近兜底窗口宽度（周，域内单点命名常量，issue #1844）：事件周自身（距离
/// 0）加前后各 8 周为候选；窗口内无任何点即缺料。
///
/// dead_code 豁免见模块头注（#1849 收窄）。
#[allow(dead_code)]
pub(crate) const FX_NEAREST_WINDOW_WEEKS: i64 = 8;

/// 汇率历史周键索引：(base, quote) → week_start → rate——读路径装载
/// `fx_rate_history` 的共享形态（组合走势 / 边界市值同款装载）。
pub(crate) type FxWeekHistory = HashMap<(String, String), HashMap<String, f64>>;

/// 按事件日期就近取汇率：事件周精确命中，未命中在 ±8 周窗口内取最近可用周
/// （等距取较早一周）；周内正查失败反查取倒数；同币种恒等；窗口内无任何点
/// 返回 `None` 缺料信号。
///
/// dead_code 豁免见模块头注（#1849 收窄）。
#[allow(dead_code)]
pub(crate) fn nearest_week_fx_rate(
    fx: &FxWeekHistory,
    base: &str,
    quote: &str,
    event_date: NaiveDate,
) -> Option<f64> {
    let event_week = crate::constant_price::week_monday(event_date);
    // 候选周按距离升序扫描，首个有点的周即「最近可用周」；同距候选先早后
    // 晚——等距并列取较早一周（模块头注纪律）。距离 0 即事件周精确命中。
    for distance in 0..=FX_NEAREST_WINDOW_WEEKS {
        let same_distance = if distance == 0 {
            vec![event_week]
        } else {
            vec![
                event_week - chrono::Duration::weeks(distance),
                event_week + chrono::Duration::weeks(distance),
            ]
        };
        for candidate in same_distance {
            let week_start = candidate.format("%Y-%m-%d").to_string();
            if let Some(rate) = fx_rate_at_week(fx, base, quote, &week_start) {
                return Some(rate);
            }
        }
    }
    None
}

/// 单周取汇率（同币种恒等 / 正查 / 反查取倒数）——单周正反向取数的域内单点
/// （issue #1849 收口），组合走势 / 边界市值同期折算与就近窗口共用；该周正反
/// 向均无点返回 `None`。
pub(crate) fn fx_rate_at_week(
    fx: &FxWeekHistory,
    base: &str,
    quote: &str,
    week_start: &str,
) -> Option<f64> {
    if base == quote {
        return Some(1.0);
    }
    if let Some(rate) = fx
        .get(&(base.to_string(), quote.to_string()))
        .and_then(|w| w.get(week_start))
    {
        return Some(*rate);
    }
    fx.get(&(quote.to_string(), base.to_string()))
        .and_then(|w| w.get(week_start))
        .map(|rev| 1.0 / rev)
}
