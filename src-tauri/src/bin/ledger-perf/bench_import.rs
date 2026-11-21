use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{Days, NaiveDate};
use rusqlite::Connection;

use ledger_accounts as accounts;
use ledger_accounts::{Account, AccountType, balance};
use ledger_reports as reports_domain;
use ledger_transaction::amount::{TransactionKind, default_currency_code};
use ledger_transaction::{BatchOutcome, TransactionBatch, TransactionInput};

use super::bench_common::{
    FlagSpec, MetricTableRow, Parsed, parse_bool_value, parse_flags, parse_nonneg_int,
    parse_tier_csv, print_metric_table, summarize,
};
use super::snapshot::{SnapshotPaths, restore_from_snapshot};

pub(crate) const BASE_AMOUNT_CENTS: i64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Distribution {
    Concentrated,
    Uniform,
}

impl Distribution {
    pub(crate) const ALL: [Distribution; 2] = [Distribution::Concentrated, Distribution::Uniform];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Distribution::Concentrated => "同账户集中",
            Distribution::Uniform => "多账户均匀",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BenchImportCli {
    pub db: PathBuf,
    pub rows: Vec<usize>,
    pub dedup: bool,
    pub warmup: usize,
    pub iterations: usize,
}

impl Default for BenchImportCli {
    fn default() -> Self {
        BenchImportCli {
            db: super::default_out(),
            rows: vec![50, 100, 200],
            dedup: true,
            warmup: 1,
            iterations: 5,
        }
    }
}

pub(crate) const FLAGS: &[FlagSpec<BenchImportCli>] = &[
    FlagSpec {
        flag: "--db <PATH>",
        help: "源库文件（默认同 generate 输出路径，须已生成；本命令不修改源库——内部建 pristine 快照，每次迭代从快照恢复）",
        apply: |cli, _flag, v| {
            cli.db = PathBuf::from(v);
            Ok(())
        },
    },
    FlagSpec {
        flag: "--rows <CSV>",
        help: "每档导入行数（默认 50,100,200；逗号分隔、保持次序；单档上限 1000000，issue #1650）",
        apply: |cli, flag, v| {
            cli.rows = parse_tier_csv(v, flag)?;
            Ok(())
        },
    },
    FlagSpec {
        flag: "--dedup <BOOL>",
        help: "批量导入去重开关（默认 true，HTTP 批量导入生产默认）",
        apply: |cli, flag, v| {
            cli.dedup = parse_bool_value(flag, v)?;
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

pub(crate) fn parse_bench_import_args(args: &[String]) -> Result<Parsed<BenchImportCli>, String> {
    match parse_flags(args, FLAGS)? {
        Parsed::Run(cli) if cli.iterations == 0 => Err("--iterations 至少为 1".to_string()),
        parsed => Ok(parsed),
    }
}

pub(crate) fn execute(args: &[String]) -> super::bench_common::Outcome {
    super::bench_common::execute_cli(args, parse_bench_import_args, run)
}

#[derive(Debug, Clone)]
pub(crate) struct ImportBenchConfig {
    pub rows: Vec<usize>,
    pub dedup: bool,
    pub warmup: usize,
    pub iterations: usize,
}

impl From<BenchImportCli> for ImportBenchConfig {
    fn from(cli: BenchImportCli) -> Self {
        ImportBenchConfig {
            rows: cli.rows,
            dedup: cli.dedup,
            warmup: cli.warmup,
            iterations: cli.iterations,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ImportBenchMetrics {
    pub name: String,
    #[cfg_attr(not(test), allow(dead_code))]
    pub rows: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub distribution: Distribution,
    pub min_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
    #[cfg_attr(not(test), allow(dead_code))]
    pub per_row_p95_ms: f64,
    pub context: String,
}

pub(crate) fn generate_inputs(
    rows: usize,
    account_pool: &[String],
    distribution: Distribution,
    currency: &str,
    date: &str,
) -> Vec<TransactionInput> {
    (0..rows)
        .map(|i| TransactionInput {
            funding: Vec::new(),
            kind: TransactionKind::Expense,
            amount_cents: BASE_AMOUNT_CENTS + i as i64,
            currency_code: currency.to_string(),
            account_id: match distribution {
                Distribution::Concentrated => Some(account_pool[0].clone()),
                Distribution::Uniform => Some(account_pool[i % account_pool.len()].clone()),
            },
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            source_order_no: None,
            funding_account_id: None,
            note: None,
            date: date.to_string(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            idempotency_key: None,
            origin: None,
            fx_rate: None,
            purchases: Vec::new(),
        })
        .collect()
}

pub(crate) fn eligible_account_ids(
    conn: &rusqlite::Connection,
    all: &[Account],
) -> Result<Vec<String>, String> {
    let native = default_currency_code(conn).map_err(|e| e.to_string())?;
    Ok(all
        .iter()
        .filter(|a| a.kind != AccountType::Investment && a.currency_code == native)
        .map(|a| a.id.clone())
        .collect())
}

pub(crate) fn assert_cache_matches_realtime(conn: &Connection) -> Result<(), String> {
    let cached = accounts::list_account_balances_with_visibility(conn, true)
        .map_err(|e| format!("余额缓存读取失败（缓存行缺失即不变量破坏）：{e}"))?;
    let realtime = balance::compute_all_balances_with_visibility(conn, true)
        .map_err(|e| format!("实时余额计算失败：{e}"))?;
    let mut mismatches: Vec<String> = Vec::new();
    for row in &cached {
        match realtime.get(&row.account.id) {
            Some(expected) if *expected == row.balance_cents => {}
            Some(expected) => mismatches.push(format!(
                "{}({}): 缓存 {} ≠ 实时 {}",
                row.account.id, row.account.name, row.balance_cents, expected
            )),
            None => mismatches.push(format!(
                "{}({}): 缓存行缺失于实时口径",
                row.account.id, row.account.name
            )),
        }
    }
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "余额缓存与实时计算不一致（{} 户，量测在坏缓存路径上进行、结果作废）：{}",
            mismatches.len(),
            mismatches.join("；")
        ))
    }
}

pub(crate) struct DatasetProbe {
    pub(crate) bench_date: String,
    pub(crate) account_pool: Vec<String>,
}

pub(crate) fn probe_dataset(conn: &Connection) -> Result<DatasetProbe, String> {
    let range = reports_domain::query_report_date_range(conn).map_err(|e| e.to_string())?;
    let max_date = range
        .max_date
        .ok_or_else(|| "库为空（无未删除交易），请先运行 ledger-perf generate".to_string())?;
    let end = NaiveDate::parse_from_str(&max_date, "%Y-%m-%d")
        .map_err(|e| format!("日期极值解析失败（{max_date}）：{e}"))?;
    let bench_date = end
        .checked_add_days(Days::new(1))
        .ok_or_else(|| "基准日期计算越界".to_string())?
        .to_string();
    let all_accounts = accounts::list_accounts(conn).map_err(|e| e.to_string())?;
    let account_pool = eligible_account_ids(conn, &all_accounts)?;
    if account_pool.is_empty() {
        return Err(
            "库内无本位币非投资账户，无法构建导入账户池，请先运行 ledger-perf generate".to_string(),
        );
    }
    Ok(DatasetProbe {
        bench_date,
        account_pool,
    })
}

pub(crate) fn run(cli: BenchImportCli) -> Result<(), String> {
    super::bench::init_tracing();
    if !cli.db.exists() {
        return Err(format!(
            "库文件不存在：{}（先运行 ledger-perf generate 生成）",
            cli.db.display()
        ));
    }
    let db = cli.db.clone();
    let cfg = ImportBenchConfig::from(cli);
    let results = run_benchmark(&db, &cfg)?;
    print_report(&db, &cfg, &results);
    Ok(())
}

pub(crate) fn run_benchmark(
    source_db: &Path,
    cfg: &ImportBenchConfig,
) -> Result<Vec<ImportBenchMetrics>, String> {
    if cfg.iterations == 0 {
        return Err("--iterations 至少为 1".to_string());
    }
    if cfg.rows.is_empty() {
        return Err("--rows 至少需要一个档位".to_string());
    }
    let guard = SnapshotPaths::create(source_db, "bench-import")?;
    let probe = {
        let conn = super::snapshot::open_paired(&guard.work).map_err(|e| e.to_string())?;
        let probe = probe_dataset(&conn)?;
        drop(conn);
        probe
    };
    let mut results = Vec::new();
    for rows in &cfg.rows {
        for distribution in Distribution::ALL {
            results.push(run_cell(&guard, *rows, distribution, &probe, cfg)?);
        }
    }
    Ok(results)
}

fn run_cell(
    guard: &SnapshotPaths,
    rows: usize,
    distribution: Distribution,
    probe: &DatasetProbe,
    cfg: &ImportBenchConfig,
) -> Result<ImportBenchMetrics, String> {
    let label = format!("导入 {rows} 行·{}", distribution.label());
    let mut durations = Vec::with_capacity(cfg.iterations);
    for iteration in 0..(cfg.warmup + cfg.iterations) {
        restore_from_snapshot(&guard.snapshot, &guard.work)?;
        let conn = super::snapshot::open_paired(&guard.work).map_err(|e| e.to_string())?;
        let inputs = generate_inputs(
            rows,
            &probe.account_pool,
            distribution,
            &default_currency_code(&conn).map_err(|e| e.to_string())?,
            &probe.bench_date,
        );
        let start = Instant::now();
        let outcome = TransactionBatch::run(&conn, inputs, cfg.dedup)
            .map_err(|e| format!("批量导入失败[{label}]：{e}"))?;
        let elapsed = start.elapsed();
        verify_outcome(&outcome, rows, &label)?;
        assert_cache_matches_realtime(&conn)?;
        drop(conn);
        if iteration >= cfg.warmup {
            durations.push(elapsed);
        }
    }
    let stats = summarize(durations);
    let per_row_p95_ms = stats.p95_ms / rows as f64;
    Ok(ImportBenchMetrics {
        name: label.clone(),
        rows,
        distribution,
        min_ms: stats.min_ms,
        avg_ms: stats.avg_ms,
        p95_ms: stats.p95_ms,
        per_row_p95_ms,
        context: format!(
            "expense ×{rows} 行，dedup 去重={}，基准日期 {}；单行均摊 p95 {per_row_p95_ms:.2} ms/行",
            cfg.dedup, probe.bench_date
        ),
    })
}

fn verify_outcome(outcome: &BatchOutcome, rows: usize, label: &str) -> Result<(), String> {
    if outcome.results.len() != rows {
        return Err(format!(
            "[{label}] 结果数 {} ≠ 提交行数 {rows}，量测无效",
            outcome.results.len()
        ));
    }
    if let Some(bad) = outcome
        .results
        .iter()
        .find(|r| !r.success || r.duplicate || r.error.is_some())
    {
        return Err(format!(
            "[{label}] 量测有效性前置失败：存在失败或命中去重的行（success={} duplicate={} error={:?}）——批内身份应全异、行行真写",
            bad.success, bad.duplicate, bad.error
        ));
    }
    Ok(())
}

impl MetricTableRow for ImportBenchMetrics {
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

fn print_report(db: &Path, cfg: &ImportBenchConfig, results: &[ImportBenchMetrics]) {
    println!("ledger-perf bench-import —— 批量导入写基准报告（纯观测，无门禁，人工判读）");
    println!(
        "源库：{}（本命令不修改源库；每次迭代从 pristine 快照恢复，数据集规模固定）",
        db.display()
    );
    println!(
        "矩阵：行数 {:?} × 分布 [同账户集中, 多账户均匀]；dedup 去重={}；预热 {} 次 × 迭代 {} 次",
        cfg.rows, cfg.dedup, cfg.warmup, cfg.iterations
    );
    println!(
        "统计口径：最近秩 p95，n<20 时 p95=max 无分位数分辨力（同 ADR-0068 口径），要真分位数请提高 --iterations"
    );
    println!();
    print_metric_table(results);
    println!();
    println!("刷新段归因：每行写入都在同一事务内触发一次余额缓存整体重算；");
    println!(
        "该段耗时经既有耗时日志按语句归因：RUST_LOG=debug 重跑后按「INSERT INTO account_balance_cache」检索全量明细（DEBUG 级），慢查询（≥100ms）以 warn「慢查询」直接可见。"
    );
}
