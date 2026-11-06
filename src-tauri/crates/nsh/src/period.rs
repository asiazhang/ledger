//! 时间维度行——期间步进与预设的纯决策逻辑（issue #2021 / ADR-0141；旧壳
//! `packages/utils/src/time-period.ts` 的 NSH 重生）。
//!
//! **纯 Rust、无 UI 依赖**（装配层同纪律）：游标（从当前区间唯一反推的自然
//! 周期）、数据期间边界（钳制输入）、步进换算与预设区间换算全部在本模块，
//! UI 层（[`crate::app`]）只消费——语义等同点击步进器/芯片是快捷键与鼠标
//! 两条触发面共用的同一出口（ADR-0141 决策 2：不为快捷键另设步进逻辑）。
//!
//! 期间语义与旧壳逐字一致：
//! - 自然周期 = 本地日历的自然月 / 自然季度（1–3、4–6、7–9、10–12）/ 自然年，
//!   输出 YYYY-MM-DD 含边界区间（与域列表查询的 date 字典序比较一致，
//!   `TransactionListFilter::from/to`）；
//! - 「全部」与任意区间无游标 → 步进幂等无操作；自然月/季/年跨度互不相同，
//!   反推唯一；
//! - 数据期间边界（旧壳 report_date_range / [`crate::boot::report_date_range`]）：
//!   `[min(最早交易期间, 当前期间), max(最新交易期间, 当前期间)]`——当前期间
//!   恒可选（全未来数据从当前期间开始），空库回退单当前期间；边界未知（在途/
//!   失败）退化为不钳制（不阻塞步进）；
//! - 「今天」由调用方注入（[`chrono::NaiveDate`]，本地时区），本模块不持时钟。
//!
//! 决策半边的权威单测住本模块（ADR-0087：期间语义归本壳装配/决策层单测）；
//! 快捷键接线（派发面）归 headless 组件测试。

use chrono::{Datelike, NaiveDate};

/// 期间单位闭集（与旧壳 PeriodUnit 一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodUnit {
    /// 自然月。
    Month,
    /// 自然季度。
    Quarter,
    /// 自然年。
    Year,
}

/// 自然周期（步进游标中间态，不落状态）：`index` 对 Month 为 0 起月份、
/// Quarter 为 0 起季度、Year 恒 0。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NaturalPeriod {
    /// 单位。
    pub unit: PeriodUnit,
    /// 年。
    pub year: i32,
    /// 单位内序（0 起）。
    pub index: u32,
}

/// 时间维度预设闭集（芯片渲染顺序即数组顺序，与旧壳一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// 全部（无日期过滤 = 默认态哨兵）。
    All,
    /// 当月。
    Month,
    /// 当季。
    Quarter,
    /// 当年。
    Year,
    /// 去年。
    LastYear,
}

/// 预设全序（弹层芯片按此渲染，与旧壳 TIME_PERIOD_PRESETS 同序）。
pub const PRESETS: &[Preset] = &[
    Preset::All,
    Preset::Month,
    Preset::Quarter,
    Preset::Year,
    Preset::LastYear,
];

/// 预设 zh-CN 文案（与旧壳 quickTimeRange.period.* 同词表；文案仅 zh-CN，
/// spec #2015 实施决策）。
pub fn preset_label(preset: Preset) -> &'static str {
    match preset {
        Preset::All => "全部",
        Preset::Month => "当月",
        Preset::Quarter => "当季",
        Preset::Year => "当年",
        Preset::LastYear => "去年",
    }
}

/// 含边界日期区间（YYYY-MM-DD，双端包含）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeriodRange {
    /// 起始日期（含）。
    pub from: String,
    /// 结束日期（含）。
    pub to: String,
}

/// 数据期间边界（旧壳 PeriodBoundary）：某单位下的闭区间
/// [最早期间, 最晚期间]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeriodBoundary {
    /// 最早可选期间。
    pub earliest: NaturalPeriod,
    /// 最晚可选期间。
    pub latest: NaturalPeriod,
}

/// YYYY-MM-DD 解析（非法格式 / 不存在日期返回 None）。
fn parse_iso_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// YYYY-MM-DD 格式化。
fn format_iso_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// 某年某月（0 起月份）的首日。
fn month_first(year: i32, m0: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(year, m0 + 1, 1)
}

