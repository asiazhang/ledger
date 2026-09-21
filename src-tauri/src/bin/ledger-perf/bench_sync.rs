//! bench-sync 子命令：同步重放写基准（issue #1628）。
//!
//! 对 generate 产出的库量测「apply_ops 循环逐条合并外来 op 流」的本地写耗时
//! （引擎重放循环 `ledger_sync_engine::apply_ops` / `ingest_ops`）：多端同步是
//! 主航线，op 流在 50 万笔库上的写放大此前无基准守护——每条 op 一次独立事务
//! （幂等查日志、位点门、LWW 裁决、命令落库、op 落日志与位点推进同事务原子）
//! 外加受影响账户余额缓存整体重算（ADR-0067），放大系数只由数裁决。
//!
//! 网络边界豁免（ADR-0068，2026-10 grilling 会话 Q5 决策）：S3 通道传输是外部
//! 网络往返，不属回归面——本基准剥离网络、只量测本地部分。op 流按「A 端
//! `read_ops` → B 端 `apply_ops`」的双端消费形态确定性构造（`SyncOp` 结构体即
//! `read_ops` 的返回形态；wire 形态是其 serde 通道搬运形态），重放走
//! `ingest_ops`（wire 形态，Transport 通道上的生产接缝）与 `apply_ops`（已解析
//! 形态，进程内工具与测试通道）两个权威入口，与生产合并同一 SQL 路径。
//!
//! 量测矩阵（照 bench-import 先例，issue #532）：op 数量档（默认 100/500/2000，
//! 按同步轮真实量级：日常轮 / 离线一周积压 / 离线一月上限形态）× 两种落位分布
//! ——「同账户集中」全部落账户池首账户（余额重算扫描量随 op 线性增长的写放大
//! 正面形态）、「多账户均匀」按账户池轮转（账户级负载摊薄的最好形态）× 两个
//! 重放入口；报告每单元总耗时 min/avg/p95 与单 op 均摊 p95（最近秩法，n<20 时
//! p95=max 无分位数分辨力，同 ADR-0068 统计口径；要真分位数提高 `--iterations`）。
//!
//! 形态与 bench-import 同构、不另起炉灶：
//!
//! - 每次迭代从 pristine 快照恢复（源库复制 + 快照零改动）：迭代间数据集规模
//!   固定，p95 是同一状态的真分位数。快照机制收口在 [`super::snapshot`]（与
//!   bench-import 共用），含恢复后预写提交冲净拷贝写回的量测有效性处理。
//! - op 流确定性生成（纯函数）：expense 创建命令（`TransactionCommand::Create`），
//!   op_id / 交易 id 由 `deterministic_uuid` 派生（同参重跑同流、逐条身份全新），
//!   逻辑时钟自 1 单调递增（跨端全序前置），schema 版本取目标库 user_version
//!   （schema 偏斜判定不触发挂起）。本位币行无折算（native 金额 = 行金额、无
//!   留痕），重放端不重折算、不依赖汇率表。生成库不含同步元数据（generate
//!   直写不入日志）：幂等查日志、位点门与 LWW 裁决走空表索引路径，量测主体是
//!   命令落库与余额重算。
//! - 前置探测与正确性底线复用 bench-import 的共享件：账户池口径（本位币非投资
//!   户——投资副作用与折算不属被测路径）、基准日期（数据集最大交易日期次日，
//!   与既有交易不共日期）与「余额缓存 = 实时计算」逐账户断言（断言失败即量测
//!   作废——基准必须跑在正确路径而非坏缓存路径上）。
//! - 计时窗口只包权威入口调用（全序排序 + 逐条重放完整提交路径）；op 流生成、
//!   wire 序列化与迭代后的结果核验在窗口外，不稀释量测。逐条报告必须是
//!   Applied（op 流身份全新且全序前置，Skipped / Superseded / Deduped / Parked
//!   任一出现都说明量到的不是「N 条新 op 重放」，结果作废）。
//! - 无门禁判定：写基准阈值未立（同 bench-import：本地/CI 磁盘口径差异未测，
//!   待 CI 基线分布稳定后修订 ADR-0068 另立判据）；perf-bench 每日以最大档 ×
//!   两分布 × 两入口观测、只记录进 Summary；check.sh 不跑本基准。不修改任何
//!   生产写入路径，置脏/信号等壳层职责不属被测量，不在基准内重演。
//!
//! 脚手架（参数解析循环 / 档位 CSV 解析 / 统计块 / 报告表列）已收口到
//! [`super::bench_common`]（issue #1650），本模块只持矩阵与量测语义。

