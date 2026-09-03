//! bench 子命令：对 generate 产出的库跑 10 项查询基准并输出 min/avg/p95 报告
//! （issue #461 / spec #458）。
//!
//! 唯一接缝（验收项）：全部基准经「现有 pub 查询函数 + 标准连接工厂
//! （[`open_connection`]）打开文件库」调用，与 IPC 命令同一 SQL 路径——
//! 基准代码内零手写业务 SQL、不重写任何查询；慢查询日志（perf_trace，
//! ≥100ms warn）由连接工厂自动挂载，超阈值单条 SQL 在日志中给出线索，
//! 本模块只初始化 tracing subscriber 让日志可见。
//!
//! 统计口径：每项预热 `--warmup` 次（不计入统计，主要热 SQLite 页缓存与
//! 语句缓存），计时 `--iterations` 次取 min / avg / p95（p95 按最近秩法，
//! 迭代数 < 20 时退化为接近 max 的保守值）。

use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{Months, NaiveDate};
use rusqlite::Connection;

use tauri_app_lib::accounts;
use tauri_app_lib::dashboard as dashboard_domain;
use tauri_app_lib::db::open_connection;
use tauri_app_lib::db::perf_trace::DEFAULT_SLOW_QUERY_THRESHOLD;
use tauri_app_lib::investment::holdings::holdings_as_of;
use tauri_app_lib::investment::list_holdings;
use tauri_app_lib::reports as reports_domain;
use tauri_app_lib::transaction::{
    TransactionListFilter, list_transactions_internal, search_transactions_internal,
};

/// 列表类基准的页大小（与前端默认页大小同量级）。
const PAGE_SIZE: usize = 20;

/// bench 参数（解析后形态；默认值见 [`Default`]）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BenchCli {
    /// 目标库文件（须已由 generate 产出）。
    pub db: PathBuf,
    /// 每项基准预热次数（不计入统计）。
    pub warmup: usize,
    /// 每项基准计时迭代次数。
    pub iterations: usize,
    /// 备注搜索基准的关键字（默认「咖啡」，命中备注池并驱动拼音过滤路径）。
    pub search: String,
}

impl Default for BenchCli {
    fn default() -> Self {
        BenchCli {
            db: super::default_out(),
            warmup: 3,
            iterations: 10,
            search: "咖啡".to_string(),
        }
    }
}

/// 参数解析结果：运行参数或帮助请求。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParsedBench {
    Run(BenchCli),
    Help,
}

/// 手写参数解析（零新增依赖）。返回 Err(消息) 表示用法错误。
pub(crate) fn parse_bench_args(args: &[String]) -> Result<ParsedBench, String> {
    let mut cli = BenchCli::default();
    let mut i = 0;
    while i < args.len() {
        let (key, inline_value) = match args[i].split_once('=') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (args[i].clone(), None),
        };
        let take_value = |i: &mut usize, inline: Option<String>| -> Result<String, String> {
            if let Some(v) = inline {
                return Ok(v);
            }
            let next = args.get(*i + 1).ok_or_else(|| format!("{key} 缺少值"))?;
            *i += 1;
            Ok(next.clone())
        };
        match key.as_str() {
            "--db" => {
                cli.db = PathBuf::from(take_value(&mut i, inline_value)?);
            }
            "--warmup" => {
                let v = take_value(&mut i, inline_value)?;
                cli.warmup = v
                    .parse::<usize>()
                    .map_err(|_| format!("--warmup 需要非负整数，得到 {v:?}"))?;
            }
            "--iterations" => {
                let v = take_value(&mut i, inline_value)?;
                cli.iterations = v
                    .parse::<usize>()
                    .map_err(|_| format!("--iterations 需要非负整数，得到 {v:?}"))?;
            }
            "--search" => {
                cli.search = take_value(&mut i, inline_value)?;
            }
            "-h" | "--help" => return Ok(ParsedBench::Help),
            other => return Err(format!("未知参数 {other:?}")),
        }
        i += 1;
    }
    if cli.iterations == 0 {
        return Err("--iterations 至少为 1".to_string());
    }
    Ok(ParsedBench::Run(cli))
}

