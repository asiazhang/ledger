//! 写基准共享脚手架（issue #1650 收口）：CLI 参数迭代、档位 CSV 解析、
//! 计时统计与人读报告表列；CLI 表驱动收口（issue #1696）：flag 表消费的
//! 通用解析循环、泛型 `Parsed<T>`、种类解析 helper、dispatch 结局与全文帮助
//! 渲染器（flag 表为用法唯一真源）。
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

// ---------------------------------------------------------------------------
// CLI 表驱动（issue #1696）：flag 表 + 通用解析循环 + 帮助渲染，五子命令同构
// ---------------------------------------------------------------------------

/// 参数解析结果：运行参数或帮助请求——泛型单壳，替换五个子命令各自约三行的
/// Run/Help 枚举壳（浅壳按删除测试收编，issue #1696）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Parsed<C> {
    Run(C),
    Help,
}

/// flag 表条目 = flag 显示形态 + 帮助文案 + apply（非捕获闭包内联表内，调
/// 种类 helper）。不建值种类闭集枚举：种类小且文案带 flag 特有措辞，正确性
/// 由逐字错误文案测试钉住（issue #1679 grilling 裁决）。
pub(crate) struct FlagSpec<C> {
    /// flag 显示形态（含值占位符，如 `--db <PATH>`）：帮助列直接用它；解析键
    /// 取首词（[`Self::key`]）。
    pub flag: &'static str,
    /// 帮助文案（单行逻辑文本；渲染器按统一规则折行，措辞逐字由全文 golden
    /// 钉住）。
    pub help: &'static str,
    /// 把一个值应用到解析目标：`(cli, flag 名, 值)`。
    pub apply: fn(&mut C, &str, &str) -> Result<(), String>,
}

impl<C> FlagSpec<C> {
    /// 解析键：flag 显示形态的首词（`--db <PATH>` → `--db`）。
    pub(crate) fn key(&self) -> &str {
        self.flag.split(' ').next().unwrap_or(self.flag)
    }
}

/// flag 表的帮助投影：帮助渲染与 dispatch 消费的无类型形态（解析走
/// [`FlagSpec`]，渲染不需要 apply 的类型参数）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FlagHelp {
    pub flag: &'static str,
    pub help: &'static str,
}

impl FlagHelp {
    /// 解析键（与 [`FlagSpec::key`] 同规则）：显示形态首词。仅测试侧消费
    /// （逐子命令矩阵测试从表取键）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn key(&self) -> &str {
        self.flag.split(' ').next().unwrap_or(self.flag)
    }
}

/// flag 表 → 帮助投影的机械投影：表本身仍是唯一真源（新增 flag 只登记表）。
pub(crate) fn flag_helps<C>(flags: &[FlagSpec<C>]) -> Vec<FlagHelp> {
    flags
        .iter()
        .map(|f| FlagHelp {
            flag: f.flag,
            help: f.help,
        })
        .collect()
}

/// 通用解析循环（消费 flag 表，issue #1696）：逐词迭代（[`CliArgs`]）；
/// `-h` / `--help` 即帮助请求；表内 flag 取值后交 apply；表外 flag 报未知参数。
/// 返回 Err(消息) 表示用法错误（文案与既有各解析器逐字一致）。
pub(crate) fn parse_flags<C: Default>(
    args: &[String],
    flags: &[FlagSpec<C>],
) -> Result<Parsed<C>, String> {
    let mut cli = C::default();
    let mut it = CliArgs::new(args);
    while let Some(f) = it.next_flag() {
        if f.flag == "-h" || f.flag == "--help" {
            return Ok(Parsed::Help);
        }
        let spec = flags
            .iter()
            .find(|s| s.key() == f.flag)
            .ok_or_else(|| format!("未知参数 {:?}", f.flag))?;
        let v = it.value(f)?;
        (spec.apply)(&mut cli, spec.key(), &v)?;
    }
    Ok(Parsed::Run(cli))
}

/// 整数参数解析（`--warmup` / `--iterations` / `--seed` / `--transactions`
/// 共用）：非负整数；错误消息以 flag 名开头（「{flag} 需要非负整数」逐字
/// 保留，issue #1696）。
pub(crate) fn parse_nonneg_int<T: std::str::FromStr>(flag: &str, v: &str) -> Result<T, String> {
    v.parse::<T>()
        .map_err(|_| format!("{flag} 需要非负整数，得到 {v:?}"))
}

