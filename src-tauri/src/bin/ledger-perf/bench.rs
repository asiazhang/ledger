//! bench 子命令：对 generate 产出的库跑 16 项查询基准并输出 min/avg/p95 报告
//! （issue #461 / spec #458；拼音子序列基准 issue #514；商户占比与投资三项
//! 读基准 issue #1627；跨账本投资汇总基准 issue #1630）。
//!
//! 唯一接缝（验收项）：全部基准经「现有 pub 查询函数 + 标准连接工厂
//! （[`open_connection`]）打开文件库」调用，与 IPC 命令同一 SQL 路径——
//! 基准代码内零手写业务 SQL、不重写任何查询；慢查询日志（perf_trace，
//! ≥100ms warn）由连接工厂自动挂载，超阈值单条 SQL 在日志中给出线索，
//! 本模块只初始化 tracing subscriber 让日志可见。
//!
//! 统计口径：每项预热 `--warmup` 次（不计入统计，主要热 SQLite 页缓存与
//! 语句缓存），计时 `--iterations` 次取 min / avg / p95（p95 按最近秩法，
//! n=10 时 rank=⌈9.5⌉=10 恒等于 max，默认 20 次起才有真分位数分辨力）。
//!
//! 性能门禁（issue #493 / ADR-0068）：`--max-p95-ms` 给出默认阈值时，全部基准
//! 项逐项判定 p95 ≤ 各自阈值，任何一项超标则以非零退出码失败（超标清单打
//! 在报告尾部）；缺省不判定，仅输出报告（本地观察不受影响）。个别基准确需
//! 不同阈值时经 [`PER_BENCH_MAX_P95_MS`] 登记分项例外（搜索 SQL 下推落地后
//! 已按 CI 实测撤销、清单当前为空，issue #516）；判据、统计口径、分项例外
//! 与豁免原则以 ADR-0068 为准。

use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{Months, NaiveDate};
use rusqlite::Connection;

use super::bench_common::{
    CliArgs, display_width, metric_table_header, name_column_width, summarize,
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

/// 列表类基准的页大小（与前端默认页大小同量级）。
const PAGE_SIZE: usize = 20;

/// 跨账本基准里主库连接的「活动本」标识（生产＝当前活动账本，ADR-0114）。
/// 附属账本 id 恒为 book-NN（books 模块命名），不与之相撞——逐本探测路径
/// 因此覆盖全部附属账本，主库走活动本读连接（生产同款：活动本复用进程内
/// 读连接）。
const ACTIVE_BOOK_ID: &str = "ledger-perf-main";

/// 分项门禁阈值例外（ADR-0068）：默认判据 200ms 对全部基准生效；个别基准
/// 确需不同阈值时在此登记（项名精确匹配 + 阈值），增删须同步修订 ADR-0068
/// 与 tests 里的例外表钉住断言。当前为空——两条搜索基准的 400ms 全量扫描型
/// 例外已随搜索 SQL 下推落地按 CI 实测撤销（run 33890569837：p95 69.52ms /
/// 82.21ms，对默认线余量 2.9×/2.4×，issue #516），全部基准统一适用默认线。
pub(crate) const PER_BENCH_MAX_P95_MS: &[(&str, f64)] = &[];

/// 某基准项的门禁阈值：分项例外优先，未列出的用默认阈值。
fn gate_threshold_for(name: &str, default_max_p95_ms: f64) -> f64 {
    PER_BENCH_MAX_P95_MS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
        .unwrap_or(default_max_p95_ms)
}

/// bench 参数（解析后形态；默认值见 [`Default`]）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BenchCli {
    /// 目标库文件（须已由 generate 产出）。
    pub db: PathBuf,
    /// 每项基准预热次数（不计入统计）。
    pub warmup: usize,
    /// 每项基准计时迭代次数（默认 20：n=10 时最近秩 p95 恒等于 max，
    /// 20 次才成真分位数；ADR-0068 统计口径）。
    pub iterations: usize,
    /// 中文子串搜索基准的关键字（默认「咖啡」，命中备注池，驱动原文连续
    /// 子串匹配路径）。
    pub search: String,
    /// 拼音子序列搜索基准的关键字（默认「kf」：命中「买咖啡」→ mkf 等，
    /// 不构成任何备注原文子串，真正驱动拼音首字母子序列匹配路径，issue #514）。
    pub search_pinyin: String,
    /// 性能门禁阈值（毫秒）：全部基准 p95 ≤ 阈值才退出 0；None = 不判定。
    pub max_p95_ms: Option<f64>,
}

