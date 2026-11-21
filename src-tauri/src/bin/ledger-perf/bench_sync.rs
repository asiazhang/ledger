use std::path::{Path, PathBuf};
use std::time::Instant;

use ledger_infra::db::{deterministic_uuid, schema_version};
use ledger_sync_engine::{ApplyReport, DomainCommand, OpOutcome, SyncOp, apply_ops, ingest_ops};
use ledger_transaction::amount::{TransactionKind, default_currency_code};
use ledger_transaction::{NormalizedTransaction, TransactionCommand};

use super::bench_common::{
    FlagSpec, MetricTableRow, Parsed, parse_flags, parse_nonneg_int, parse_tier_csv,
    print_metric_table, summarize,
};
use super::bench_import::{Distribution, assert_cache_matches_realtime, probe_dataset};
use super::snapshot::{SnapshotPaths, restore_from_snapshot};

pub(crate) const SOURCE_DEVICE_ID: &str = "ledger-perf-bench-sync-device-a";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryForm {
    Ingest,
    Apply,
}

impl EntryForm {
    pub(crate) const ALL: [EntryForm; 2] = [EntryForm::Ingest, EntryForm::Apply];

    pub(crate) fn label(self) -> &'static str {
        match self {
            EntryForm::Ingest => "wire 接入",
            EntryForm::Apply => "进程内重放",
        }
    }

    pub(crate) fn entry_fn(self) -> &'static str {
        match self {
            EntryForm::Ingest => "ingest_ops",
            EntryForm::Apply => "apply_ops",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BenchSyncCli {
    pub db: PathBuf,
    pub ops: Vec<usize>,
    pub warmup: usize,
    pub iterations: usize,
}

impl Default for BenchSyncCli {
    fn default() -> Self {
        BenchSyncCli {
            db: super::default_out(),
            ops: vec![100, 500, 2000],
            warmup: 1,
            iterations: 5,
        }
    }
}

pub(crate) const FLAGS: &[FlagSpec<BenchSyncCli>] = &[
    FlagSpec {
        flag: "--db <PATH>",
        help: "源库文件（默认同 generate 输出路径，须已生成；本命令不修改源库——内部建 pristine 快照，每次迭代从快照恢复）",
        apply: |cli, _flag, v| {
            cli.db = PathBuf::from(v);
            Ok(())
        },
    },
    FlagSpec {
        flag: "--ops <CSV>",
        help: "每档重放 op 条数（默认 100,500,2000；逗号分隔、保持次序；单档上限 1000000，issue #1650）",
        apply: |cli, flag, v| {
            cli.ops = parse_tier_csv(v, flag)?;
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

pub(crate) fn parse_bench_sync_args(args: &[String]) -> Result<Parsed<BenchSyncCli>, String> {
    match parse_flags(args, FLAGS)? {
        Parsed::Run(cli) if cli.iterations == 0 => Err("--iterations 至少为 1".to_string()),
        parsed => Ok(parsed),
    }
}

pub(crate) fn execute(args: &[String]) -> super::bench_common::Outcome {
    super::bench_common::execute_cli(args, parse_bench_sync_args, run)
}

#[derive(Debug, Clone)]
pub(crate) struct SyncBenchConfig {
    pub ops: Vec<usize>,
    pub warmup: usize,
    pub iterations: usize,
}

impl From<BenchSyncCli> for SyncBenchConfig {
    fn from(cli: BenchSyncCli) -> Self {
        SyncBenchConfig {
            ops: cli.ops,
            warmup: cli.warmup,
            iterations: cli.iterations,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SyncBenchMetrics {
    pub name: String,
    #[cfg_attr(not(test), allow(dead_code))]
    pub ops: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub distribution: Distribution,
    #[cfg_attr(not(test), allow(dead_code))]
    pub entry: EntryForm,
    pub min_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
    #[cfg_attr(not(test), allow(dead_code))]
    pub per_op_p95_ms: f64,
    pub context: String,
}

pub(crate) fn generate_ops(
    ops: usize,
    account_pool: &[String],
    distribution: Distribution,
    currency: &str,
    date: &str,
    schema_version: i64,
) -> Vec<SyncOp> {
    (0..ops)
        .map(|i| {
            let amount_cents = super::bench_import::BASE_AMOUNT_CENTS + i as i64;
            SyncOp {
                op_id: deterministic_uuid(&format!("bench-sync-op-{i}")),
                device_id: SOURCE_DEVICE_ID.to_string(),
                clock: i as i64 + 1,
                schema_version,
                command: DomainCommand::Transaction(TransactionCommand::Create {
                    id: deterministic_uuid(&format!("bench-sync-txn-{i}")),
                    row: NormalizedTransaction {
                        funding: Vec::new(),
                        kind: TransactionKind::Expense,
                        amount_cents,
                        currency_code: currency.to_string(),
                        amount_native_cents: amount_cents,
                        fx_rate_used: None,
                        fx_rate_source: None,
                        account_id: match distribution {
                            Distribution::Concentrated => Some(account_pool[0].clone()),
                            Distribution::Uniform => {
                                Some(account_pool[i % account_pool.len()].clone())
                            }
                        },
                        to_account_id: None,
                        funding_account_id: None,
                        category_id: None,
                        merchant_id: None,
                        policy_id: None,
                        refund_of_transaction_id: None,
                        source_order_no: None,
                        note: None,
                        date: date.to_string(),
                        purchases: Vec::new(),
                    },
                    investment: None,
                    convert: None,
                    split: None,
                }),
            }
        })
        .collect()
}

pub(crate) fn generate_wire(stream: &[SyncOp]) -> Result<Vec<String>, String> {
    stream
        .iter()
        .map(|op| serde_json::to_string(op).map_err(|e| format!("op 序列化失败：{e}")))
        .collect()
}

pub(crate) fn run(cli: BenchSyncCli) -> Result<(), String> {
    super::bench::init_tracing();
    if !cli.db.exists() {
        return Err(format!(
            "库文件不存在：{}（先运行 ledger-perf generate 生成）",
            cli.db.display()
        ));
    }
    let db = cli.db.clone();
    let cfg = SyncBenchConfig::from(cli);
    let results = run_benchmark(&db, &cfg)?;
    print_report(&db, &cfg, &results);
    Ok(())
}

pub(crate) fn run_benchmark(
    source_db: &Path,
    cfg: &SyncBenchConfig,
) -> Result<Vec<SyncBenchMetrics>, String> {
    if cfg.iterations == 0 {
        return Err("--iterations 至少为 1".to_string());
    }
    if cfg.ops.is_empty() {
        return Err("--ops 至少需要一个档位".to_string());
    }
    let guard = SnapshotPaths::create(source_db, "bench-sync")?;
    let (probe, local_schema_version) = {
        let conn = super::snapshot::open_paired(&guard.work).map_err(|e| e.to_string())?;
        let probe = probe_dataset(&conn)?;
        let v = schema_version(&conn).map_err(|e| e.to_string())?;
        (probe, v)
    };
    let mut results = Vec::new();
    for ops in &cfg.ops {
        for distribution in Distribution::ALL {
            for entry in EntryForm::ALL {
                results.push(run_cell(
                    &guard,
                    *ops,
                    distribution,
                    entry,
                    &probe,
                    local_schema_version,
                    cfg,
                )?);
            }
        }
    }
    Ok(results)
}

fn run_cell(
    guard: &SnapshotPaths,
    ops: usize,
    distribution: Distribution,
    entry: EntryForm,
    probe: &super::bench_import::DatasetProbe,
    schema_version: i64,
    cfg: &SyncBenchConfig,
) -> Result<SyncBenchMetrics, String> {
    let label = format!("同步 {ops} op·{}·{}", distribution.label(), entry.label());
    let mut durations = Vec::with_capacity(cfg.iterations);
    for iteration in 0..(cfg.warmup + cfg.iterations) {
        restore_from_snapshot(&guard.snapshot, &guard.work)?;
        let conn = super::snapshot::open_paired(&guard.work).map_err(|e| e.to_string())?;
        let stream = generate_ops(
            ops,
            &probe.account_pool,
            distribution,
            &default_currency_code(&conn).map_err(|e| e.to_string())?,
            &probe.bench_date,
            schema_version,
        );
        let wire = generate_wire(&stream)?;
        let start = Instant::now();
        let reports = match entry {
            EntryForm::Ingest => ingest_ops(&conn, &wire),
            EntryForm::Apply => apply_ops(&conn, &stream),
        }
        .map_err(|e| format!("同步重放失败[{label}]：{e}"))?;
        let elapsed = start.elapsed();
        verify_reports(&reports, ops, &label)?;
        assert_cache_matches_realtime(&conn)?;
        drop(conn);
        if iteration >= cfg.warmup {
            durations.push(elapsed);
        }
    }
    let stats = summarize(durations);
    let per_op_p95_ms = stats.p95_ms / ops as f64;
    Ok(SyncBenchMetrics {
        name: label.clone(),
        ops,
        distribution,
        entry,
        min_ms: stats.min_ms,
        avg_ms: stats.avg_ms,
        p95_ms: stats.p95_ms,
        per_op_p95_ms,
        context: format!(
            "expense create ×{ops} op，入口 {}，基准日期 {}；单 op 均摊 p95 {per_op_p95_ms:.3} ms/op",
            entry.entry_fn(),
            probe.bench_date
        ),
    })
}

fn verify_reports(reports: &[ApplyReport], ops: usize, label: &str) -> Result<(), String> {
    if reports.len() != ops {
        return Err(format!(
            "[{label}] 报告数 {} ≠ op 条数 {ops}，量测无效",
            reports.len()
        ));
    }
    if let Some(bad) = reports.iter().find(|r| r.outcome != OpOutcome::Applied) {
        return Err(format!(
            "[{label}] 量测有效性前置失败：存在非 Applied 归宿（op_id={} outcome={:?}）——op 流身份全新且全序前置，重放应逐条 Applied",
            bad.op_id, bad.outcome
        ));
    }
    Ok(())
}

impl MetricTableRow for SyncBenchMetrics {
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

fn print_report(db: &Path, cfg: &SyncBenchConfig, results: &[SyncBenchMetrics]) {
    println!("ledger-perf bench-sync —— 同步重放写基准报告（纯观测，无门禁，人工判读）");
    println!(
        "源库：{}（本命令不修改源库；每次迭代从 pristine 快照恢复，数据集规模固定）",
        db.display()
    );
    println!(
        "矩阵：op 档 {:?} × 分布 [同账户集中, 多账户均匀] × 入口 [wire 接入, 进程内重放]；预热 {} 次 × 迭代 {} 次",
        cfg.ops, cfg.warmup, cfg.iterations
    );
    println!(
        "统计口径：最近秩 p95，n<20 时 p95=max 无分位数分辨力（同 ADR-0068 口径），要真分位数请提高 --iterations"
    );
    println!();
    print_metric_table(results);
    println!();
    println!("刷新段归因：每条 op 的命令落库都在同一事务内触发一次余额缓存整体重算；");
    println!(
        "该段耗时经既有耗时日志按语句归因：RUST_LOG=debug 重跑后按「INSERT INTO account_balance_cache」检索全量明细（DEBUG 级），慢查询（≥100ms）以 warn「慢查询」直接可见。"
    );
}