/// 基准运行配置（测试可注入小参数；与 [`BenchCli`] 的 CLI 字段一一对应）。
#[derive(Debug, Clone)]
pub(crate) struct BenchConfig {
    pub warmup: usize,
    pub iterations: usize,
    pub search_term: String,
}

/// 单项基准的统计结果（人读报告行 + 冒烟断言面）。
#[derive(Debug, Clone)]
pub(crate) struct BenchMetrics {
    /// 基准名（名单钉住在 tests，增删必须显式更新）。
    pub name: &'static str,
    /// 规模备注：本次基准处理的数据量线索（行数/命中数/页码等）。
    pub context: String,
    pub min_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
    /// 实际计时迭代次数（报告头部已统一展示配置值；生产路径不读，
    /// 仅供 bin 内冒烟断言「产出全部指标」使用，与 tests 豁免同款纪律）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub iterations: usize,
}

/// 单项基准的执行体：吃连接、跑一次查询、返回人读规模备注
/// （行数/命中数/数量等线索，由各基准自行描述语义）。
type BenchFn<'a> = dyn Fn(&Connection) -> Result<String, String> + 'a;

/// 最近秩法 p95：升序样本取第 ⌈0.95·n⌉ 个（n<20 时偏保守、接近 max）。
fn percentile_ms(sorted_ms: &[f64], p: f64) -> f64 {
    let n = sorted_ms.len();
    let rank = (p * n as f64).ceil().max(1.0) as usize;
    sorted_ms[rank.min(n) - 1]
}

/// 入口：打开库、跑全部基准、打印人读表格。
pub(crate) fn run(cli: BenchCli) -> Result<(), String> {
    init_tracing();
    if !cli.db.exists() {
        return Err(format!(
            "库文件不存在：{}（先运行 ledger-perf generate 生成）",
            cli.db.display()
        ));
    }
    let conn = open_connection(&cli.db).map_err(|e| e.to_string())?;
    let cfg = BenchConfig {
        warmup: cli.warmup,
        iterations: cli.iterations,
        search_term: cli.search.clone(),
    };
    let results = run_benchmarks(&conn, &cfg)?;
    print_report(&cli.db, &cfg, &results);
    Ok(())
}