/// 某年某月（0 起月份）的末日（经「次月一日的前一天」滚动，自动处理闰年）。
fn month_last(year: i32, m0: u32) -> Option<NaiveDate> {
    let (next_year, next_m0) = if m0 == 11 {
        (year + 1, 0)
    } else {
        (year, m0 + 1)
    };
    month_first(next_year, next_m0)?.pred_opt()
}

/// 自然月区间（`m0` 为 0 起月份）。
fn month_period_range(year: i32, m0: u32) -> Option<PeriodRange> {
    Some(PeriodRange {
        from: format_iso_date(month_first(year, m0)?),
        to: format_iso_date(month_last(year, m0)?),
    })
}

/// 自然季度区间（`q` 为 0 起季度：1–3、4–6、7–9、10–12）：起于季度首月 1 日、
/// 止于季度末月最后一日。
fn quarter_period_range(year: i32, q: u32) -> Option<PeriodRange> {
    Some(PeriodRange {
        from: format_iso_date(month_first(year, q * 3)?),
        to: format_iso_date(month_last(year, q * 3 + 2)?),
    })
}

/// 自然年区间：起于 1 月 1 日、止于 12 月 31 日。
fn year_period_range(year: i32) -> Option<PeriodRange> {
    Some(PeriodRange {
        from: format_iso_date(month_first(year, 0)?),
        to: format_iso_date(month_last(year, 11)?),
    })
}

/// 期间在时间轴上的单调整数序（跨年/年内大小比较与边界判定）。
fn period_value(p: &NaturalPeriod) -> i64 {
    let per_year = match p.unit {
        PeriodUnit::Month => 12,
        PeriodUnit::Quarter => 4,
        PeriodUnit::Year => 1,
    };
    p.year as i64 * per_year + p.index as i64
}

/// 数据期间边界派生单点（旧壳 derivePeriodBoundary）：输入日期对
/// （min_date / max_date，可空）与「今天」，输出指定单位下的闭区间
/// `[min(最早交易期间, 当前期间), max(最新交易期间, 当前期间)]`。
/// 空库或非法日期回退为单「当前期间」；「与今天更晚者」的抬升在此发生。
pub fn derive_boundary(
    unit: PeriodUnit,
    min_date: Option<&str>,
    max_date: Option<&str>,
    today: NaiveDate,
) -> PeriodBoundary {
    let current = period_from_date(unit, today);
    let (Some(min), Some(max)) = (min_date, max_date) else {
        return PeriodBoundary {
            earliest: current,
            latest: current,
        };
    };
    let (Some(min_d), Some(max_d)) = (parse_iso_date(min), parse_iso_date(max)) else {
        return PeriodBoundary {
            earliest: current,
            latest: current,
        };
    };
    let earliest_from_data = period_from_date(unit, min_d);
    // 当前期间必须始终可选：所有交易都在未来时，从当前期间而不是未来的最早
    // 交易期间开始（边界为连续且包含当前期间的闭区间）。
    let earliest = if period_value(&earliest_from_data) < period_value(&current) {
        earliest_from_data
    } else {
        current
    };
    let max_tx_period = period_from_date(unit, max_d);
    let latest = if period_value(&max_tx_period) > period_value(&current) {
        max_tx_period
    } else {
        current
    };
    PeriodBoundary { earliest, latest }
}

/// 本地日期按单位反推自然期间（边界派生与预设共用）。
fn period_from_date(unit: PeriodUnit, d: NaiveDate) -> NaturalPeriod {
    let index = match unit {
        PeriodUnit::Month => d.month0(),
        PeriodUnit::Quarter => d.month0() / 3,
        PeriodUnit::Year => 0,
    };
    NaturalPeriod {
        unit,
        year: d.year(),
        index,
    }
}

/// 判定期间是否落在数据期间边界内（双端包含）。
pub fn period_within_boundary(p: &NaturalPeriod, boundary: &PeriodBoundary) -> bool {
    let val = period_value(p);
    val >= period_value(&boundary.earliest) && val <= period_value(&boundary.latest)
}

