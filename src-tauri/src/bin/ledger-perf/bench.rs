use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{Months, NaiveDate};
use rusqlite::Connection;

use super::bench_common::{
    FlagSpec, Parsed, display_width, metric_table_header, name_column_width, parse_flags,
    parse_gate_ms, parse_nonneg_int, summarize,
};
use ledger_accounts as accounts;
use ledger_dashboard as dashboard_domain;
use ledger_infra::db::open_connection;
use ledger_infra::db::perf_trace::DEFAULT_SLOW_QUERY_THRESHOLD;
use ledger_investment::holdings::holdings_as_of;
use ledger_investment::{
    MwrRange, TrendRange, list_holdings, query_financial_freedom,
    query_money_weighted_return_summary, query_portfolio_value_trend,
};
use ledger_reports as reports_domain;
use ledger_transaction::amount;
use ledger_transaction::{
    TransactionListFilter, list_transactions_internal, search_transactions_internal,
};
use tauri_app_lib::cross_book_summary::{
    CrossBookBookStatus, collect_other_books, merge_readings, read_book_investment,
};

const PAGE_SIZE: usize = 20;

const ACTIVE_BOOK_ID: &str = "ledger-perf-main";

pub(crate) const PER_BENCH_MAX_P95_MS: &[(&str, f64)] = &[];

