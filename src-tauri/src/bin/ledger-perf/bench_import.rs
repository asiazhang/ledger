//! bench-import 子命令：批量导入写基准（issue #532）。
//!
//! 对 generate 产出的库量测「批量导入固定行数」的写耗时，检验 #519 grilling
//! 留下的 O(N²) 假设：ADR-0067 之后每一行都在同一事务内对受影响账户整体重算
//! 余额缓存，同账户集中导入的行扫描量是否构成可感知浪费由数裁决。
//!
//! 量测矩阵：行数档（默认 50/100/200，按月导入真实量级：轻量/典型/上限月）
//! × 两种分布——
//! 「同账户集中」全部落首账户（最坏形态）、「多账户均匀」按账户池轮转
//! （最好形态）；报告每单元总耗时 min/avg/p95 与单行均摊 p95（最近秩法，
//! n<20 时 p95=max 无分位数分辨力，同 ADR-0068 统计口径；要真分位数提高
//! `--iterations`）。
//!
//! 形态与读基准（[`super::bench`]，ADR-0062）同构、不另起炉灶：
//!
//! - 唯一接缝：导入走批量编排权威 [`TransactionBatch::run`]（HTTP 批量导入与
//!   IPC 批量创建共享的同一入口，每行经行为层创建编排 → Writer 接缝落库 →
//!   同事务余额缓存整体重算），与生产导入同一 SQL 路径；前置探测与导入后的
//!   一致性断言走现有 pub 查询函数，本模块零手写业务 SQL。
//! - 每次迭代从 pristine 快照恢复（源库复制 + 快照零改动）：迭代间数据集规模
//!   固定，p95 是同一状态的真分位数——这是「量测结论可复现」的落点。快照机制
//!   收口在 [`super::snapshot`]（与 bench-sync 共用），含恢复后预写提交冲净
//!   拷贝写回的量测有效性处理。
//! - 刷新段开销不新增观测代码：余额缓存重算（`INSERT INTO
//!   account_balance_cache …`）的单条 SQL 耗时由连接工厂自动挂载的耗时日志
//!   （perf_trace）全覆盖——慢查询（≥100ms）以 warn「慢查询」可见，全量明细需
//!   DEBUG 级日志；报告不拆分该段，归因在日志中按语句检索。
//! - 正确性底线（issue #532 测试决策）：每次导入完成后断言余额缓存与实时计算
//!   逐账户一致（读出口 + 审计口径同一对 pub 函数），断言失败即量测作废——
//!   基准必须跑在正确路径而非坏缓存路径上。
//! - 无门禁判定：阈值待 CI 基线（perf-bench 每日以最大档 200 行 × 两分布
//!   观测、只记录进 Summary；立阈值时修订 ADR-0068）；check.sh 不跑本基准。
//!   不修改任何生产写入路径，置脏/信号等壳层职责（`write_entry`）不属
//!   被测量，不在基准内重演。
//!
//! 脚手架（参数解析循环 / 档位 CSV 解析 / 统计块 / 报告表列）已收口到
//! [`super::bench_common`]（issue #1650），本模块只持矩阵与量测语义。

use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{Days, NaiveDate};
use rusqlite::Connection;

use ledger_accounts as accounts;
use ledger_accounts::{Account, AccountType, balance};
use ledger_infra::db::open_connection;
use ledger_reports as reports_domain;
use ledger_transaction::amount::{TransactionKind, default_currency_code};
use ledger_transaction::{BatchOutcome, TransactionBatch, TransactionInput};

use super::bench_common::{CliArgs, MetricTableRow, parse_tier_csv, print_metric_table, summarize};
use super::snapshot::{SnapshotPaths, restore_from_snapshot};

/// 基准行金额基数（分）：行金额逐行 +1 递增，保证批内去重身份全异
/// （dedup=true 时行行真写、无一行被去重跳过——量测有效性前置）。
pub(crate) const BASE_AMOUNT_CENTS: i64 = 1_000;

/// 导入行的分布形态（issue #532：量测矩阵的第二轴）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Distribution {
    /// 同账户集中：全部行落账户池首账户——受影响账户的流水随导入线性增长，
    /// O(N²) 假设的正面检验形态。
    Concentrated,
    /// 多账户均匀：行按账户池顺序轮转——账户级负载摊薄的最好情况形态。
    Uniform,
}

impl Distribution {
    /// 矩阵展开次序（外层行数档、内层分布）的稳定清单。
    pub(crate) const ALL: [Distribution; 2] = [Distribution::Concentrated, Distribution::Uniform];