impl Default for BenchCli {
    fn default() -> Self {
        BenchCli {
            db: super::default_out(),
            warmup: 3,
            iterations: 20,
            search: "咖啡".to_string(),
            search_pinyin: "kf".to_string(),
            max_p95_ms: None,
        }
    }
}

/// 参数解析结果：运行参数或帮助请求。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParsedBench {
    Run(BenchCli),
    Help,
}

/// 手写参数解析（零新增依赖；循环机制收口在 [`super::bench_common::CliArgs`]，
/// issue #1650）。返回 Err(消息) 表示用法错误。
pub(crate) fn parse_bench_args(args: &[String]) -> Result<ParsedBench, String> {
    let mut cli = BenchCli::default();
    let mut it = CliArgs::new(args);
    while let Some(f) = it.next_flag() {
        match f.flag {
            "--db" => {
                cli.db = PathBuf::from(it.value(f)?);
            }
            "--warmup" => {
                let v = it.value(f)?;
                cli.warmup = v
                    .parse::<usize>()
                    .map_err(|_| format!("--warmup 需要非负整数，得到 {v:?}"))?;
            }
            "--iterations" => {
                let v = it.value(f)?;
                cli.iterations = v
                    .parse::<usize>()
                    .map_err(|_| format!("--iterations 需要非负整数，得到 {v:?}"))?;
            }
            "--search" => {
                cli.search = it.value(f)?;
            }
            "--search-pinyin" => {
                cli.search_pinyin = it.value(f)?;
            }
            "--max-p95-ms" => {
                let v = it.value(f)?;
                let ms = v
                    .parse::<f64>()
                    .ok()
                    .filter(|m| m.is_finite() && *m > 0.0)
                    .ok_or_else(|| format!("--max-p95-ms 需要正数（毫秒），得到 {v:?}"))?;
                cli.max_p95_ms = Some(ms);
            }
            "-h" | "--help" => return Ok(ParsedBench::Help),
            other => return Err(format!("未知参数 {other:?}")),
        }
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
    pub pinyin_search_term: String,
    /// 附属账本根目录（issue #1630；生产由 `--db` 同级推导，books 模块）：
    /// 跨账本投资汇总基准项的前置探测与逐本建连都消费它。
    pub books_dir: PathBuf,
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

/// 最近秩法 p95 与字符显示宽估算收口在 [`super::bench_common`]（issue #1650，
/// 读基准与写基准共用）；本模块读基准的表列消费共享列宽与表头构造，
/// ▲ 超阈值标记列的行渲染自持。单项基准的执行体：吃连接、跑一次查询、
/// 返回人读规模备注（行数/命中数/数量等线索，由各基准自行描述语义）。
type BenchFn<'a> = dyn Fn(&Connection) -> Result<String, String> + 'a;

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
        pinyin_search_term: cli.search_pinyin.clone(),
        books_dir: super::books::attached_books_root(&cli.db),
    };
    let results = run_benchmarks(&conn, &cfg)?;
    print_report(&cli.db, &cfg, &results);
    gate(&results, cli.max_p95_ms)?;
    Ok(())
}