use std::path::{Path, PathBuf};
use std::time::Instant;

use ledger_infra::db::{deterministic_uuid, open_connection, schema_version};
use ledger_sync_engine::{ApplyReport, DomainCommand, OpOutcome, SyncOp, apply_ops, ingest_ops};
use ledger_transaction::amount::{TransactionKind, default_currency_code};
use ledger_transaction::{NormalizedTransaction, TransactionCommand};

use super::bench_common::{CliArgs, MetricTableRow, parse_tier_csv, print_metric_table, summarize};
use super::bench_import::{Distribution, assert_cache_matches_realtime, probe_dataset};
use super::snapshot::{SnapshotPaths, restore_from_snapshot};

/// op 流源端设备标识（A 端）：固定值，op 信封的 device_id 字段进跨端全序
/// （与位点表键），恒定保证同参重跑同流。
pub(crate) const SOURCE_DEVICE_ID: &str = "ledger-perf-bench-sync-device-a";

/// 重放入口形态（量测矩阵第三轴）：两个权威入口各自一条计时路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryForm {
    /// wire 形态：op 信封 JSON 文本逐条解析后重放（Transport 通道上的生产
    /// 接缝，解析成本计入量测）。
    Ingest,
    /// 已解析形态：`SyncOp` 结构体直接重放（进程内工具与测试通道）。
    Apply,
}

impl EntryForm {
    /// 矩阵展开次序（分布内层）的稳定清单：生产通道形态在前。
    pub(crate) const ALL: [EntryForm; 2] = [EntryForm::Ingest, EntryForm::Apply];

    /// 报告与指标名用的人读标签。
    pub(crate) fn label(self) -> &'static str {
        match self {
            EntryForm::Ingest => "wire 接入",
            EntryForm::Apply => "进程内重放",
        }
    }

    /// 规模备注里的权威入口锚点（函数名，量测路径的人读归因）。
    pub(crate) fn entry_fn(self) -> &'static str {
        match self {
            EntryForm::Ingest => "ingest_ops",
            EntryForm::Apply => "apply_ops",
        }
    }
}

/// bench-sync 参数（解析后形态；默认值见 [`Default`]）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BenchSyncCli {
    /// 源库文件（须已由 generate 产出；本命令不修改源库）。
    pub db: PathBuf,
    /// 每档 op 条数（量测矩阵第一轴，默认按同步轮真实量级：日常轮/离线一周
    /// 积压/离线一月上限形态）。
    pub ops: Vec<usize>,
    /// 每档预热次数（不计入统计；同步重放迭代成本高，默认 1）。
    pub warmup: usize,
    /// 每档计时迭代次数（默认 5；每次迭代从 pristine 快照恢复）。
    pub iterations: usize,
}

impl Default for BenchSyncCli {
    fn default() -> Self {
        BenchSyncCli {
            db: super::default_out(),
            // 三档按同步轮真实量级校准：多端合计的日常轮在百笔内（100）、离线
            // 一周积压约数百笔（500）、离线一月的上限形态约两千笔（2000）。
            ops: vec![100, 500, 2000],
            warmup: 1,
            iterations: 5,
        }
    }
}

/// 参数解析结果：运行参数或帮助请求。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParsedBenchSync {
    Run(BenchSyncCli),
    Help,
}