/// 基准执行核心（测试接缝）：对已打开的连接跑全部 10 项基准。
///
/// 前置数据（账户 id、日期极值、深分页页码）全部经现有查询函数在预热外
/// 一次性探测，基准闭包内只做「参数已定型的单次查询调用」。
pub(crate) fn run_benchmarks(
    conn: &Connection,
    cfg: &BenchConfig,
) -> Result<Vec<BenchMetrics>, String> {
    // ---- 前置探测（同样走现有查询函数，不计入任何基准） ------------------
    let range = reports_domain::query_report_date_range(conn).map_err(|e| e.to_string())?;
    let (min_date, max_date) = match (range.min_date, range.max_date) {
        (Some(min), Some(max)) => (min, max),
        _ => return Err("库为空（无未删除交易），请先运行 ledger-perf generate".to_string()),
    };
    let all_accounts = accounts::list_accounts(conn).map_err(|e| e.to_string())?;
    let first_account = all_accounts
        .first()
        .ok_or_else(|| "库内无账户，请先运行 ledger-perf generate".to_string())?;
    let first_account_id = first_account.id.clone();

    // 首页列表探测 total → 推导深分页页码（最后一页，OFFSET 逼近全量）。
    let first_filter = TransactionListFilter {
        page_size: Some(PAGE_SIZE),
        page: Some(1),
        ..TransactionListFilter::default()
    };
    let first_page = list_transactions_internal(conn, &first_filter).map_err(|e| e.to_string())?;
    if first_page.total == 0 {
        return Err("库为空（无未删除交易），请先运行 ledger-perf generate".to_string());
    }
    let deep_page = (first_page.total as usize).div_ceil(PAGE_SIZE);
    let deep_offset = (deep_page - 1) * PAGE_SIZE;

    // 日期筛选窗口：数据窗口末端的近 12 个月（NaiveDate 只做参数整形，非业务 SQL）。
    let window_end = NaiveDate::parse_from_str(&max_date, "%Y-%m-%d")
        .map_err(|e| format!("日期极值解析失败（{max_date}）：{e}"))?;
    let window_start = window_end
        .checked_sub_months(Months::new(12))
        .map(|d| d.to_string())
        .ok_or_else(|| "日期窗口起点计算失败".to_string())?;

    // ---- 10 项基准（每项一个定型参数的查询闭包） ------------------------
    let first_page_filter = TransactionListFilter {
        page_size: Some(PAGE_SIZE),
        page: Some(1),
        ..TransactionListFilter::default()
    };
    let deep_filter = TransactionListFilter {
        page_size: Some(PAGE_SIZE),
        page: Some(deep_page),
        ..TransactionListFilter::default()
    };
    let account_filter = TransactionListFilter {
        account_id: Some(first_account_id.clone()),
        from: Some(window_start.clone()),
        to: Some(max_date.clone()),
        page_size: Some(PAGE_SIZE),
        page: Some(1),
        ..TransactionListFilter::default()
    };
    // 每个闭包专用克隆（move 捕获，互不争用所有权）。
    let search_term = cfg.search_term.clone();
    let account_window_end = max_date.clone();
    let monthly_min = min_date.clone();
    let monthly_max = max_date.clone();
    let shares_min = min_date.clone();
    let shares_max = max_date.clone();
    let as_of_date = max_date.clone();

    let benches: Vec<(&'static str, Box<BenchFn>)> = vec![
        (
            "列表首页分页",
            Box::new(|conn| {
                list_transactions_internal(conn, &first_page_filter)
                    .map_err(|e| e.to_string())
                    .map(|r| format!("每页 {PAGE_SIZE} 行，共 {} 笔", r.total))
            }),
        ),
        (
            "深分页",
            Box::new(move |conn| {
                list_transactions_internal(conn, &deep_filter)
                    .map_err(|e| e.to_string())
                    .map(|r| {
                        format!(
                            "第 {deep_page} 页（OFFSET {deep_offset} 逼近全量），{} 行",
                            r.items.len()
                        )
                    })
            }),
        ),
        (
            "账户日期筛选列表",
            Box::new(move |conn| {
                list_transactions_internal(conn, &account_filter)
                    .map_err(|e| e.to_string())
                    .map(|r| {
                        format!(
                            "{window_start} → {account_window_end} × 首账户，命中 {} 笔",
                            r.total
                        )
                    })
            }),
        ),
        (
            "全账户实时余额",
            Box::new(|conn| {
                accounts::list_account_balances_with_visibility(conn, false)
                    .map_err(|e| e.to_string())
                    .map(|rows| format!("{} 个账户", rows.len()))
            }),
        ),
        (
            "月度汇总",
            Box::new(move |conn| {
                // 期间口径：from/to 任一存在即按期间聚合，year 参数不参与。
                reports_domain::monthly_summary_rows(
                    conn,
                    0,
                    Some(&monthly_min),
                    Some(&monthly_max),
                )
                .map_err(|e| e.to_string())
                .map(|rows| format!("{monthly_min} → {monthly_max}，{} 个月", rows.len()))
            }),
        ),
        (
            "分类占比",
            Box::new(move |conn| {
                reports_domain::category_shares_rows(
                    conn,
                    "expense",
                    None,
                    None,
                    Some(&shares_min),
                    Some(&shares_max),
                )
                .map_err(|e| e.to_string())
                .map(|rows| {
                    format!(
                        "{shares_min} → {shares_max} expense 净值，{} 个分类",
                        rows.len()
                    )
                })
            }),
        ),
        (
            "备注搜索拼音过滤",
            Box::new(move |conn| {
                search_transactions_internal(
                    conn,
                    &search_term,
                    1,
                    PAGE_SIZE,
                    None,
                    None,
                    None,
                    None,
                )
                .map_err(|e| e.to_string())
                .map(|r| format!("关键字「{search_term}」全量扫描，命中 {} 条", r.total))
            }),
        ),
        (
            "净资产总览",
            Box::new(|conn| {
                dashboard_domain::query_dashboard_overview(conn)
                    .map_err(|e| e.to_string())
                    .map(|o| format!("净资产 {} 分（余额+持仓市值跨币种折算）", o.net_worth_cents))
            }),
        ),
        (
            "持仓列表",
            Box::new(|conn| {
                list_holdings(conn)
                    .map_err(|e| e.to_string())
                    .map(|rows| format!("v_holdings 视图，{} 条持仓", rows.len()))
            }),
        ),
        (
            "时点持仓",
            Box::new(move |conn| {
                holdings_as_of(conn, None, &as_of_date)
                    .map_err(|e| e.to_string())
                    .map(|q| format!("全组合 @{as_of_date}（直接聚合标的交易），合计 {q:.2}"))
            }),
        ),
    ];

    // ---- 预热 + 计时 ------------------------------------------------------
    if cfg.iterations == 0 {
        return Err("--iterations 至少为 1".to_string());
    }
    let mut results = Vec::with_capacity(benches.len());
    for (name, bench_fn) in &benches {
        for _ in 0..cfg.warmup {
            bench_fn(conn).map_err(|e| format!("预热失败[{name}]：{e}"))?;
        }
        let mut durations = Vec::with_capacity(cfg.iterations);
        let mut context = String::new();
        for _ in 0..cfg.iterations {
            let start = Instant::now();
            context = bench_fn(conn).map_err(|e| format!("基准失败[{name}]：{e}"))?;
            durations.push(start.elapsed());
        }
        durations.sort();
        let ms: Vec<f64> = durations.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
        let min_ms = ms[0];
        let avg_ms = ms.iter().sum::<f64>() / ms.len() as f64;
        let p95_ms = percentile_ms(&ms, 0.95);
        results.push(BenchMetrics {
            name,
            context,
            min_ms,
            avg_ms,
            p95_ms,
            iterations: cfg.iterations,
        });
    }
    Ok(results)
}

