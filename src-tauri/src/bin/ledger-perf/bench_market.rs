//! bench-market 子命令：行情/价格历史批量 upsert 写基准（issue #1629）。
//!
//! 对 generate 产出的库量测「行情同步与价格历史补全的本地批量 upsert」耗时
//! （价格写入单点 `ledger_investment::prices::upsert_market_price` /
//! `upsert_price_history`）：行情换源大批复接线（#1542–#1568）与价格历史后台
//! 补全（#1375）引入大体量本地批量 upsert——换源后整根重刷（每只标的一个
//! 事务：现价行 + 全窗口周采样逐点 upsert）、历史补全一轮排空（逐只近两年
//! 整根回填），50 万笔库上的批量面此前无基准守护。
//!
//! 网络边界豁免（ADR-0068，随 #1628 成文）：腾讯/新浪等行情源抓取是外部网络
//! 往返，不属回归面——本基准剥离网络、只量本地落库部分。价格点流确定性生成
//! （纯函数替代取数层产出），落库走投资域价格写入单点（投资域全部价格写入
//! 通道共用，issue #291 收口，不另写第二份 upsert SQL），与生产同步/回填通道
//! 同一 SQL 路径；事务边界照生产形态「一只标的一个事务」（[`ensure_transaction`]，
//! `write_weekly_price_history` 接缝契约：周采样落库单点不开事务，调用方包事务）。
//!
//! 量测矩阵（照 bench-import / bench-sync 先例）：周采样点数档（默认 104/1040/
//! 5200，按同步落库真实量级：单只标的近两年整根首刷 / 部分批量重刷（约十只
//! × 近两年）/ 全库价格线整根重刷（全部标的 × 五年全窗口，与生成库价格线
//! 行数同量级））× 两种落位分布——「单标的集中」全部点落首标的（同一索引
//! 子树内连续插入的写放大正面形态）、「多标的均匀」按池依次分块、每只获得
//! 一段连续周序列（镜像生产「逐只整根回填 / 一轮排空」形状，索引子树逐只
//! 摊薄的最好形态）；报告每单元总耗时 min/
//! avg/p95 与单点均摊 p95（最近秩法，n<20 时 p95=max 无分位数分辨力，同
//! ADR-0068 统计口径；要真分位数提高 `--iterations`）。
//!
//! 形态与 bench-sync 同构、不另起炉灶（复用 #1628 脚手架）：
//!
//! - 每次迭代从 pristine 快照恢复（快照机制收口在 [`super::snapshot`]，与
//!   bench-import / bench-sync 共用）：迭代间数据集规模固定，p95 是同一状态
//!   的真分位数；源库全程零改动。
//! - 点流确定性生成（纯函数）：现价行 + 周采样点，采样周自价格历史最大采样
//!   日次周起（新周追加，不与既有周冲突——行行真写、无一条命中整周覆盖），
//!   采样日取当周周五（生成器「周一进位、取当周周五」同款）、价格逐点递增
//!   （全异可核验）、币种与来源标记随标的字典（来源按实际取数源：场内
//!   `tencent`、场外基金 `sina`，ADR-0130 决策 7）。现价形态照生产通道：基金
//!   priced_at = 净值日期（兼任 nav_date），场内 priced_at = 写入时刻、无
//!   净值日期。
//! - 标的池口径：价格写入通道成员中的同步采集面（行情 / 净值通道）——手动
//!   报价是用户单笔动作不属大批量形态，恒定价格不落历史行、无来源不参与
//!   价格写入（词汇表 PriceHistory 覆盖面与 PriceChannel 闭集）。
//! - 前置探测走现有 pub 查询函数（与读基准同款前置，不计入任何量测）；
//!   正确性底线（照 bench-import 既有断言模式）：每次落库后断言价格历史行数
//!   恰增点数（行行真写，无幂等命中/覆盖）且触达标的的现价行与点流一致，
//!   断言失败即量测作废。快照恢复闭环：每次迭代前核对基线行数与 pristine
//!   快照一致——上一迭代的写副作用若有残留，本轮断言即红（连续迭代无残留
//!   是量测有效性的前提）。
//! - 计时窗口只包逐标的落库（事务壳 + 现价 upsert + 周采样逐点 upsert）；
//!   点流生成与迭代后的核验在窗口外，不稀释量测。
//! - 无门禁判定：写基准阈值未立（同 bench-import / bench-sync：本地/CI 磁盘
//!   口径差异未测，待 CI 基线分布稳定后修订 ADR-0068 另立判据）；perf-bench
//!   每日以最大档 5200 点 × 两分布观测、只记录进 Summary；check.sh 不跑本
//!   基准。不修改任何生产写入路径；行情源抓取（网络）与解析不属被测量，
//!   不在基准内重演。
//!
//! 脚手架（参数解析循环 / 档位 CSV 解析 / 统计块 / 报告表列）已收口到
//! [`super::bench_common`]（issue #1650），本模块只持矩阵与量测语义；点数档
//! 上界（[`MAX_TIER`]）在解析层拒绝极端档位，采样日运算不触 chrono 日期
//! 越界 panic。

