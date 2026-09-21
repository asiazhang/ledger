//! 写基准共享脚手架（issue #1650 收口）：CLI 参数迭代、档位 CSV 解析、
//! 计时统计与人读报告表列。
//!
//! bench-import / bench-sync / bench-market 三个写基准自 #532 / #1628 / #1629
//! 起刻意「形态同构、不另起炉灶」，同构在四件处各积累到 3–5 份：手写参数
//! 解析循环、档位 CSV 解析（`parse_rows_csv` / `parse_ops_csv` /
//! `parse_points_csv` 仅改名）、run_cell 统计块（排序 / min / avg / 最近秩
//! p95）与 print_report 排版（CJK 显示宽手排 + 同款表头，固定 pad 宽在长
//! 名称下 saturate 归零、表头标签与数值列错位）。本模块把四件收口为单一
//! 实现：三个写基准与本模块（连同读基准 bench 的统计块与两个原语）一律
//! 消费这里，不再各自持有一份。
//!
//! 档位上界（issue #1650）：`--rows` / `--ops` / `--points` 原先只有非零
//! 下限、无上限（开发工具信任操作者，与既有件同款）；极端档位下
//! bench-market 的采样日运算（锚点周一 + i×7 天）会触 chrono 日期越界
//! panic（实际先撞内存分配，纯理论路径）。上界在解析层统一收口：超过
//! [`MAX_TIER`] 即拒绝。

use std::time::Duration;

/// 档位上界（`--rows` / `--ops` / `--points` 共用，issue #1650）。
///
/// 取 1_000_000：远超任何真实量级（默认最大档 200 行 / 2000 op / 5200 点；
/// 生成库交易总量本身 50 万笔），挡住手滑多敲数字的极端档位；同时保证
/// bench-market 的采样日运算安全——上界 × 7 天 ≈ 一万九千年，远离 chrono
/// `NaiveDate` 极值（约 ±26 万年）、`i64` 乘法亦无溢出。刻意压测（如全库
/// 量级 op 重放）仍可在界内进行，不被误伤。
pub(crate) const MAX_TIER: usize = 1_000_000;

/// 单个 CLI flag 的迭代产物：flag 本体与 inline 值（`--flag=v` 形态）。
pub(crate) struct CliFlag<'a> {
    pub flag: &'a str,
    pub inline_value: Option<&'a str>,
}

/// CLI 参数逐词迭代（解析循环的同构收口，issue #1650）：支持
/// `--flag value` 与 `--flag=value` 两种取值形态，`--db=path/to=x` 按
/// 首个 `=` 切分（值可含 `=`）。各子命令解析器保留自己的 flag 表，
/// 循环机制与取值/缺值报错收口在这里。
pub(crate) struct CliArgs<'a> {
    args: &'a [String],
    pos: usize,
}

impl<'a> CliArgs<'a> {
    pub(crate) fn new(args: &'a [String]) -> Self {
        CliArgs { args, pos: 0 }
    }

    /// 下一个 flag（含 inline 值，如有）：遍历结束返回 None。
    pub(crate) fn next_flag(&mut self) -> Option<CliFlag<'a>> {
        let arg = self.args.get(self.pos)?;
        self.pos += 1;
        let (flag, inline_value) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (arg.as_str(), None),
        };
        Some(CliFlag { flag, inline_value })
    }

    /// 当前 flag 的值：inline（`--flag=v`）优先，否则吃下一个词；缺值报错
    /// （错误消息以 flag 名开头，与既有各解析器同款）。flag 名取自传入的
    /// [`CliFlag`]，调用方不必手工回传。
    pub(crate) fn value(&mut self, f: CliFlag<'a>) -> Result<String, String> {
        if let Some(v) = f.inline_value {
            return Ok(v.to_string());
        }
        let next = self
            .args
            .get(self.pos)
            .ok_or_else(|| format!("{} 缺少值", f.flag))?;
        self.pos += 1;
        Ok(next.clone())
    }
}

/// 档位 CSV 解析（`--rows` / `--ops` / `--points` 共用收口，issue #1650）：
/// 非零、互不重复、不超过 [`MAX_TIER`]、保持给定次序（矩阵展开次序即报告
/// 次序）。错误消息带 flag 名，与既有各解析器同款。
pub(crate) fn parse_tier_csv(csv: &str, flag: &str) -> Result<Vec<usize>, String> {
    let mut tiers = Vec::new();
    for part in csv.split(',') {
        let part = part.trim();
        let n = part
            .parse::<usize>()
            .map_err(|_| format!("{flag} 档位需要非负整数，得到 {part:?}"))?;
        if n == 0 {
            return Err(format!("{flag} 档位必须大于 0"));
        }
        if n > MAX_TIER {
            return Err(format!("{flag} 档位超过上限 {MAX_TIER}：{n}"));
        }
        if tiers.contains(&n) {
            return Err(format!("{flag} 档位重复：{n}"));
        }
        tiers.push(n);
    }
    if tiers.is_empty() {
        return Err(format!("{flag} 至少需要一个档位"));
    }
    Ok(tiers)
}