/// 门禁判定（ADR-0068）：全部基准逐项 p95 ≤ 各自阈值（默认线 + 分项例外）
/// 才通过；任何一项超标，超标清单打在报告尾部（随 tee 进 Job Summary）并
/// 返回 Err（非零退出）。
fn gate(results: &[BenchMetrics], max_p95_ms: Option<f64>) -> Result<(), String> {
    let Some(max_p95_ms) = max_p95_ms else {
        return Ok(());
    };
    let failures = gate_failures(results, max_p95_ms);
    println!();
    // 门禁行据实报告分项例外状态：清单为空时不再虚指「分项例外见 ADR-0068」。
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

/// 超标项清单（判定纯函数）：p95 > 各自阈值（判据为 ≤）的基准名 + 实测值
/// + 阈值（分项例外优先，未列出者用默认线）。
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

/// 基准执行核心（测试接缝）：对已打开的连接跑全部 16 项基准。
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

    // 跨账本投资汇总基准的前置探测（issue #1630，不计入任何基准）：附属账本
    // 夹具必须齐备且 schema 与主库一致——删除生成侧（或夹具残缺）→ 此处失败
    // → 基准运行红（删除即变红），不静默少算本数。主库连接即「活动本」，
    // schema 版本同生产取自活动本（建连必经迁移，即当前应用 schema 版本）。
    let active_schema_version =
        ledger_infra::db::schema_version(conn).map_err(|e| e.to_string())?;
    let attached_books =
        super::books::discover_attached_books(&cfg.books_dir, active_schema_version)?;

    // ---- 16 项基准（每项一个定型参数的查询闭包） ------------------------
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
    let pinyin_search_term = cfg.pinyin_search_term.clone();
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
            "商户占比",
            Box::new(move |conn| {
                // 全窗口期间口径（year 不参与），top_n None = 全量（issue #588
                // 语义），报表三件套的第三件入集（issue #1627）。
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
            "备注搜索拼音子序列",
            Box::new(move |conn| {
                search_transactions_internal(
                    conn,
                    &pinyin_search_term,
                    1,
                    PAGE_SIZE,
                    None,
                    None,
                    None,
                    None,
                )
                .map_err(|e| e.to_string())
                .map(|r| {
                    format!(
                        "关键字「{pinyin_search_term}」拼音子序列全量扫描，命中 {} 条",
                        r.total
                    )
                })
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
                // 全窗口周线（issue #1627 入集；#1654 优化后：数量推算按标的
                // 分组增量推进，周采样 × 标的的嵌套循环已消除，取数成本 =
                // 全库腿流一次装载 + 逐价格行游标累加）。
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
                // 区间不设界 = IPC 命令的缺省调用形态（unwrap_or_default）；
                // 每对（账户 × 标的）一次 XIRR 数值解（200 次迭代）。
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
                // 生产编排同形（issue #1630 / ADR-0114，唯一接缝不变）：经壳层
                // 编排 pub 函数与 IPC 命令同一编排/SQL 路径——附属账本逐本只读
                // 建连探测取数（collect_other_books 内含密文/空库/版本分派，
                // 连接取数后即弃）→ 主库连接＝活动本读 → 当期汇率折算合并。
                // 每次迭代重建附属账本连接是生产形态（汇总命令逐次建连）。
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
                    + 1; // + 主库（活动本恒计入）
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

/// 初始化 tracing subscriber（stderr，默认 info）：让连接工厂自动挂载的
/// perf_trace 慢查询 warn（≥100ms）在终端可见。重复初始化静默忽略
/// （测试进程内可能已被其它用例占用全局 subscriber）。bench-import 子命令
/// 同样依赖该挂载归因刷新段耗时（issue #532），故开放为 bin 内共享。
pub(crate) fn init_tracing() {
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
    // CJK 名称在 {:<N} 下按字符数填充、与终端显示宽错位：名称列宽随最长行
    // 动态取、表头构造与写基准同源（共享 [`bench_common::name_column_width`]
    // / [`bench_common::metric_table_header`]，issue #1650）；数字列右对齐
    // 10/11 位，单位毫秒入表头；▲ 超阈值标记列自持。
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