use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{Datelike, Days, NaiveDate};
use rusqlite::Connection;

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{now_iso, open_connection};
use ledger_investment::prices::{
    MarketPriceWrite, SINA_PRICE_SOURCE, TENCENT_PRICE_SOURCE, upsert_market_price,
    upsert_price_history,
};
use ledger_investment::{InstrumentListFilter, PriceChannel};
use ledger_reports as reports_domain;

use super::bench_common::{CliArgs, MetricTableRow, parse_tier_csv, print_metric_table, summarize};
use super::snapshot::{SnapshotPaths, restore_from_snapshot};

/// 落库点流的来源标记基准价（万分之一元，ADR-0038 价格刻度）：价格逐点 +1
/// 递增，保证批内价格全异（行行真写可核验——点流身份全新，无一条同周同值）。
pub(crate) const BASE_PRICE_UNITS: i64 = 1_000_000;

/// 落位分布（量测矩阵第二轴，照 bench-import 的 Distribution 先例；标的池
/// 维度与账户池维度语义不同，不共用枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spread {
    /// 单标的集中：全部点落首标的——同一 (instrument, week) 索引子树内连续
    /// 插入，写放大正面检验形态。
    Concentrated,
    /// 多标的均匀：点按标的池轮转——每只获得同一段连续周序列，索引子树
    /// 逐只摊薄的最好情况形态。
    Uniform,
}

impl Spread {
    /// 矩阵展开次序（外层点数档、内层分布）的稳定清单。
    pub(crate) const ALL: [Spread; 2] = [Spread::Concentrated, Spread::Uniform];

    /// 报告与指标名用的人读标签。
    pub(crate) fn label(self) -> &'static str {
        match self {
            Spread::Concentrated => "单标的集中",
            Spread::Uniform => "多标的均匀",
        }
    }
}

/// bench-market 参数（解析后形态；默认值见 [`Default`]）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BenchMarketCli {
    /// 源库文件（须已由 generate 产出；本命令不修改源库）。
    pub db: PathBuf,
    /// 每档周采样点数（量测矩阵第一轴，默认按同步落库真实量级：单只近两年
    /// 整根 / 部分批量重刷 / 全库价格线量级）。
    pub points: Vec<usize>,
    /// 每档预热次数（不计入统计；批量落库迭代成本高，默认 1）。
    pub warmup: usize,
    /// 每档计时迭代次数（默认 5；每次迭代从 pristine 快照恢复）。
    pub iterations: usize,
}