/// 最近秩法 p95：升序样本取第 ⌈0.95·n⌉ 个（n=10 时恒等于 max，
/// n≥20 才有真分位数分辨力）。读基准与写基准共用（ADR-0068 统计口径）。
pub(crate) fn percentile_ms(sorted_ms: &[f64], p: f64) -> f64 {
    let n = sorted_ms.len();
    let rank = (p * n as f64).ceil().max(1.0) as usize;
    sorted_ms[rank.min(n) - 1]
}

/// 字符串终端显示宽估算：ASCII 记 1、其余（CJK 等）记 2。人读报告表列
/// 共用（读基准与 bench-import / bench-sync / bench-market 的 print_report）。
pub(crate) fn display_width(s: &str) -> usize {
    s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

/// 计时样本统计（毫秒）：排序 → min / 算术平均 / 最近秩 p95
/// （[`percentile_ms`]，ADR-0068 口径）。run_cell 统计块的同构收口
/// （issue #1650）。
pub(crate) struct DurationStats {
    pub min_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
}

/// 统计入口：吃一次迭代的计时样本（调用方保证非空——各入口解析层已拒绝
/// `--iterations 0`），排序后取三项统计量。
pub(crate) fn summarize(mut durations: Vec<Duration>) -> DurationStats {
    durations.sort();
    let ms: Vec<f64> = durations.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    DurationStats {
        min_ms: ms[0],
        avg_ms: ms.iter().sum::<f64>() / ms.len() as f64,
        p95_ms: percentile_ms(&ms, 0.95),
    }
}

/// 人读报告表行（写基准 min / avg / p95 + 规模备注的同构列形收口，
/// issue #1650）：各基准指标结构体实现本 trait 后交
/// [`print_metric_table`] 打印。
pub(crate) trait MetricTableRow {
    fn name(&self) -> &str;
    fn min_ms(&self) -> f64;
    fn avg_ms(&self) -> f64;
    fn p95_ms(&self) -> f64;
    fn context(&self) -> &str;
}

/// 表头名称列标签（显示宽 4，宽度下限）。
const NAME_HEADER: &str = "基准";

/// 人读表头行（名称列按 name_width 取宽，min / avg / p95 标签右对齐在各自
/// 数值列上方，单位毫秒入表头）：[`print_metric_table`] 与读基准
/// print_report（带 ▲ 超阈值标记列，行渲染自持）共用。
pub(crate) fn metric_table_header(name_width: usize) -> String {
    let header_pad = " ".repeat(name_width - display_width(NAME_HEADER));
    format!("{NAME_HEADER}{header_pad}       min        avg        p95  规模备注（毫秒）")
}

/// 名称列宽 = 最长行的显示宽与表头标签显示宽取大（issue #1650）：固定
/// pad 宽度在长名称下 saturate 归零、数字列错位，列宽随内容动态取后
/// 任意名称长度都对齐。
pub(crate) fn name_column_width<'a>(names: impl IntoIterator<Item = &'a str>) -> usize {
    names
        .into_iter()
        .map(display_width)
        .max()
        .unwrap_or(0)
        .max(display_width(NAME_HEADER))
}

/// 人读表格输出（写基准同款列形）：名称列按结果显示宽动态取宽，min /
/// avg / p95 数字列右对齐 10/11 位，表头标签右对齐在各自数值列上方，
/// 单位毫秒入表头。
pub(crate) fn print_metric_table<M: MetricTableRow>(rows: &[M]) {
    let width = name_column_width(rows.iter().map(MetricTableRow::name));
    println!("{}", metric_table_header(width));
    for r in rows {
        let pad = " ".repeat(width - display_width(r.name()));
        println!(
            "{name}{pad}{min:>10.2}{avg:>11.2}{p95:>11.2}  {ctx}",
            name = r.name(),
            pad = pad,
            min = r.min_ms(),
            avg = r.avg_ms(),
            p95 = r.p95_ms(),
            ctx = r.context(),
        );
    }
}