/// 布尔参数解析（`--dedup` 只认 true/false，拒绝顺手 coercion 的歧义形态）。
/// 既有文案不含 flag 名（逐字保留），flag 参数仅为统一 apply 签名而收。
pub(crate) fn parse_bool_value(_flag: &str, v: &str) -> Result<bool, String> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("布尔参数需要 true/false，得到 {other:?}")),
    }
}

/// 门禁毫秒数解析（`--max-p95-ms`）：有限正数；错误消息以 flag 名开头
/// （「{flag} 需要正数（毫秒）」逐字保留）。
pub(crate) fn parse_gate_ms(flag: &str, v: &str) -> Result<f64, String> {
    v.parse::<f64>()
        .ok()
        .filter(|m| m.is_finite() && *m > 0.0)
        .ok_or_else(|| format!("{flag} 需要正数（毫秒），得到 {v:?}"))
}

/// 子命令执行结局（dispatch 表 run 入口的返回形态）：与 main 既有五臂 match
/// 的分支一一对应（成功 / 帮助 / 参数错误 exit 2 / 运行失败 exit 1），行为零
/// 变化只换形状。
#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    /// 运行成功（退出 0）。
    Ok,
    /// 帮助请求：打印全文帮助（退出 0）。
    Help,
    /// 参数错误：`参数错误：{msg}` + 全文帮助（退出 2）。
    ParamError(String),
    /// 运行失败：`{子命令} 失败：{msg}`（退出 1）。
    Failed(String),
}