/// 手写参数解析（零新增依赖；循环机制与档位解析收口在 [`super::bench_common`]，
/// issue #1650）。返回 Err(消息) 表示用法错误。
pub(crate) fn parse_bench_sync_args(args: &[String]) -> Result<ParsedBenchSync, String> {
    let mut cli = BenchSyncCli::default();
    let mut it = CliArgs::new(args);
    while let Some(f) = it.next_flag() {
        match f.flag {
            "--db" => {
                cli.db = PathBuf::from(it.value(f.inline_value, "--db")?);
            }
            "--ops" => {
                cli.ops = parse_tier_csv(&it.value(f.inline_value, "--ops")?, "--ops")?;
            }
            "--warmup" => {
                let v = it.value(f.inline_value, "--warmup")?;
                cli.warmup = v
                    .parse::<usize>()
                    .map_err(|_| format!("--warmup 需要非负整数，得到 {v:?}"))?;
            }
            "--iterations" => {
                let v = it.value(f.inline_value, "--iterations")?;
                cli.iterations = v
                    .parse::<usize>()
                    .map_err(|_| format!("--iterations 需要非负整数，得到 {v:?}"))?;
            }
            "-h" | "--help" => return Ok(ParsedBenchSync::Help),
            other => return Err(format!("未知参数 {other:?}")),
        }
    }
    if cli.iterations == 0 {
        return Err("--iterations 至少为 1".to_string());
    }
    Ok(ParsedBenchSync::Run(cli))
}

/// 基准运行配置（测试可注入小参数；与 [`BenchSyncCli`] 的矩阵字段一一对应）。
#[derive(Debug, Clone)]
pub(crate) struct SyncBenchConfig {
    pub ops: Vec<usize>,
    pub warmup: usize,
    pub iterations: usize,
}

/// 单元（op 档 × 分布 × 入口）的量测结果（人读报告行 + 冒烟断言面）。
#[derive(Debug, Clone)]
pub(crate) struct SyncBenchMetrics {
    /// 指标名 = 「同步 {ops} op·{分布}·{入口}」（动态生成，冒烟测试按默认矩阵钉住）。
    pub name: String,
    /// op 档（报告列 + 断言面）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub ops: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub distribution: Distribution,
    #[cfg_attr(not(test), allow(dead_code))]
    pub entry: EntryForm,
    pub min_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
    /// 单 op 均摊 p95 = 单元 p95 ÷ op 条数（逐条重放耗时不新增观测代码，见模块头）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub per_op_p95_ms: f64,
    /// 规模备注：分布、入口、基准日期与单 op 均摊口径。
    pub context: String,
}

/// op 流生成（纯函数，确定性可测）：expense 创建命令流，双端消费形态里 A 端
/// `read_ops` 的返回形态（A 端产 op 的内容由本函数按固定画像替代——产端写路径
/// 不属被测量，剥网络的同时剥掉产端成本）。
///
/// 账户池必须非空（调用方 [`probe_dataset`] 已保证）；币种随池子口径传入
/// （池子已按本位币过滤，传入值即 [`default_currency_code`]）；schema 版本
/// 由调用方从目标库读取传入（偏斜判定不触发挂起）。
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
            // 金额逐条 +1 递增：逐条金额全异（与 bench-import 同款量测有效性
            // 前置——行行真写，无一条命中任何幂等身份）。
            let amount_cents = super::bench_import::BASE_AMOUNT_CENTS + i as i64;
            SyncOp {
                op_id: deterministic_uuid(&format!("bench-sync-op-{i}")),
                device_id: SOURCE_DEVICE_ID.to_string(),
                clock: i as i64 + 1,
                schema_version,
                command: DomainCommand::Transaction(TransactionCommand::Create {
                    id: deterministic_uuid(&format!("bench-sync-txn-{i}")),
                    row: NormalizedTransaction {
                        kind: TransactionKind::Expense,
                        amount_cents,
                        currency_code: currency.to_string(),
                        // 本位币行无折算：native 金额 = 行金额、无留痕（源端
                        // 折算语义只涉外币行，重放不重折算）。
                        amount_native_cents: amount_cents,
                        fx_rate_used: None,
                        fx_rate_source: None,
                        account_id: match distribution {
                            Distribution::Concentrated => account_pool[0].clone(),
                            Distribution::Uniform => account_pool[i % account_pool.len()].clone(),
                        },
                        to_account_id: None,
                        funding_account_id: None,
                        category_id: None,
                        merchant_id: None,
                        policy_id: None,
                        refund_of_transaction_id: None,
                        note: None,
                        date: date.to_string(),
                    },
                    investment: None,
                    convert: None,
                    split: None,
                }),
            }
        })
        .collect()
}

