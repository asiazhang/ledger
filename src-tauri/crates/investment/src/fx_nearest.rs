//! 事件周就近汇率查找（±8 周窗口兜底，issue #1844，父 spec #1843）：按事件
//! 日期在汇率历史（FxRateHistory）周键序列中取汇率——事件所在周精确命中；
//! 未命中时在 ±8 周窗口（[`FX_NEAREST_WINDOW_WEEKS`]）内取最近可用周；正反向
//! 币对兜底与同币种恒等沿用既有历史折算纪律（组合走势 / 边界市值同款：反查
//! 取倒数，不用当期汇率近似历史）；窗口内无任何点返回 `None` 缺料信号，调用
//! 方按空值语义显式标注或不计入，不以零折算。
//!
//! 就近周是盈亏页逐腿事件周折算新增的唯一折算语义（父 spec #1843）：组合走
//! 势、资金加权收益率与写路径折算各守既有纪律（精确周、无窗口），本模块只
//! 服务读侧新消费面，不回灌既有读路径。等距并列（事件周前后同距各有一点）
//! 取较早一周——「按实现时点汇率」语义下，事件时点可得的汇率只有已过去的
//! 那一周；该理由只辖并列裁决，不辖窗口允许向更晚方向取数——窗口兜底本身
//! 是票面（#1844）语义。
//
// dead_code 豁免（#1844）：本模块是盈亏页逐腿事件周折算的预置接缝（父 spec
// #1843），域单测已消费，生产消费方在后续实施票接线；接线时连同本注记撤去
// （先例：market-sync sina_fund #1566 同款豁免）。
#![allow(dead_code)]

use std::collections::HashMap;

use chrono::NaiveDate;

/// 就近兜底窗口宽度（周，域内单点命名常量，issue #1844）：事件周自身（距离
/// 0）加前后各 8 周为候选；窗口内无任何点即缺料。
pub(crate) const FX_NEAREST_WINDOW_WEEKS: i64 = 8;

/// 汇率历史周键索引：(base, quote) → week_start → rate——读路径装载
/// `fx_rate_history` 的共享形态（组合走势 / 边界市值同款装载）。
pub(crate) type FxWeekHistory = HashMap<(String, String), HashMap<String, f64>>;

/// 按事件日期就近取汇率：事件周精确命中，未命中在 ±8 周窗口内取最近可用周
/// （等距取较早一周）；周内正查失败反查取倒数；同币种恒等；窗口内无任何点
/// 返回 `None` 缺料信号。
pub(crate) fn nearest_week_fx_rate(
    fx: &FxWeekHistory,
    base: &str,
    quote: &str,
    event_date: NaiveDate,
) -> Option<f64> {
    if base == quote {
        return Some(1.0);
    }
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

/// 单周取汇率（正查失败反查取倒数），与组合走势 / 边界市值同期取数同一纪律
/// 形态；该周正反向均无点返回 `None`。
fn fx_rate_at_week(fx: &FxWeekHistory, base: &str, quote: &str, week_start: &str) -> Option<f64> {
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
