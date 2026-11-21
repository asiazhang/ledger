use std::time::Duration;

pub(crate) const MAX_TIER: usize = 1_000_000;

pub(crate) struct CliFlag<'a> {
    pub flag: &'a str,
    pub inline_value: Option<&'a str>,
}

pub(crate) struct CliArgs<'a> {
    args: &'a [String],
    pos: usize,
}

impl<'a> CliArgs<'a> {
    pub(crate) fn new(args: &'a [String]) -> Self {
        CliArgs { args, pos: 0 }
    }

    pub(crate) fn next_flag(&mut self) -> Option<CliFlag<'a>> {
        let arg = self.args.get(self.pos)?;
        self.pos += 1;
        let (flag, inline_value) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (arg.as_str(), None),
        };
        Some(CliFlag { flag, inline_value })
    }

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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Parsed<C> {
    Run(C),
    Help,
}

pub(crate) struct FlagSpec<C> {
    pub flag: &'static str,
    pub help: &'static str,
    pub apply: fn(&mut C, &str, &str) -> Result<(), String>,
}

impl<C> FlagSpec<C> {
    pub(crate) fn key(&self) -> &str {
        self.flag.split(' ').next().unwrap_or(self.flag)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FlagHelp {
    pub flag: &'static str,
    pub help: &'static str,
}

impl FlagHelp {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn key(&self) -> &str {
        self.flag.split(' ').next().unwrap_or(self.flag)
    }
}

pub(crate) fn flag_helps<C>(flags: &[FlagSpec<C>]) -> Vec<FlagHelp> {
    flags
        .iter()
        .map(|f| FlagHelp {
            flag: f.flag,
            help: f.help,
        })
        .collect()
}

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

pub(crate) fn parse_nonneg_int<T: std::str::FromStr>(flag: &str, v: &str) -> Result<T, String> {
    v.parse::<T>()
        .map_err(|_| format!("{flag} 需要非负整数，得到 {v:?}"))
}

pub(crate) fn parse_bool_value(_flag: &str, v: &str) -> Result<bool, String> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("布尔参数需要 true/false，得到 {other:?}")),
    }
}

pub(crate) fn parse_gate_ms(flag: &str, v: &str) -> Result<f64, String> {
    v.parse::<f64>()
        .ok()
        .filter(|m| m.is_finite() && *m > 0.0)
        .ok_or_else(|| format!("{flag} 需要正数（毫秒），得到 {v:?}"))
}

#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    Ok,
    Help,
    ParamError(String),
    Failed(String),
}

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

pub(crate) fn percentile_ms(sorted_ms: &[f64], p: f64) -> f64 {
    let n = sorted_ms.len();
    let rank = (p * n as f64).ceil().max(1.0) as usize;
    sorted_ms[rank.min(n) - 1]
}

pub(crate) fn display_width(s: &str) -> usize {
    s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

const HELP_INDENT: usize = 4;
const HELP_GAP: usize = 2;
const HELP_LINE_WIDTH: usize = 80;
const HELP_TITLE: &str = "ledger-perf —— Ledger 性能基准工具";
const HELP_USAGE_LINE: &str = "    ledger-perf <SUBCOMMAND> [OPTIONS]";
const HELP_FLAG_ENTRY: (&str, &str) = ("-h, --help", "打印本说明");

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
    !(wordish(prev) && wordish(next))
}

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

pub(crate) struct DurationStats {
    pub min_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
}

pub(crate) fn summarize(mut durations: Vec<Duration>) -> DurationStats {
    durations.sort();
    let ms: Vec<f64> = durations.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    DurationStats {
        min_ms: ms[0],
        avg_ms: ms.iter().sum::<f64>() / ms.len() as f64,
        p95_ms: percentile_ms(&ms, 0.95),
    }
}

pub(crate) trait MetricTableRow {
    fn name(&self) -> &str;
    fn min_ms(&self) -> f64;
    fn avg_ms(&self) -> f64;
    fn p95_ms(&self) -> f64;
    fn context(&self) -> &str;
}

const NAME_HEADER: &str = "基准";

pub(crate) fn metric_table_header(name_width: usize) -> String {
    let header_pad = " ".repeat(name_width - display_width(NAME_HEADER));
    format!("{NAME_HEADER}{header_pad}       min        avg        p95  规模备注（毫秒）")
}

pub(crate) fn name_column_width<'a>(names: impl IntoIterator<Item = &'a str>) -> usize {
    names
        .into_iter()
        .map(display_width)
        .max()
        .unwrap_or(0)
        .max(display_width(NAME_HEADER))
}

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
