use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{Datelike, Days, NaiveDate};
use rusqlite::Connection;

use ledger_infra::db::now_iso;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_investment::prices::{
    MarketPriceWrite, SINA_PRICE_SOURCE, TENCENT_PRICE_SOURCE, upsert_market_price,
    upsert_price_history,
};
use ledger_investment::{InstrumentListFilter, PriceChannel};
use ledger_reports as reports_domain;

use super::bench_common::{
    FlagSpec, MetricTableRow, Parsed, parse_flags, parse_nonneg_int, parse_tier_csv,
    print_metric_table, summarize,
};
use super::snapshot::{SnapshotPaths, restore_from_snapshot};

pub(crate) const BASE_PRICE_UNITS: i64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spread {
    Concentrated,
    Uniform,
}

impl Spread {
    pub(crate) const ALL: [Spread; 2] = [Spread::Concentrated, Spread::Uniform];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Spread::Concentrated => "单标的集中",
            Spread::Uniform => "多标的均匀",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BenchMarketCli {
    pub db: PathBuf,
    pub points: Vec<usize>,
    pub warmup: usize,
    pub iterations: usize,
}

impl Default for BenchMarketCli {
    fn default() -> Self {
        BenchMarketCli {
            db: super::default_out(),
            points: vec![104, 1040, 5200],
            warmup: 1,
            iterations: 5,
        }
    }
}

pub(crate) const FLAGS: &[FlagSpec<BenchMarketCli>] = &[
    FlagSpec {
        flag: "--db <PATH>",
        help: "源库文件（默认同 generate 输出路径，须已生成；本命令不修改源库——内部建 pristine 快照，每次迭代从快照恢复）",
        apply: |cli, _flag, v| {
            cli.db = PathBuf::from(v);
            Ok(())
        },
    },
    FlagSpec {
        flag: "--points <CSV>",
        help: "每档周采样点数（默认 104,1040,5200；逗号分隔、保持次序；单档上限 1000000，采样日运算防 chrono 日期越界，issue #1650）",
        apply: |cli, flag, v| {
            cli.points = parse_tier_csv(v, flag)?;
            Ok(())
        },
    },
    FlagSpec {
        flag: "--warmup <N>",
        help: "每档预热次数（默认 1，不计入统计）",
        apply: |cli, flag, v| {
            cli.warmup = parse_nonneg_int(flag, v)?;
            Ok(())
        },
    },
    FlagSpec {
        flag: "--iterations <N>",
        help: "每档计时迭代次数（默认 5；每次迭代从快照恢复，数据集规模固定）",
        apply: |cli, flag, v| {
            cli.iterations = parse_nonneg_int(flag, v)?;
            Ok(())
        },
    },
];

pub(crate) fn parse_bench_market_args(args: &[String]) -> Result<Parsed<BenchMarketCli>, String> {
    match parse_flags(args, FLAGS)? {
        Parsed::Run(cli) if cli.iterations == 0 => Err("--iterations 至少为 1".to_string()),
        parsed => Ok(parsed),
    }
}

pub(crate) fn execute(args: &[String]) -> super::bench_common::Outcome {
    super::bench_common::execute_cli(args, parse_bench_market_args, run)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PoolInstrument {
    pub instrument_id: String,
    pub currency_code: String,
    pub is_fund: bool,
    pub source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedPoint {
    pub trade_date: String,
    pub price_units: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedSeries {
    pub pool_index: usize,
    pub points: Vec<PlannedPoint>,
}

pub(crate) fn generate_price_plan(
    points: usize,
    pool: &[PoolInstrument],
    spread: Spread,
    anchor_monday: NaiveDate,
) -> Vec<PlannedSeries> {
    let point = |i: usize| -> PlannedPoint {
        let trade_date = (anchor_monday + chrono::Duration::days(i as i64 * 7 + 4))
            .format("%Y-%m-%d")
            .to_string();
        PlannedPoint {
            trade_date,
            price_units: BASE_PRICE_UNITS + i as i64,
        }
    };
    match spread {
        Spread::Concentrated => vec![PlannedSeries {
            pool_index: 0,
            points: (0..points).map(point).collect(),
        }],
        Spread::Uniform => {
            let len = pool.len();
            let block = points.div_ceil(len);
            (0..len)
                .map(|pool_index| {
                    let start = pool_index * block;
                    let end = ((pool_index + 1) * block).min(points);
                    PlannedSeries {
                        pool_index,
                        points: (start..end).map(point).collect(),
                    }
                })
                .filter(|s| !s.points.is_empty())
                .collect()
        }
    }
}

fn next_monday(on_or_after: NaiveDate) -> NaiveDate {
    let days_from_monday = i64::from(on_or_after.weekday().num_days_from_monday());
    on_or_after + chrono::Duration::days((7 - days_from_monday) % 7)
}

pub(crate) fn count_price_history_rows(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .map_err(|e| format!("价格历史行数读取失败：{e}"))
}

pub(crate) struct MarketProbe {
    pub(crate) pool: Vec<PoolInstrument>,
    pub(crate) anchor_monday: NaiveDate,
    pub(crate) pristine_history_rows: i64,
}

pub(crate) fn probe_market_dataset(conn: &Connection) -> Result<MarketProbe, String> {
    let range = reports_domain::query_report_date_range(conn).map_err(|e| e.to_string())?;
    if range.max_date.is_none() {
        return Err("库为空（无未删除交易），请先运行 ledger-perf generate".to_string());
    }
    let max_price_date: Option<String> = conn
        .query_row("SELECT MAX(trade_date) FROM price_history", [], |r| {
            r.get(0)
        })
        .map_err(|e| format!("价格历史日期极值读取失败：{e}"))?;
    let max_price_date = max_price_date
        .ok_or_else(|| "库内无价格历史行，请先运行 ledger-perf generate".to_string())?;
    let last_sampled = NaiveDate::parse_from_str(&max_price_date, "%Y-%m-%d")
        .map_err(|e| format!("价格历史日期极值解析失败（{max_price_date}）：{e}"))?;
    let bench_date = last_sampled
        .checked_add_days(Days::new(1))
        .ok_or_else(|| "基准日期计算越界".to_string())?;

    let listing = ledger_investment::list_instruments(
        conn,
        &InstrumentListFilter {
            search: None,
            market: None,
            kind: None,
            only_invested: None,
            page: Some(1),
            page_size: Some(500),
            anchor_id: None,
        },
    )
    .map_err(|e| e.to_string())?;
    let pool: Vec<PoolInstrument> = listing
        .items
        .iter()
        .filter(|inst| {
            matches!(
                inst.price_channel,
                PriceChannel::Quote | PriceChannel::FundNav
            )
        })
        .map(|inst| PoolInstrument {
            instrument_id: inst.id.clone(),
            currency_code: inst.currency_code.clone(),
            is_fund: inst.price_channel == PriceChannel::FundNav,
            source: if inst.price_channel == PriceChannel::FundNav {
                SINA_PRICE_SOURCE
            } else {
                TENCENT_PRICE_SOURCE
            },
        })
        .collect();
    if pool.is_empty() {
        return Err(
            "库内无行情/净值通道标的，无法构建落库标的池，请先运行 ledger-perf generate"
                .to_string(),
        );
    }

    Ok(MarketProbe {
        pool,
        anchor_monday: next_monday(bench_date),
        pristine_history_rows: count_price_history_rows(conn)?,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct MarketBenchConfig {
    pub points: Vec<usize>,
    pub warmup: usize,
    pub iterations: usize,
}

impl From<BenchMarketCli> for MarketBenchConfig {
    fn from(cli: BenchMarketCli) -> Self {
        MarketBenchConfig {
            points: cli.points,
            warmup: cli.warmup,
            iterations: cli.iterations,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MarketBenchMetrics {
    pub name: String,
    #[cfg_attr(not(test), allow(dead_code))]
    pub points: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub spread: Spread,
    pub min_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
    #[cfg_attr(not(test), allow(dead_code))]
    pub per_point_p95_ms: f64,
    pub context: String,
}

pub(crate) fn run(cli: BenchMarketCli) -> Result<(), String> {
    super::bench::init_tracing();
    if !cli.db.exists() {
        return Err(format!(
            "库文件不存在：{}（先运行 ledger-perf generate 生成）",
            cli.db.display()
        ));
    }
    let db = cli.db.clone();
    let cfg = MarketBenchConfig::from(cli);
    let results = run_benchmark(&db, &cfg)?;
    print_report(&db, &cfg, &results);
    Ok(())
}

pub(crate) fn run_benchmark(
    source_db: &Path,
    cfg: &MarketBenchConfig,
) -> Result<Vec<MarketBenchMetrics>, String> {
    if cfg.iterations == 0 {
        return Err("--iterations 至少为 1".to_string());
    }
    if cfg.points.is_empty() {
        return Err("--points 至少需要一个档位".to_string());
    }
    let guard = SnapshotPaths::create(source_db, "bench-market")?;
    let probe = {
        let conn = super::snapshot::open_paired(&guard.work).map_err(|e| e.to_string())?;
        let probe = probe_market_dataset(&conn)?;
        drop(conn);
        probe
    };
    let mut results = Vec::new();
    for points in &cfg.points {
        for spread in Spread::ALL {
            results.push(run_cell(&guard, *points, spread, &probe, cfg)?);
        }
    }
    Ok(results)
}

fn run_cell(
    guard: &SnapshotPaths,
    points: usize,
    spread: Spread,
    probe: &MarketProbe,
    cfg: &MarketBenchConfig,
) -> Result<MarketBenchMetrics, String> {
    let label = format!("行情 {points} 点·{}", spread.label());
    let mut durations = Vec::with_capacity(cfg.iterations);
    for iteration in 0..(cfg.warmup + cfg.iterations) {
        restore_from_snapshot(&guard.snapshot, &guard.work)?;
        let conn = super::snapshot::open_paired(&guard.work).map_err(|e| e.to_string())?;
        let plan = generate_price_plan(points, &probe.pool, spread, probe.anchor_monday);
        let baseline = count_price_history_rows(&conn)?;
        if baseline != probe.pristine_history_rows {
            return Err(format!(
                "[{label}] 快照恢复不完整：迭代前价格历史行数 {baseline} ≠ pristine 基线 {}，连续迭代存在写副作用残留，量测无效",
                probe.pristine_history_rows
            ));
        }
        let start = Instant::now();
        for series in &plan {
            write_series(&conn, series, &probe.pool[series.pool_index], &label)
                .map_err(|e| format!("[{label}] {e}"))?;
        }
        let elapsed = start.elapsed();
        verify_writes(&conn, &plan, probe, points, &label)?;
        drop(conn);
        if iteration >= cfg.warmup {
            durations.push(elapsed);
        }
    }
    let stats = summarize(durations);
    let per_point_p95_ms = stats.p95_ms / points as f64;
    Ok(MarketBenchMetrics {
        name: label.clone(),
        points,
        spread,
        min_ms: stats.min_ms,
        avg_ms: stats.avg_ms,
        p95_ms: stats.p95_ms,
        per_point_p95_ms,
        context: format!(
            "现价+周采样 ×{points} 点，锚点周一 {}；单点均摊 p95 {per_point_p95_ms:.3} ms/点",
            probe.anchor_monday.format("%Y-%m-%d"),
        ),
    })
}

fn write_series(
    conn: &Connection,
    series: &PlannedSeries,
    pool_entry: &PoolInstrument,
    label: &str,
) -> Result<(), String> {
    let last = series.points.last().ok_or_else(|| {
        format!(
            "[{label}] 落库计划为空序列（标的 {}）",
            pool_entry.instrument_id
        )
    })?;
    ensure_transaction(conn, || -> ledger_infra::error::Result<()> {
        let now = now_iso();
        let priced_at: &str = if pool_entry.is_fund {
            &last.trade_date
        } else {
            &now
        };
        let nav_date = pool_entry.is_fund.then_some(last.trade_date.as_str());
        upsert_market_price(
            conn,
            &MarketPriceWrite {
                instrument_id: &pool_entry.instrument_id,
                price_cents: last.price_units,
                currency_code: &pool_entry.currency_code,
                priced_at,
                nav_date,
                source: Some(pool_entry.source),
            },
        )?;
        for point in &series.points {
            upsert_price_history(
                conn,
                &pool_entry.instrument_id,
                &point.trade_date,
                point.price_units,
                &pool_entry.currency_code,
                pool_entry.source,
            )?;
        }
        Ok(())
    })
    .map_err(|e| format!("行情批量落库失败（标的 {}）：{e}", pool_entry.instrument_id))
}

fn verify_writes(
    conn: &Connection,
    plan: &[PlannedSeries],
    probe: &MarketProbe,
    points: usize,
    label: &str,
) -> Result<(), String> {
    let written = count_price_history_rows(conn)? - probe.pristine_history_rows;
    if written != points as i64 {
        return Err(format!(
            "[{label}] 量测有效性前置失败：价格历史行数恰增 {written} ≠ 点数 {points}——点流应全为新周、行行真写，命中整周覆盖即量测无效"
        ));
    }
    let prices = ledger_investment::list_market_prices(conn).map_err(|e| e.to_string())?;
    let mut mismatches: Vec<String> = Vec::new();
    for series in plan {
        let pool_entry = &probe.pool[series.pool_index];
        let Some(last) = series.points.last() else {
            mismatches.push(format!("{}: 落库计划为空序列", pool_entry.instrument_id));
            continue;
        };
        match prices
            .iter()
            .find(|p| p.instrument_id == pool_entry.instrument_id)
        {
            Some(row) if row.price_cents != last.price_units => mismatches.push(format!(
                "{}: 现价 {} ≠ 末点 {}",
                pool_entry.instrument_id, row.price_cents, last.price_units
            )),
            None => mismatches.push(format!("{}: 现价行缺失", pool_entry.instrument_id)),
            _ => {}
        }
    }
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "[{label}] 量测有效性前置失败：现价行与点流不一致（{} 项）——{}",
            mismatches.len(),
            mismatches.join("；")
        ))
    }
}

impl MetricTableRow for MarketBenchMetrics {
    fn name(&self) -> &str {
        &self.name
    }
    fn min_ms(&self) -> f64 {
        self.min_ms
    }
    fn avg_ms(&self) -> f64 {
        self.avg_ms
    }
    fn p95_ms(&self) -> f64 {
        self.p95_ms
    }
    fn context(&self) -> &str {
        &self.context
    }
}

fn print_report(db: &Path, cfg: &MarketBenchConfig, results: &[MarketBenchMetrics]) {
    println!(
        "ledger-perf bench-market —— 行情/价格历史批量 upsert 写基准报告（纯观测，无门禁，人工判读）"
    );
    println!(
        "源库：{}（本命令不修改源库；每次迭代从 pristine 快照恢复，数据集规模固定）",
        db.display()
    );
    println!(
        "矩阵：点数档 {:?} × 分布 [单标的集中, 多标的均匀]；预热 {} 次 × 迭代 {} 次",
        cfg.points, cfg.warmup, cfg.iterations
    );
    println!(
        "统计口径：最近秩 p95，n<20 时 p95=max 无分位数分辨力（同 ADR-0068 口径），要真分位数请提高 --iterations"
    );
    println!();
    print_metric_table(results);
    println!();
    println!(
        "归因说明：计时窗口 = 逐标的一个事务（现价 upsert + 周采样逐点 upsert），全部经价格写入单点，"
    );
    println!(
        "与生产同步/回填通道同一 SQL 路径；行情源网络抓取与报文解析已剥离（ADR-0068 网络边界豁免），不在计时窗口内。"
    );
    println!(
        "慢查询观测：单条 SQL ≥100ms 以 warn「慢查询」直接可见（连接工厂自动挂载，无需 DEBUG 级）。"
    );
}