/// 期间 ±delta 步进（视图只用 ±1）：月/季在年界自然回绕（12 月 → 次年 1 月、
/// 四季度 → 次年一季度），年直接 ±1；本函数不做边界判定，钳制由
/// [`step_current`] 在调用侧承担（旧壳 #391 修订 #383「不钳制未来」同形）。
pub fn step_period(p: &NaturalPeriod, delta: i32) -> NaturalPeriod {
    match p.unit {
        PeriodUnit::Year => NaturalPeriod {
            unit: p.unit,
            year: p.year + delta,
            index: 0,
        },
        PeriodUnit::Month => {
            let per_year: i64 = 12;
            let total = p.year as i64 * per_year + p.index as i64 + delta as i64;
            NaturalPeriod {
                unit: p.unit,
                year: total.div_euclid(per_year) as i32,
                index: total.rem_euclid(per_year) as u32,
            }
        }
        PeriodUnit::Quarter => {
            let per_year: i64 = 4;
            let total = p.year as i64 * per_year + p.index as i64 + delta as i64;
            NaturalPeriod {
                unit: p.unit,
                year: total.div_euclid(per_year) as i32,
                index: total.rem_euclid(per_year) as u32,
            }
        }
    }
}

/// 期间 → 含边界日期区间（与预设区间换算共用同一自然周期换算单点）。
pub fn period_range(p: &NaturalPeriod) -> Option<PeriodRange> {
    match p.unit {
        PeriodUnit::Month => month_period_range(p.year, p.index),
        PeriodUnit::Quarter => quarter_period_range(p.year, p.index),
        PeriodUnit::Year => year_period_range(p.year),
    }
}

/// 含边界日期区间 → 唯一（单位，期间）：区间恰为某自然月/季/年时返回对应
/// 期间。任一端缺失（「全部」= 默认态）、任意区间与非法日期一律 None
/// （无可步进游标）。自然月/季/年跨度互不相同，反推唯一。
pub fn range_to_period(from: Option<&str>, to: Option<&str>) -> Option<NaturalPeriod> {
    let (Some(from), Some(to)) = (from, to) else {
        return None;
    };
    let f = parse_iso_date(from)?;
    let end = parse_iso_date(to)?;
    // 年：1 月 1 日 → 同年 12 月 31 日。
    if f.month0() == 0
        && f.day() == 1
        && end.year() == f.year()
        && end.month0() == 11
        && end.day() == 31
    {
        return Some(NaturalPeriod {
            unit: PeriodUnit::Year,
            year: f.year(),
            index: 0,
        });
    }
    // 季：同年同季度，起于季度首月 1 日、止于季度末月最后一日（跨季度区间不命中）。
    let fq = f.month0() / 3;
    if end.year() == f.year()
        && f.month0() == fq * 3
        && end.month0() == fq * 3 + 2
        && f.day() == 1
        && Some(end.day()) == month_last(end.year(), end.month0()).map(|d| d.day())
    {
        return Some(NaturalPeriod {
            unit: PeriodUnit::Quarter,
            year: f.year(),
            index: fq,
        });
    }
    // 月：同年同月，起于 1 日、止于月末（自动兼容闰年 2 月）。
    if f.year() == end.year()
        && f.month0() == end.month0()
        && f.day() == 1
        && Some(end.day()) == month_last(end.year(), end.month0()).map(|d| d.day())
    {
        return Some(NaturalPeriod {
            unit: PeriodUnit::Month,
            year: f.year(),
            index: f.month0(),
        });
    }
    None
}

/// 预设 → 相对「今天」的含边界日期区间（本地自然周期；「全部」= 无区间 None，
/// 「去年」= 当前年减一的完整自然年）。
pub fn preset_range(preset: Preset, today: NaiveDate) -> Option<PeriodRange> {
    match preset {
        Preset::All => None,
        Preset::Month => month_period_range(today.year(), today.month0()),
        Preset::Quarter => quarter_period_range(today.year(), today.month0() / 3),
        Preset::Year => year_period_range(today.year()),
        Preset::LastYear => year_period_range(today.year() - 1),
    }
}

/// 高亮派生（旧壳 matchPreset）：当前日期区间恰等于某预设定义（相对今天）时
/// 返回该预设；双端皆空 = 「全部」；单端过滤、任意区间与历史周期区间 None。
pub fn match_preset(from: Option<&str>, to: Option<&str>, today: NaiveDate) -> Option<Preset> {
    if from.is_none() && to.is_none() {
        return Some(Preset::All);
    }
    let candidates = [
        (Preset::Month, preset_range(Preset::Month, today)),
        (Preset::Quarter, preset_range(Preset::Quarter, today)),
        (Preset::Year, preset_range(Preset::Year, today)),
        (Preset::LastYear, preset_range(Preset::LastYear, today)),
    ];
    candidates
        .into_iter()
        .find(|(_, range)| {
            range
                .as_ref()
                .is_some_and(|r| Some(r.from.as_str()) == from && Some(r.to.as_str()) == to)
        })
        .map(|(preset, _)| preset)
}