/// op 流的 wire 形态（双端消费形态里通道上的搬运形态）：逐条 serde 序列化，
/// 与 A 端 `read_ops` 产出经通道传输的报文同形（`ingest_ops` 的输入形态）。
pub(crate) fn generate_wire(stream: &[SyncOp]) -> Result<Vec<String>, String> {
    stream
        .iter()
        .map(|op| serde_json::to_string(op).map_err(|e| format!("op 序列化失败：{e}")))
        .collect()
}

/// 入口：打开源库校验、建快照、跑量测矩阵、打印人读报告（无门禁，纯观测）。
pub(crate) fn run(cli: BenchSyncCli) -> Result<(), String> {
    super::bench::init_tracing();
    if !cli.db.exists() {
        return Err(format!(
            "库文件不存在：{}（先运行 ledger-perf generate 生成）",
            cli.db.display()
        ));
    }
    let cfg = SyncBenchConfig {
        ops: cli.ops.clone(),
        warmup: cli.warmup,
        iterations: cli.iterations,
    };
    let results = run_benchmark(&cli.db, &cfg)?;
    print_report(&cli.db, &cfg, &results);
    Ok(())
}

/// 量测核心（测试接缝）：建 pristine 快照 → 探测 → 矩阵逐单元
/// 「恢复快照 → 重放 → 断言 → 计时」→ 指标清单。
///
/// 单元展开次序稳定：op 档外层、分布中层、入口内层
/// （[`Distribution::ALL`] × [`EntryForm::ALL`]）。
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
    // 探测在工作库上做（快照的副本，探测的读路径与正式迭代完全一致）；
    // schema 版本同次读取（op 信封字段的生成依据）。
    let (probe, local_schema_version) = {
        let conn = open_connection(&guard.work).map_err(|e| e.to_string())?;
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

/// 跑一个矩阵单元：`预热+迭代` 次恢复快照重放，计时窗口只包权威入口调用
/// （全序排序 + 逐条重放：幂等查日志 → 位点门 → LWW 裁决 → 命令落库 → op
/// 落日志与位点推进，每条 op 独立事务的完整提交路径）；重放后的结果核验与
/// 缓存一致性断言在计时窗口外，不稀释量测。
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
        let conn = open_connection(&guard.work).map_err(|e| e.to_string())?;
        // op 流与 wire 形态每次迭代重新生成（纯函数、确定性，量测窗口外）：
        // 币种随连接口径读取，与 bench-import 的行生成同形。
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

/// 量测有效性前置核验：整批 op 逐条 Applied、数量一致。任何 Skipped（幂等
/// 或位点命中）/ Superseded（LWW）/ Deduped / Parked 归宿都说明量到的不是
/// 「N 条新 op 重放」，结果无效。
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

/// 人读报告行接口实现（issue #1650 收口）：交共享表列打印，不再自持排版。
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

/// 人读表格输出：与 bench-import 同款列形（min / avg / p95 + 规模备注），无门禁行；
/// 表列排版收口在 [`print_metric_table`]（issue #1650）。
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