/// 解析 + 运行的通用编排（各子命令 run 入口的共享体，issue #1696）：解析
/// Err → 参数错误、Help → 帮助、Run → 交 run。「至少为 1」等原位校验住各自
/// parse 入口（错误通道保持原位，行为零变化）。
pub(crate) fn execute_cli<C>(
    args: &[String],
    parse: fn(&[String]) -> Result<Parsed<C>, String>,
    run: fn(C) -> Result<(), String>,
) -> Outcome {
    match parse(args) {
        Ok(Parsed::Help) => Outcome::Help,
        Err(msg) => Outcome::ParamError(msg),
        Ok(Parsed::Run(cli)) => match run(cli) {
            Ok(()) => Outcome::Ok,
            Err(msg) => Outcome::Failed(msg),
        },
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
/// 共用（读基准与 bench-import / bench-sync / bench-market 的 print_report），
/// 帮助渲染的折行与列宽同此口径。
pub(crate) fn display_width(s: &str) -> usize {
    s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

// ---------------------------------------------------------------------------
// 帮助渲染（issue #1679：OPTIONS / SUBCOMMANDS 块从数据渲染，flag 表为用法
// 唯一真源；列对齐与折行按单一规则一次性归一，措辞逐字保留，golden 钉住）
// ---------------------------------------------------------------------------

/// 帮助条目的缩进。
const HELP_INDENT: usize = 4;
/// 字段列与文案列之间的间隔空格（对齐规则归一：既有手工 27/28 列不齐一并
/// 收成同一规则）。
const HELP_GAP: usize = 2;
/// 帮助行显示宽上限（CJK 按 2 宽计，同 [`display_width`] 口径）。
const HELP_LINE_WIDTH: usize = 80;
/// 帮助标题与 USAGE 骨架行。
const HELP_TITLE: &str = "ledger-perf —— Ledger 性能基准工具";
const HELP_USAGE_LINE: &str = "    ledger-perf <SUBCOMMAND> [OPTIONS]";
/// 每个 OPTIONS 节尾部恒有的帮助 flag 行（文案与既有各节逐字一致）。
const HELP_FLAG_ENTRY: (&str, &str) = ("-h, --help", "打印本说明");

/// 断点判定（cand = 下一行首字符下标，断在 cand 之前）：空格处可断；行首
/// 不落闭标点、行尾不留开标点、ASCII 词内不拆（`ADR-0068` / 路径等整词）。
fn can_break(chars: &[char], cand: usize) -> bool {
    let prev = chars[cand - 1];
    let next = chars[cand];
    if next == ' ' || prev == ' ' {
        return true;
    }
    const CLOSING: &[char] = &[
        '，', '。', '、', '；', '：', '！', '？', '）', '」', '』', '】', '》', '〉', '…', '’',
        '”', '%', ',', ')',
    ];
    const OPENING: &[char] = &['（', '「', '『', '【', '《', '〈', '“', '‘', '('];
    if CLOSING.contains(&next) || OPENING.contains(&prev) {
        return false;
    }
    let wordish = |c: char| c.is_ascii_alphanumeric() || "-_/.#,".contains(c);
    if wordish(prev) && wordish(next) {
        return false;
    }
    true
}

/// 显示宽贪心折行（帮助条目文案的唯一折行实现）：每行显示宽 ≤ limit，
/// 断点按 [`can_break`]；找不到合法断点时硬切（超长 ASCII 词），并保证每轮
/// 至少前进一个字符。
fn wrap_display(text: &str, limit: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut lines = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut width = 0;
        let mut end = start;
        while end < chars.len() && width + if chars[end].is_ascii() { 1 } else { 2 } <= limit {
            width += if chars[end].is_ascii() { 1 } else { 2 };
            end += 1;
        }
        if end >= chars.len() {
            lines.push(chars[start..].iter().collect());
            break;
        }
        let mut cut = None;
        let mut cand = end;
        while cand > start {
            if can_break(&chars, cand) {
                cut = Some(cand);
                break;
            }
            cand -= 1;
        }
        let mut cut = cut.unwrap_or(end);
        if cut <= start {
            // limit 过窄时至少前进一个字符，避免原地打转。
            cut = start + 1;
        }
        let mut seg: String = chars[start..cut].iter().collect();
        while seg.ends_with(' ') {
            seg.pop();
        }
        while cut < chars.len() && chars[cut] == ' ' {
            cut += 1;
        }
        lines.push(seg);
        start = cut;
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// 帮助条目渲染（SUBCOMMANDS 与 OPTIONS 两块共用的对齐 + 折行规则）：字段
/// 列按 `field_width` 对齐到文案列 `text_col`，首行与续行的文案都从
/// `text_col` 起按 [`HELP_LINE_WIDTH`] 折行。
fn render_entry_lines(field: &str, text: &str, field_width: usize, text_col: usize) -> Vec<String> {
    let field_pad = " ".repeat(field_width.saturating_sub(display_width(field)));
    let avail = HELP_LINE_WIDTH.saturating_sub(text_col).max(1);
    let mut segs = wrap_display(text, avail);
    let head = segs.remove(0);
    let mut lines = vec![format!(
        "{}{}{}{}{}",
        " ".repeat(HELP_INDENT),
        field,
        field_pad,
        " ".repeat(HELP_GAP),
        head
    )];
    let continuation = " ".repeat(text_col);
    for seg in segs {
        lines.push(format!("{continuation}{seg}"));
    }
    lines
}

/// 全文帮助渲染（返回 String，issue #1696）：标题 / USAGE 骨架 + SUBCOMMANDS
/// （简介从 dispatch 数据渲染，沿表序）+ 各子命令 OPTIONS 节（flag 表投影
/// 渲染，节序由调用方排定）。列宽取各自块内字段最大显示宽 + 间隔，一次性
/// 归一；措辞逐字来自表数据，全文形态由 tests 的 inline golden 钉住。
pub(crate) fn render_help(
    subcommands: &[(&str, &str)],
    sections: &[(&str, Vec<FlagHelp>)],
) -> String {
    let sub_width = subcommands
        .iter()
        .map(|(name, _)| display_width(name))
        .max()
        .unwrap_or(0);
    let sub_text_col = HELP_INDENT + sub_width + HELP_GAP;
    let flag_width = sections
        .iter()
        .flat_map(|(_, flags)| flags.iter().map(|f| display_width(f.flag)))
        .chain(std::iter::once(display_width(HELP_FLAG_ENTRY.0)))
        .max()
        .unwrap_or(0);
    let opt_text_col = HELP_INDENT + flag_width + HELP_GAP;

    let mut lines = vec![
        HELP_TITLE.to_string(),
        String::new(),
        "USAGE:".to_string(),
        HELP_USAGE_LINE.to_string(),
        String::new(),
        "SUBCOMMANDS:".to_string(),
    ];
    for (name, summary) in subcommands {
        lines.extend(render_entry_lines(name, summary, sub_width, sub_text_col));
    }
    for (section, flags) in sections {
        lines.push(String::new());
        lines.push(format!("{section} OPTIONS:"));
        for f in flags {
            lines.extend(render_entry_lines(f.flag, f.help, flag_width, opt_text_col));
        }
        lines.extend(render_entry_lines(
            HELP_FLAG_ENTRY.0,
            HELP_FLAG_ENTRY.1,
            flag_width,
            opt_text_col,
        ));
    }
    lines.join("\n")
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