impl Default for BenchMarketCli {
    fn default() -> Self {
        BenchMarketCli {
            db: super::default_out(),
            // 三档按同步落库真实量级校准：104 = 单只标的近两年整根首刷
            // （历史补全单只形态，ADR-0122）；1040 = 部分批量重刷（约十只
            // 标的的近两年整根）；5200 = 全库价格线整根重刷（全部 20 标的
            // × 五年全窗口，与生成库价格线行数同量级——换源大批复的极限形态）。
            points: vec![104, 1040, 5200],
            warmup: 1,
            iterations: 5,
        }
    }
}

/// 参数解析结果：运行参数或帮助请求。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParsedBenchMarket {
    Run(BenchMarketCli),
    Help,
}

/// 手写参数解析（零新增依赖；循环机制与档位解析收口在 [`super::bench_common`]，
/// issue #1650）。返回 Err(消息) 表示用法错误。
pub(crate) fn parse_bench_market_args(args: &[String]) -> Result<ParsedBenchMarket, String> {
    let mut cli = BenchMarketCli::default();
    let mut it = CliArgs::new(args);
    while let Some(f) = it.next_flag() {
        match f.flag {
            "--db" => {
                cli.db = PathBuf::from(it.value(f.inline_value, "--db")?);
            }
            "--points" => {
                cli.points = parse_tier_csv(&it.value(f.inline_value, "--points")?, "--points")?;
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
            "-h" | "--help" => return Ok(ParsedBenchMarket::Help),
            other => return Err(format!("未知参数 {other:?}")),
        }
    }
    if cli.iterations == 0 {
        return Err("--iterations 至少为 1".to_string());
    }
    Ok(ParsedBenchMarket::Run(cli))
}

/// 标的池成员（前置探测产出，落库计划的输入）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PoolInstrument {
    pub instrument_id: String,
    pub currency_code: String,
    /// 场外基金（净值通道）：现价 priced_at = 净值日期且兼任 nav_date、来源
    /// 记新浪（生产基金通道形态）；场内为 false：priced_at = 写入时刻、无
    /// 净值日期、来源记腾讯。
    pub is_fund: bool,
    /// 价格来源标记（按实际取数源，ADR-0130 决策 7）：随通道映射，非自由文本。
    pub source: &'static str,
}

/// 单只标的的周采样点（落库计划内存形态）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedPoint {
    /// 采样交易日（当周周五，YYYY-MM-DD）。
    pub trade_date: String,
    /// 价格（万分之一元，ADR-0038 价格刻度）：BASE + 全局序号，批内全异。
    pub price_units: i64,
}

/// 单只标的的落库计划：周采样点序列（现价行由 [`Self::points`] 末点导出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedSeries {
    /// 标的池下标（回查币种/基金形态/来源标记用）。
    pub pool_index: usize,
    /// 周采样点（按采样周升序）。
    pub points: Vec<PlannedPoint>,
}