/// 步进决策单点（旧壳 onStepPeriod；快捷键与点击步进器共用）：从当前区间反推
/// 游标步进 ±delta，越出数据期间边界或无游标（「全部」/任意区间）返回 None
/// （幂等无操作）；否则返回写回的新快照区间。
pub fn step_current(
    from: Option<&str>,
    to: Option<&str>,
    delta: i32,
    boundary: Option<&PeriodBoundary>,
) -> Option<PeriodRange> {
    let cursor = range_to_period(from, to)?;
    let next = step_period(&cursor, delta);
    if let Some(boundary) = boundary
        && !period_within_boundary(&next, boundary)
    {
        return None;
    }
    period_range(&next)
}

/// 步进可达性（旧壳 canStepPrev/canStepNext；步进按钮置灰依据）：无游标或
/// 越界为 false；边界未知（None）退化为不钳制（恒可步）。
pub fn can_step_current(
    from: Option<&str>,
    to: Option<&str>,
    delta: i32,
    boundary: Option<&PeriodBoundary>,
) -> bool {
    step_current(from, to, delta, boundary).is_some()
}

/// 期间标签 zh-CN（旧壳 formatPeriodLabel 同词表）：
/// 「2026年2月」「2026年二季度」「2025年」。
pub fn format_period_label(p: &NaturalPeriod) -> String {
    match p.unit {
        PeriodUnit::Year => format!("{}年", p.year),
        PeriodUnit::Month => format!("{}年{}月", p.year, p.index + 1),
        PeriodUnit::Quarter => {
            let name = ["一", "二", "三", "四"][p.index as usize % 4];
            format!("{}年{name}季度", p.year)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试锚定的「今天」（ assertions 全部相对它，不受真实时钟影响）。
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 5).unwrap()
    }

    fn date(s: &str) -> Option<NaiveDate> {
        parse_iso_date(s)
    }

    fn month(y: i32, m0: u32) -> NaturalPeriod {
        NaturalPeriod {
            unit: PeriodUnit::Month,
            year: y,
            index: m0,
        }
    }

    fn quarter(y: i32, q0: u32) -> NaturalPeriod {
        NaturalPeriod {
            unit: PeriodUnit::Quarter,
            year: y,
            index: q0,
        }
    }

    fn year(y: i32) -> NaturalPeriod {
        NaturalPeriod {
            unit: PeriodUnit::Year,
            year: y,
            index: 0,
        }
    }

    #[test]
    fn preset_labels_match_the_old_shell_zh_copy() {
        assert_eq!(
            PRESETS,
            [
                Preset::All,
                Preset::Month,
                Preset::Quarter,
                Preset::Year,
                Preset::LastYear
            ]
            .as_slice(),
            "预设闭集与顺序同旧壳"
        );
        let labels: Vec<_> = PRESETS.iter().map(|p| preset_label(*p)).collect();
        assert_eq!(labels, ["全部", "当月", "当季", "当年", "去年"]);
    }

    #[test]
    fn preset_ranges_follow_natural_calendars() {
        let t = today();
        assert_eq!(
            preset_range(Preset::Month, t),
            Some(PeriodRange {
                from: "2026-11-01".into(),
                to: "2026-11-30".into()
            }),
            "当月 = 自然月含边界"
        );
        assert_eq!(
            preset_range(Preset::Quarter, t),
            Some(PeriodRange {
                from: "2026-10-01".into(),
                to: "2026-12-31".into()
            }),
            "当季 = 自然季度含边界"
        );
        assert_eq!(
            preset_range(Preset::Year, t),
            Some(PeriodRange {
                from: "2026-01-01".into(),
                to: "2026-12-31".into()
            }),
        );
        assert_eq!(
            preset_range(Preset::LastYear, t),
            Some(PeriodRange {
                from: "2025-01-01".into(),
                to: "2025-12-31".into()
            }),
        );
        assert_eq!(preset_range(Preset::All, t), None, "「全部」无区间");
        // 闰年 2 月：末日 29 日。
        let leap = NaiveDate::from_ymd_opt(2028, 2, 10).unwrap();
        assert_eq!(
            preset_range(Preset::Month, leap),
            Some(PeriodRange {
                from: "2028-02-01".into(),
                to: "2028-02-29".into()
            }),
            "闰年自然月末日为 29 日"
        );
    }

    #[test]
    fn range_to_period_recovers_exact_natural_periods_only() {
        assert_eq!(
            range_to_period(Some("2026-02-01"), Some("2026-02-28")),
            Some(month(2026, 1)),
            "平年 2 月自然月反推"
        );
        assert_eq!(
            range_to_period(Some("2028-02-01"), Some("2028-02-29")),
            Some(month(2028, 1)),
            "闰年 2 月自然月反推"
        );
        assert_eq!(
            range_to_period(Some("2026-04-01"), Some("2026-06-30")),
            Some(quarter(2026, 1)),
            "自然季度反推"
        );
        assert_eq!(
            range_to_period(Some("2025-01-01"), Some("2025-12-31")),
            Some(year(2025)),
            "自然年反推"
        );
        // 无游标形态：全部 / 单端 / 任意区间 / 非法日期。
        assert_eq!(range_to_period(None, None), None, "「全部」无游标");
        assert_eq!(
            range_to_period(Some("2026-02-01"), None),
            None,
            "单端无游标"
        );
        assert_eq!(
            range_to_period(Some("2026-02-01"), Some("2026-03-15")),
            None,
            "任意区间无游标"
        );
        assert_eq!(range_to_period(Some("不是日期"), Some("2026-03-31")), None);
    }

    #[test]
    fn step_period_wraps_naturally_across_years() {
        assert_eq!(
            step_period(&month(2026, 11), 1),
            month(2026 + 1, 0),
            "12 月 → 次年 1 月"
        );
        assert_eq!(
            step_period(&month(2026, 0), -1),
            month(2025, 11),
            "1 月 → 去年 12 月"
        );
        assert_eq!(
            step_period(&quarter(2026, 3), 1),
            quarter(2027, 0),
            "四季度 → 次年一季度"
        );
        assert_eq!(step_period(&quarter(2026, 0), -1), quarter(2025, 3));
        assert_eq!(step_period(&year(2026), 1), year(2027));
        assert_eq!(step_period(&year(2026), -1), year(2025));
        assert_eq!(
            step_period(&month(2026, 4), 0),
            month(2026, 4),
            "零步进原样"
        );
    }

    #[test]
    fn derive_boundary_keeps_current_period_always_available() {
        // 历史数据：边界 = [最早交易期间, max(最新交易期间, 当前期间)]。
        let b = derive_boundary(
            PeriodUnit::Month,
            Some("2024-03-09"),
            Some("2026-01-20"),
            today(),
        );
        assert_eq!(b.earliest, month(2024, 2), "最早交易期间");
        assert_eq!(b.latest, month(2026, 10), "当前期间抬升（数据止于 1 月）");
        // 全未来数据：最早端抬升为当前期间。
        let b = derive_boundary(
            PeriodUnit::Month,
            Some("2030-06-01"),
            Some("2030-08-15"),
            today(),
        );
        assert_eq!(b.earliest, month(2026, 10), "全未来数据从当前期间开始");
        assert_eq!(b.latest, month(2030, 7));
        // 空库 / 非法日期：单当前期间。
        let empty = derive_boundary(PeriodUnit::Month, None, None, today());
        assert_eq!(empty.earliest, month(2026, 10));
        assert_eq!(empty.latest, month(2026, 10));
        let bad = derive_boundary(PeriodUnit::Month, Some("垃圾"), Some("也是垃圾"), today());
        assert_eq!(bad, empty, "非法日期回退单当前期间");
        // 季 / 年单位。
        let q = derive_boundary(
            PeriodUnit::Quarter,
            Some("2025-08-01"),
            Some("2026-03-01"),
            today(),
        );
        assert_eq!(q.earliest, quarter(2025, 2));
        assert_eq!(q.latest, quarter(2026, 3), "当前季度（四季度）抬升");
        let y = derive_boundary(
            PeriodUnit::Year,
            Some("2024-02-01"),
            Some("2026-02-01"),
            today(),
        );
        assert_eq!(y.earliest, year(2024));
        assert_eq!(y.latest, year(2026));
    }

    #[test]
    fn step_current_clamps_at_boundary_and_noops_without_cursor() {
        let t = today();
        let b = derive_boundary(PeriodUnit::Month, Some("2026-10-04"), Some("2026-11-08"), t);
        // 当月（2026-11）向后步进：越出上界（当前期间即最晚）→ 幂等无操作。
        assert_eq!(
            step_current(Some("2026-11-01"), Some("2026-11-30"), 1, Some(&b)),
            None
        );
        // 向前步进：到 2026-10（边界内）。
        assert_eq!(
            step_current(Some("2026-11-01"), Some("2026-11-30"), -1, Some(&b)),
            Some(PeriodRange {
                from: "2026-10-01".into(),
                to: "2026-10-31".into()
            }),
        );
        // 再向前：越出下界（2026-10 即最早）→ 幂等无操作。
        assert_eq!(
            step_current(Some("2026-10-01"), Some("2026-10-31"), -1, Some(&b)),
            None
        );
        // 「全部」与任意区间：无游标 → 无操作。
        assert_eq!(step_current(None, None, -1, Some(&b)), None);
        assert_eq!(
            step_current(Some("2026-01-05"), Some("2026-03-20"), -1, Some(&b)),
            None
        );
        // 边界未知（None）：退化为不钳制。
        assert_eq!(
            step_current(Some("2026-11-01"), Some("2026-11-30"), 1, None),
            Some(PeriodRange {
                from: "2026-12-01".into(),
                to: "2026-12-31".into()
            }),
        );
        // 步进写回仍是精确自然周期（可继续反推）：宽边界下连步 3 个月不触界。
        let wide = derive_boundary(PeriodUnit::Month, Some("2025-01-15"), Some("2026-11-08"), t);
        let stepped =
            step_current(Some("2026-11-01"), Some("2026-11-30"), -3, Some(&wide)).unwrap();
        assert_eq!(
            range_to_period(Some(&stepped.from), Some(&stepped.to)),
            Some(month(2026, 7)),
            "连步 3 个月写回快照仍可反推"
        );
        let _ = date("2026-11-05");
    }

    #[test]
    fn can_step_current_mirrors_step_availability() {
        let t = today();
        let b = derive_boundary(PeriodUnit::Month, Some("2026-10-04"), Some("2026-11-08"), t);
        assert!(can_step_current(
            Some("2026-11-01"),
            Some("2026-11-30"),
            -1,
            Some(&b)
        ));
        assert!(!can_step_current(
            Some("2026-11-01"),
            Some("2026-11-30"),
            1,
            Some(&b)
        ));
        assert!(
            !can_step_current(None, None, -1, Some(&b)),
            "无游标不可步进"
        );
        assert!(
            !can_step_current(None, None, -1, None),
            "无游标恒不可步进（旧壳 canStepPrev 同形：p !== null 先决）"
        );
    }

    #[test]
    fn period_labels_match_the_old_shell_zh_copy() {
        assert_eq!(format_period_label(&month(2026, 1)), "2026年2月");
        assert_eq!(format_period_label(&month(2025, 11)), "2025年12月");
        assert_eq!(format_period_label(&quarter(2026, 0)), "2026年一季度");
        assert_eq!(format_period_label(&quarter(2026, 3)), "2026年四季度");
        assert_eq!(format_period_label(&year(2025)), "2025年");
    }

    #[test]
    fn match_preset_lights_the_exact_preset_only() {
        let t = today();
        assert_eq!(match_preset(None, None, t), Some(Preset::All));
        assert_eq!(
            match_preset(Some("2026-11-01"), Some("2026-11-30"), t),
            Some(Preset::Month)
        );
        assert_eq!(
            match_preset(Some("2026-10-01"), Some("2026-12-31"), t),
            Some(Preset::Quarter)
        );
        assert_eq!(
            match_preset(Some("2026-01-01"), Some("2026-12-31"), t),
            Some(Preset::Year)
        );
        assert_eq!(
            match_preset(Some("2025-01-01"), Some("2025-12-31"), t),
            Some(Preset::LastYear)
        );
        assert_eq!(
            match_preset(Some("2026-05-01"), Some("2026-05-31"), t),
            None,
            "历史月不点亮"
        );
        assert_eq!(
            match_preset(Some("2026-11-01"), None, t),
            None,
            "单端不点亮"
        );
    }
}