/// 初始化 tracing subscriber（stderr，默认 info）：让连接工厂自动挂载的
/// perf_trace 慢查询 warn（≥100ms）在终端可见。重复初始化静默忽略
/// （测试进程内可能已被其它用例占用全局 subscriber）。
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}

/// 人读表格输出：min / avg / p95 三列 + 超阈值标记。
fn print_report(db: &Path, cfg: &BenchConfig, results: &[BenchMetrics]) {
    let threshold_ms = DEFAULT_SLOW_QUERY_THRESHOLD.as_secs_f64() * 1000.0;
    println!("ledger-perf bench —— 查询基准报告");
    println!("库：{}", db.display());
    println!(
        "预热 {} 次 × 迭代 {} 次；单条 SQL 慢查询阈值 {:.0}ms（超阈值语句在日志中以 warn「慢查询」给出线索）",
        cfg.warmup, cfg.iterations, threshold_ms
    );
    println!();
    // CJK 名称在 {:<N} 下按字符数填充、与终端显示宽错位，表头与名称列手排显示宽
    //（名称列显示宽 18，数字列右对齐 10/11 位，单位毫秒入表头）。
    println!("基准                        min        avg        p95  规模备注（毫秒）");
    for r in results {
        let display_width = r.name.chars().count() * 2;
        let pad = " ".repeat(18usize.saturating_sub(display_width));
        let slow_mark = if r.p95_ms > threshold_ms {
            "　▲"
        } else {
            ""
        };
        println!(
            "{name}{pad}{min:>10.2}{avg:>11.2}{p95:>11.2}  {ctx}{mark}",
            name = r.name,
            pad = pad,
            min = r.min_ms,
            avg = r.avg_ms,
            p95 = r.p95_ms,
            ctx = r.context,
            mark = slow_mark,
        );
    }
    println!();
    println!("（▲ = p95 超过慢查询阈值，日志中可按「慢查询」检索 SQL 线索）");
}