/// 落库点流生成（纯函数，确定性可测）：替代取数层产出的确定性形态——换源
/// 重刷 / 历史补全落库计划的「剥网络」前置（产端网络与解析不属被测量）。
///
/// 采样周自锚点周起逐周递进（锚点 = 价格历史最大采样日次周周一，调用方
/// [`probe_market_dataset`] 传入）：全部为既有库无有的新周，落库恰增行、无
/// 一条命中整周覆盖。采样日取当周周五（生成器「周一进位、取当周周五」同款），
/// 价格 = [`BASE_PRICE_UNITS`] + 全局序号（批内全异）。分布：单标的集中全部
/// 点落首标的（同一索引子树内连续插入）；多标的均匀按池依次分块，每只获得
/// 一段连续周序列（镜像生产「逐只整根回填 / 一轮排空」的落库形状，索引子树
/// 逐只摊薄的最好形态）。
///
/// 标的池必须非空（调用方探测已保证）；points 为该单元总点数，两种分布的
/// 总点数守恒（行行真写的计数前提）。
pub(crate) fn generate_price_plan(
    points: usize,
    pool: &[PoolInstrument],
    spread: Spread,
    anchor_monday: NaiveDate,
) -> Vec<PlannedSeries> {
    let point = |i: usize| -> PlannedPoint {
        // 采样周：锚点周 + i 周；采样日 = 当周周五（周一 + 4 日）。档位受解析
        // 层上界（[`super::bench_common::MAX_TIER`]）约束（issue #1650）：上界
        // × 7 天远离 chrono 极值，日期运算不越界。
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
            // 依次分块：每只标的得一段连续周（块宽 = ⌈points/len⌉，末块收短），
            // 全局序号连续跨越块边界（价格全异、周不重叠、总点数守恒）；
            // 点数不足整池时尾部标的零点、不落库（镜像生产：一轮排空只写
            // 队列内的标的）。
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

/// 锚点周周一：价格历史最大采样日次日所在周及之后的第一个周一
/// ——采样周全部晚于既有价格窗口，新周追加不与既有周冲突。
fn next_monday(on_or_after: NaiveDate) -> NaiveDate {
    let days_from_monday = i64::from(on_or_after.weekday().num_days_from_monday());
    on_or_after + chrono::Duration::days((7 - days_from_monday) % 7)
}

/// 价格历史行数（schema 级计数事实）：量测有效性的核验面——本基准无既有
/// pub 计数 API（价格历史无批量读出口），与生成器测试的 PRAGMA/原生 SQL
/// 豁免同类；探针/基线核对/落库核验共用，禁止扩散为第二业务读口径。
pub(crate) fn count_price_history_rows(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .map_err(|e| format!("价格历史行数读取失败：{e}"))
}

/// 数据集前置探测结果：标的池、锚点周与基线行数。
pub(crate) struct MarketProbe {
    /// 落库标的池：行情/净值通道成员（同步采集面），顺序随标的列表查询
    /// （确定性，同一库恒同序）。
    pub(crate) pool: Vec<PoolInstrument>,
    /// 锚点周周一（采样周起点：价格历史最大采样日次周，与既有价格周零交集）。
    pub(crate) anchor_monday: NaiveDate,
    /// pristine 快照的价格历史行数（落库核验与快照恢复闭环的基线）。
    pub(crate) pristine_history_rows: i64,
}

/// 前置探测（走现有 pub 查询函数，与读基准同款前置，不计入任何量测）：
/// 日期极值推锚点周、标的清单推落库池、行数取基线。
///
/// 标的池口径：价格写入通道成员中的同步采集面（行情 / 净值）——手动报价是
/// 用户单笔动作不属大批量形态，恒定价格不落历史行、无来源不参与价格写入
/// （词汇表 PriceChannel 闭集）；池空即拒绝（生成库恒有行情标的）。
pub(crate) fn probe_market_dataset(conn: &Connection) -> Result<MarketProbe, String> {
    let range = reports_domain::query_report_date_range(conn).map_err(|e| e.to_string())?;
    if range.max_date.is_none() {
        return Err("库为空（无未删除交易），请先运行 ledger-perf generate".to_string());
    }
    // 锚点依据 = 价格历史最大采样日（非交易日期极值）：交易散布不满窗口时
    // 最大交易日期早于价格窗口末端，按它推周会与既有价格周相交、触发整周
    // 覆盖（正确性底线断言误杀量测）。最大采样日是无既有读 API 的 schema 级
    // 事实（与行数计数同类豁免）。
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

    // 页上限 500：工具契约是消费 generate 产出库（20 标的），单页足够；
    // 标的清单非分页语义的消费方，静默截断只会让池变小、不会错写。
    let listing = ledger_investment::list_instruments(
        conn,
        &InstrumentListFilter {
            search: None,
            market: None,
            kind: None,
            only_invested: None,
            page: Some(1),
            page_size: Some(500),
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

/// 基准运行配置（测试可注入小参数；与 [`BenchMarketCli`] 的矩阵字段一一对应）。
#[derive(Debug, Clone)]
pub(crate) struct MarketBenchConfig {
    pub points: Vec<usize>,
    pub warmup: usize,
    pub iterations: usize,
}

/// 单元（点数档 × 分布）的量测结果（人读报告行 + 冒烟断言面）。
#[derive(Debug, Clone)]
pub(crate) struct MarketBenchMetrics {
    /// 指标名 = 「行情 {points} 点·{分布}」（动态生成，冒烟测试按默认矩阵钉住）。
    pub name: String,
    /// 点数档（报告列 + 断言面）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub points: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub spread: Spread,
    pub min_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
    /// 单点均摊 p95 = 单元 p95 ÷ 点数（逐点落库耗时不新增观测代码，见模块头）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub per_point_p95_ms: f64,
    /// 规模备注：分布、锚点周与单点均摊口径。
    pub context: String,
}

/// 入口：打开源库校验、建快照、跑量测矩阵、打印人读报告（无门禁，纯观测）。
pub(crate) fn run(cli: BenchMarketCli) -> Result<(), String> {
    super::bench::init_tracing();
    if !cli.db.exists() {
        return Err(format!(
            "库文件不存在：{}（先运行 ledger-perf generate 生成）",
            cli.db.display()
        ));
    }
    let cfg = MarketBenchConfig {
        points: cli.points.clone(),
        warmup: cli.warmup,
        iterations: cli.iterations,
    };
    let results = run_benchmark(&cli.db, &cfg)?;
    print_report(&cli.db, &cfg, &results);
    Ok(())
}

/// 量测核心（测试接缝）：建 pristine 快照 → 探测 → 矩阵逐单元
/// 「恢复快照 → 基线核对 → 批量落库 → 核验 → 计时」→ 指标清单。
///
/// 单元展开次序稳定：点数档外层、分布内层（[`Spread::ALL`]）。
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
    // 探测在工作库上做（快照的副本，探测的读路径与正式迭代完全一致）。
    let probe = {
        let conn = open_connection(&guard.work).map_err(|e| e.to_string())?;
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

/// 跑一个矩阵单元：`预热+迭代` 次恢复快照批量落库，计时窗口只包逐标的
/// 落库（事务壳 + 现价 upsert + 周采样逐点 upsert——生产同步/回填通道的
/// 完整本地落库路径）；点流生成与迭代后的核验在计时窗口外，不稀释量测。
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
        let conn = open_connection(&guard.work).map_err(|e| e.to_string())?;
        // 点流每次迭代重新生成（纯函数、确定性，量测窗口外）；计划按标的
        // 分组，组序即落库序（集中单组、均匀按池序）。
        let plan = generate_price_plan(points, &probe.pool, spread, probe.anchor_monday);
        // 快照恢复闭环（量测有效性）：迭代前基线行数必须与 pristine 快照一致
        // ——上一迭代的写副作用若有残留，本轮断言即红。
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

/// 单只标的的落库（生产通道形态：一只标的一个事务——`write_weekly_price_history`
/// 接缝契约「周采样落库单点不开事务，调用方包事务」）：现价行 upsert +
/// 周采样逐点 upsert，全部经价格写入单点。现价形态照通道：基金 priced_at =
/// 净值日期（兼任 nav_date）、来源新浪；场内 priced_at = 写入时刻、无净值
/// 日期、来源腾讯（ADR-0130 决策 7）。
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
        // 场内现价时点 = 写入时刻（生产通道同款，ADR-0103 决策 4）。
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

/// 量测有效性前置核验（照 bench-import 既有断言模式）：价格历史行数恰增
/// 点数（行行真写，无一条命中整周覆盖——点流全为新周），且每只触达标的的
/// 现价行与点流末点一致（现价 = 最新历史点映像；基金 nav_date 随末点）。
/// 任一违反即基准量到的不是「N 点落库」，结果无效。
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

/// 人读报告行接口实现（issue #1650 收口）：交共享表列打印，不再自持排版。
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

/// 人读表格输出：与 bench-import / bench-sync 同款列形（min / avg / p95 +
/// 规模备注），无门禁行；表列排版收口在 [`print_metric_table`]（issue #1650）。
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