fn gate_threshold_for(name: &str, default_max_p95_ms: f64) -> f64 {
    PER_BENCH_MAX_P95_MS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
        .unwrap_or(default_max_p95_ms)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BenchCli {
    pub db: PathBuf,
    pub warmup: usize,
    pub iterations: usize,
    pub search: String,
    pub purchase_search: String,
    pub max_p95_ms: Option<f64>,
}

impl Default for BenchCli {
    fn default() -> Self {
        BenchCli {
            db: super::default_out(),
            warmup: 3,
            iterations: 20,
            search: "咖啡".to_string(),
            purchase_search: "猫粮".to_string(),
            max_p95_ms: None,
        }
    }
}

pub(crate) const FLAGS: &[FlagSpec<BenchCli>] = &[
    FlagSpec {
        flag: "--db <PATH>",
        help: "目标库文件（默认同 generate 输出路径，须已生成）",
        apply: |cli, _flag, v| {
            cli.db = PathBuf::from(v);
            Ok(())
        },
    },
    FlagSpec {
        flag: "--warmup <N>",
        help: "每项基准预热次数（默认 3，不计入统计）",
        apply: |cli, flag, v| {
            cli.warmup = parse_nonneg_int(flag, v)?;
            Ok(())
        },
    },
    FlagSpec {
        flag: "--iterations <N>",
        help: "每项基准计时迭代次数（默认 20，n=20 才成真 p95 分位数）",
        apply: |cli, flag, v| {
            cli.iterations = parse_nonneg_int(flag, v)?;
            Ok(())
        },
    },
    FlagSpec {
        flag: "--search <TERM>",
        help: "中文子串搜索基准的关键字（默认 咖啡）",
        apply: |cli, _flag, v| {
            cli.search = v.to_string();
            Ok(())
        },
    },
    FlagSpec {
        flag: "--purchase-search <TERM>",
        help: "商品名搜索基准的关键字（默认 猫粮）",
        apply: |cli, _flag, v| {
            cli.purchase_search = v.to_string();
            Ok(())
        },
    },
    FlagSpec {
        flag: "--max-p95-ms <MS>",
        help: "默认门禁阈值（毫秒）：全部基准 p95 ≤ 各自阈值才退出 0，任何一项超标即失败（CI 用；缺省不判定；分项例外机制与现行清单见 ADR-0068）",
        apply: |cli, flag, v| {
            cli.max_p95_ms = Some(parse_gate_ms(flag, v)?);
            Ok(())
        },
    },
];

pub(crate) fn parse_bench_args(args: &[String]) -> Result<Parsed<BenchCli>, String> {
    match parse_flags(args, FLAGS)? {
        Parsed::Run(cli) if cli.iterations == 0 => Err("--iterations 至少为 1".to_string()),
        parsed => Ok(parsed),
    }
}

pub(crate) fn execute(args: &[String]) -> super::bench_common::Outcome {
    super::bench_common::execute_cli(args, parse_bench_args, run)
}

#[derive(Debug, Clone)]
pub(crate) struct BenchConfig {
    pub warmup: usize,
    pub iterations: usize,
    pub search_term: String,
    pub purchase_search_term: String,
    pub books_dir: PathBuf,
}

impl From<BenchCli> for BenchConfig {
    fn from(cli: BenchCli) -> Self {
        BenchConfig {
            warmup: cli.warmup,
            iterations: cli.iterations,
            search_term: cli.search,
            purchase_search_term: cli.purchase_search,
            books_dir: super::books::attached_books_root(&cli.db),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BenchMetrics {
    pub name: &'static str,
    pub context: String,
    pub min_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
    #[cfg_attr(not(test), allow(dead_code))]
    pub iterations: usize,
}

type BenchFn<'a> = dyn Fn(&Connection) -> Result<String, String> + 'a;

pub(crate) fn run(cli: BenchCli) -> Result<(), String> {
    init_tracing();
    if !cli.db.exists() {
        return Err(format!(
            "库文件不存在：{}（先运行 ledger-perf generate 生成）",
            cli.db.display()
        ));
    }
    let max_p95_ms = cli.max_p95_ms;
    let db = cli.db.clone();
    let cfg = BenchConfig::from(cli);
    let conn = open_connection(&db).map_err(|e| e.to_string())?;
    let results = run_benchmarks(&conn, &cfg)?;
    print_report(&db, &cfg, &results);
    gate(&results, max_p95_ms)?;
    Ok(())
}

fn gate(results: &[BenchMetrics], max_p95_ms: Option<f64>) -> Result<(), String> {
    let Some(max_p95_ms) = max_p95_ms else {
        return Ok(());
    };
    let failures = gate_failures(results, max_p95_ms);
    println!();
    let line_prefix = if PER_BENCH_MAX_P95_MS.is_empty() {
        format!("门禁（全部基准 p95 ≤ {max_p95_ms:.0}ms，无分项例外）")
    } else {
        format!("门禁（默认 p95 ≤ {max_p95_ms:.0}ms，分项例外见 ADR-0068）")
    };
    if failures.is_empty() {
        println!("{line_prefix}：通过（{} 项全部达标）", results.len());
        Ok(())
    } else {
        println!(
            "{line_prefix}：失败——{} 项超标：{}",
            failures.len(),
            failures.join("、")
        );
        Err(format!(
            "性能门禁失败：{} 项 p95 超各自阈值（默认 {max_p95_ms:.0}ms，超标项见上方报告门禁行）",
            failures.len()
        ))
    }
}

pub(crate) fn gate_failures(results: &[BenchMetrics], max_p95_ms: f64) -> Vec<String> {
    results
        .iter()
        .filter_map(|r| {
            let threshold = gate_threshold_for(r.name, max_p95_ms);
            (r.p95_ms > threshold).then(|| {
                format!(
                    "{}（p95 {:.2}ms > 阈值 {:.0}ms）",
                    r.name, r.p95_ms, threshold
                )
            })
        })
        .collect()
}

pub(crate) fn run_benchmarks(
    conn: &Connection,
    cfg: &BenchConfig,
) -> Result<Vec<BenchMetrics>, String> {
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

    let window_end = NaiveDate::parse_from_str(&max_date, "%Y-%m-%d")
        .map_err(|e| format!("日期极值解析失败（{max_date}）：{e}"))?;
    let window_start = window_end
        .checked_sub_months(Months::new(12))
        .map(|d| d.to_string())
        .ok_or_else(|| "日期窗口起点计算失败".to_string())?;

    let active_schema_version =
        ledger_infra::db::schema_version(conn).map_err(|e| e.to_string())?;
    let attached_books =
        super::books::discover_attached_books(&cfg.books_dir, active_schema_version)?;

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
    let search_term = cfg.search_term.clone();
    let purchase_term = cfg.purchase_search_term.clone();
    let account_window_end = max_date.clone();
    let monthly_min = min_date.clone();
    let monthly_max = max_date.clone();
    let shares_min = min_date.clone();
    let shares_max = max_date.clone();
    let merchant_min = min_date.clone();
    let merchant_max = max_date.clone();
    let trend_min = min_date.clone();
    let trend_max = max_date.clone();
    let trend_range = TrendRange {
        start_date: Some(trend_min.clone()),
        end_date: Some(trend_max.clone()),
    };
    let mwr_range = MwrRange::default();
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
            "商户占比",
            Box::new(move |conn| {
                reports_domain::merchant_shares_report(
                    conn,
                    0,
                    Some(&merchant_min),
                    Some(&merchant_max),
                    None,
                )
                .map_err(|e| e.to_string())
                .map(|r| {
                    format!(
                        "{merchant_min} → {merchant_max} expense 净值，{} 个商户，合计 {} 分",
                        r.rows.len(),
                        r.total_cents
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
            "商品名搜索",
            Box::new(move |conn| {
                search_transactions_internal(
                    conn,
                    &purchase_term,
                    1,
                    PAGE_SIZE,
                    None,
                    None,
                    None,
                    None,
                )
                .map_err(|e| e.to_string())
                .map(|r| format!("关键字「{purchase_term}」命中购买项订单 {} 笔", r.total))
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
        (
            "投资组合趋势",
            Box::new(move |conn| {
                query_portfolio_value_trend(conn, &trend_range)
                    .map_err(|e| e.to_string())
                    .map(|t| {
                        format!(
                            "{trend_min} → {trend_max} 全组合周线，{} 个周点（折算 {}）",
                            t.points.len(),
                            t.currency_code
                        )
                    })
            }),
        ),
        (
            "资金加权收益率",
            Box::new(move |conn| {
                query_money_weighted_return_summary(conn, &mwr_range)
                    .map_err(|e| e.to_string())
                    .map(|s| {
                        format!(
                            "区间不设界，标的行 {} / 账户行 {} / 币种行 {}（含 XIRR 数值解）",
                            s.by_instrument.len(),
                            s.by_account.len(),
                            s.total.len()
                        )
                    })
            }),
        ),
        (
            "财务自由度",
            Box::new(|conn| {
                query_financial_freedom(conn)
                    .map_err(|e| e.to_string())
                    .map(|o| {
                        format!(
                            "可投资资产 {} 分（{}），年预算 {} 分，覆盖 {} 年",
                            o.numerator_cents,
                            o.native_currency,
                            o.denominator_cents,
                            o.coverage_years
                        )
                    })
            }),
        ),
        (
            "跨账本投资汇总",
            Box::new(move |conn| {
                let (rows, mut readings) =
                    collect_other_books(&attached_books, ACTIVE_BOOK_ID, active_schema_version);
                readings.push(read_book_investment(conn).map_err(|e| e.to_string())?);
                let target_currency =
                    amount::default_currency_code(conn).map_err(|e| e.to_string())?;
                let totals = merge_readings(&readings, &target_currency, &mut |cents, currency| {
                    amount::convert_to_native_current(conn, cents, currency)
                })
                .map_err(|e| e.to_string())?;
                let included = rows
                    .iter()
                    .filter(|r| r.status == CrossBookBookStatus::Included)
                    .count()
                    + 1;
                Ok(format!(
                    "{included}/{} 本计入（{} 附属 + 主库），市值合计 {} 分，可投资资产 {} 分",
                    rows.len() + 1,
                    attached_books.len(),
                    totals.market_value_cents,
                    totals.investable_assets_cents,
                ))
            }),
        ),
    ];

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
        let stats = summarize(durations);
        results.push(BenchMetrics {
            name,
            context,
            min_ms: stats.min_ms,
            avg_ms: stats.avg_ms,
            p95_ms: stats.p95_ms,
            iterations: cfg.iterations,
        });
    }
    Ok(results)
}

pub(crate) fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}

fn print_report(db: &Path, cfg: &BenchConfig, results: &[BenchMetrics]) {
    let threshold_ms = DEFAULT_SLOW_QUERY_THRESHOLD.as_secs_f64() * 1000.0;
    println!("ledger-perf bench —— 查询基准报告");
    println!("库：{}", db.display());
    println!(
        "预热 {} 次 × 迭代 {} 次；单条 SQL 慢查询阈值 {:.0}ms（超阈值语句在日志中以 warn「慢查询」给出线索）",
        cfg.warmup, cfg.iterations, threshold_ms
    );
    println!();
    let name_width = name_column_width(results.iter().map(|r| r.name));
    println!("{}", metric_table_header(name_width));
    for r in results {
        let pad = " ".repeat(name_width - display_width(r.name));
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