    /// 报告与指标名用的人读标签。
    pub(crate) fn label(self) -> &'static str {
        match self {
            Distribution::Concentrated => "同账户集中",
            Distribution::Uniform => "多账户均匀",
        }
    }
}

/// bench-import 参数（解析后形态；默认值见 [`Default`]）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BenchImportCli {
    /// 源库文件（须已由 generate 产出；本命令不修改源库）。
    pub db: PathBuf,
    /// 每档导入行数（量测矩阵第一轴，默认按月导入真实量级：轻量/典型/上限月）。
    pub rows: Vec<usize>,
    /// 批量导入去重开关（默认 true，与 HTTP 批量导入生产默认一致）。
    pub dedup: bool,
    /// 每档预热次数（不计入统计；导入迭代成本高，默认 1）。
    pub warmup: usize,
    /// 每档计时迭代次数（默认 5；每次迭代从 pristine 快照恢复）。
    pub iterations: usize,
}

impl Default for BenchImportCli {
    fn default() -> Self {
        BenchImportCli {
            db: super::default_out(),
            // 三档按 AI 导入真实量级校准（维护者确认：按月导入，单月账单最多上百笔）：
            // 50 = 轻量月、100 = 典型月、200 = 上限月；不预设万级单批——真实场景不存在。
            rows: vec![50, 100, 200],
            dedup: true,
            warmup: 1,
            iterations: 5,
        }
    }
}

/// 参数解析结果：运行参数或帮助请求。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParsedBenchImport {
    Run(BenchImportCli),
    Help,
}

/// 手写参数解析（零新增依赖；循环机制与档位解析收口在 [`super::bench_common`]，
/// issue #1650）。返回 Err(消息) 表示用法错误。
pub(crate) fn parse_bench_import_args(args: &[String]) -> Result<ParsedBenchImport, String> {
    let mut cli = BenchImportCli::default();
    let mut it = CliArgs::new(args);
    while let Some(f) = it.next_flag() {
        match f.flag {
            "--db" => {
                cli.db = PathBuf::from(it.value(f)?);
            }
            "--rows" => {
                cli.rows = parse_tier_csv(&it.value(f)?, "--rows")?;
            }
            "--dedup" => {
                let v = it.value(f)?;
                cli.dedup = parse_bool(&v)?;
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
            "-h" | "--help" => return Ok(ParsedBenchImport::Help),
            other => return Err(format!("未知参数 {other:?}")),
        }
    }
    if cli.iterations == 0 {
        return Err("--iterations 至少为 1".to_string());
    }
    Ok(ParsedBenchImport::Run(cli))
}

/// 布尔参数解析（`--dedup` 只认 true/false，拒绝顺手 coercion 的歧义形态）。
fn parse_bool(v: &str) -> Result<bool, String> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("布尔参数需要 true/false，得到 {other:?}")),
    }
}

/// 基准运行配置（测试可注入小参数；与 [`BenchImportCli`] 的矩阵字段一一对应）。
#[derive(Debug, Clone)]
pub(crate) struct ImportBenchConfig {
    pub rows: Vec<usize>,
    pub dedup: bool,
    pub warmup: usize,
    pub iterations: usize,
}

/// 单元（行数档 × 分布）的量测结果（人读报告行 + 冒烟断言面）。
#[derive(Debug, Clone)]
pub(crate) struct ImportBenchMetrics {
    /// 指标名 = 「导入 {rows} 行·{分布}」（动态生成，冒烟测试按默认矩阵钉住）。
    pub name: String,
    /// 行数档（报告列 + 断言面）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub rows: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub distribution: Distribution,
    pub min_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
    /// 单行均摊 p95 = 批次 p95 ÷ 行数（批内单行耗时不新增观测代码，见模块头）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub per_row_p95_ms: f64,
    /// 规模备注：分布、去重开关、基准日期与单行均摊口径。
    pub context: String,
}

/// 基准行生成（纯函数，确定性可测）：expense 行，金额逐行 +1 递增
/// （批内去重身份全异），账户按分布落位，日期统一为基准日期。
///
/// 账户池必须非空（调用方 [`probe_dataset`] 已保证）；币种随池子口径传入
/// （池子已按本位币过滤，传入值即 [`default_currency_code`]）。
pub(crate) fn generate_inputs(
    rows: usize,
    account_pool: &[String],
    distribution: Distribution,
    currency: &str,
    date: &str,
) -> Vec<TransactionInput> {
    (0..rows)
        .map(|i| TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents: BASE_AMOUNT_CENTS + i as i64,
            currency_code: currency.to_string(),
            account_id: match distribution {
                Distribution::Concentrated => account_pool[0].clone(),
                Distribution::Uniform => account_pool[i % account_pool.len()].clone(),
            },
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
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
        })
        .collect()
}

/// 导入基准账户池：非投资 + 本位币（`list_accounts` 已滤软删与黑洞账户）。
///
/// 投资户排除：expense 落投资户不触发 buy/sell 副作用但偏离画像语义；外币户
/// 排除：折算要查汇率、行金额口径随币种漂移，两者都不属被测的刷新粒度问题。
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

/// 正确性底线（issue #532 测试决策）：余额缓存与实时计算逐账户一致。
///
/// 断言走与生产读出口/审计完全相同的两个 pub 函数——缓存侧
/// [`accounts::list_account_balances_with_visibility`]（读缓存，缓存行缺失
/// 报码化错误）、实时侧 [`balance::compute_all_balances_with_visibility`]；
/// 任何漂移让量测作废（基准不许跑在坏缓存路径上）。
///
/// 与共享断言库
/// [`tauri_app_lib::test_support::assert_balance_cache_matches_realtime`]
/// 的分工边界（issue #776 裁决，甲案不归一）：两份实现刻意不同、各自长期
/// 存在，非漏收编——本函数属基准工具（开发侧基础设施，ADR-0062 不产生领域
/// 术语）的正确性底线：Result 契约（失败给可读报告让量测作废，非 panic；bin
/// 生产构建在 ADR-0060 六件套 deny 下，panic 形态需另行豁免论证）+
/// 生产读出口接缝（要验证的正是生产读路径没跑在坏缓存上）；共享版本属
/// 测试域对拍：panic 契约 + 逐账户 `compute_balance` 审计口径（ADR-0084
/// 决策 6 收编清单不含本函数；决策 1 准入规则「只收跨 ≥2 域重复的夹具」
/// 同理排除）。
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

/// 数据集前置探测结果：基准日期与账户池（bench-import 与 bench-sync 共用，
/// #1628 收编：两类写基准消费同一 generated 数据集的同一前置）。
pub(crate) struct DatasetProbe {
    /// 基准行统一日期 = 数据集最大交易日期次日——与既有全部交易不共日期，
    /// 去重身份不可能命中既有数据（量测有效性的确定性保障）。
    pub(crate) bench_date: String,
    pub(crate) account_pool: Vec<String>,
}

/// 前置探测（走现有查询函数，与读基准同款前置，不计入任何量测）：
/// 日期极值推基准日期、账户清单推导入账户池。
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

/// 入口：打开源库校验、建快照、跑量测矩阵、打印人读报告（无门禁，纯观测）。
pub(crate) fn run(cli: BenchImportCli) -> Result<(), String> {
    super::bench::init_tracing();
    if !cli.db.exists() {
        return Err(format!(
            "库文件不存在：{}（先运行 ledger-perf generate 生成）",
            cli.db.display()
        ));
    }
    let cfg = ImportBenchConfig {
        rows: cli.rows.clone(),
        dedup: cli.dedup,
        warmup: cli.warmup,
        iterations: cli.iterations,
    };
    let results = run_benchmark(&cli.db, &cfg)?;
    print_report(&cli.db, &cfg, &results);
    Ok(())
}

/// 量测核心（测试接缝）：建 pristine 快照 → 探测 → 矩阵逐单元
/// 「恢复快照 → 导入 → 断言 → 计时」→ 指标清单。
///
/// 单元展开次序稳定：行数档外层、分布内层（[`Distribution::ALL`]）。
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
    // 探测在工作库上做（快照的副本，探测的读路径与正式迭代完全一致）。
    let probe = {
        let conn = open_connection(&guard.work).map_err(|e| e.to_string())?;
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

/// 跑一个矩阵单元：`预热+迭代` 次恢复快照导入，计时窗口只包
/// [`TransactionBatch::run`]（批次事务 + 逐行创建/去重 + 提交后
/// `PRAGMA optimize`——生产批量导入的完整提交路径）；导入后的结果核验与
/// 缓存一致性断言在计时窗口外，不稀释量测。
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
        let conn = open_connection(&guard.work).map_err(|e| e.to_string())?;
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

/// 量测有效性前置核验：整批行行真写（数量对、全部 success、无一命中去重）。
/// 任一违反即基准量到的不是「N 行导入」，结果无效。
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

/// 人读报告行接口实现（issue #1650 收口）：交共享表列打印，不再自持排版。
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

/// 人读表格输出：与读基准同款列形（min / avg / p95 + 规模备注），无门禁行；
/// 表列排版收口在 [`print_metric_table`]（issue #1650）。
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
