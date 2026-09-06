//! 性能基准工具（ledger-perf）——后端包内 CLI，spec #458 / issue #459。

// 测试整体豁免（ADR-0060，与 lib.rs 同款）：clippy 六件套 deny 仅约束生产路径；
// 本 bin 的 #[cfg(test)] 模块（含 tests.rs）经 crate 根 cfg(test) 整体放行，
// 生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

//! # 工具用法（本注释即用法真源）
//!
//! ```text
//! cargo run --bin ledger-perf -- generate [--seed N] [--transactions N]
//!                                        [--end-date YYYY-MM-DD] [--out PATH]
//! ```
//!
//! ## generate：生成性能形似真实的多域画像大库（issue #459/#460）
//!
//! 一条命令在本地生成默认 50 万笔 Transaction 的 SQLite 库：
//!
//! - 建库经应用自身的迁移应用路径（`db::open_connection` → `db::init_db`），
//!   不复制任何 DDL——schema 与 user_version 永远和真实产品一致（迁移是唯一事实来源）。
//! - 核心交易域画像（ADR-0062）：50 个账户（现金/储蓄/信用卡/钱包/投资混合，
//!   含少量 USD/EUR/HKD 账户）；40 个分类（迁移自带默认种子分类之外另生成）；
//!   800 个商户呈长尾（top 20 占挂商户流水的约 60%）；DefaultCurrency 为 CNY，
//!   USD/EUR/HKD 少量且 `fx_rate_history` 全历史（窗口内每周一采样）填充；
//!   转账约 8%、退款链约 2%（`refund_of_transaction_id` 指向该账户更早生成的
//!   支出，退款日期不早于原支出日）、交易软删除约 1%。
//! - 投资域画像（issue #460）：20 个标的（A 股/ETF/港股/场外基金，同步来源随
//!   通道标记）；周采样 `price_history` 全窗口 + `market_prices` 现价缓存
//!   （现价 = 最新历史点映像，基金带净值日期）；约 0.6% 交易为 buy/sell 标的
//!   交易（默认规模下约 3000 笔：买入 0.4% + 卖出 0.2%，含部分卖出与清仓）——
//!   交易行 + `security_transactions` 扩展 + buy 建仓批次 + sell FIFO 匹配，
//!   分摊/闭合公式镜像产品 Writer；标的只与同币种投资账户交易（产品金额
//!   公式不做标的价币→账户币折算），港股市值的币种折算发生在净资产聚合层。
//! - 计划域画像（issue #460）：6 条 Budget（月度 4 + 年度 2，支出分类不重复）；
//!   8 个 ScheduledTransaction（分期 3/订阅 3/定时转账 2，含 paused/failed/
//!   cancelled 形态）+ 期次展开——结束日前往期 completed 并各生成一条真实
//!   交易（从 `--transactions` 预算预留，交易总数 = max(--transactions, 期次
//!   交易数)），未来期次 pending。
//! - 确定性：固定默认种子 42（`--seed` 覆盖）；默认 500,000 笔（`--transactions`）；
//!   锚定结束日期 2025-12-31（`--end-date`），数据落在其前约 5 年窗口内，
//!   不锚定「今天」；生成数据的全部时间字段（id 时间戳位 / created_at /
//!   updated_at）由种子与日期推导，无墙钟参与——同参数两次生成，生成内容的
//!   全表有序摘要一致（迁移种子行的审计时间列除外，见 tests 摘要断言）。
//! - 默认输出 `<src-tauri>/target/ledger-perf/ledger-perf.db`（构建目标目录下，
//!   天然被版本控制忽略、与真实用户库物理隔离，`--out` 可改为任意文件路径）。
//!   目标文件已存在时先删除再重建（保证从空库迁移 + 幂等重建）。
//! - 零新增依赖：随机数用手写可种子化 PRNG（本 bin `rng` 模块），参数用手写解析。
//! - 生成耗时采用一次性事务 + 关闭 fsync 的连接级 PRAGMA（`synchronous=OFF`、
//!   `journal_mode=MEMORY`，均不持久化进库文件），50 万笔在几十秒内完成；
//!   生成的库仅供性能基准消费，不是用户数据，崩溃重跑即可。
//!
//! ## bench：查询基准（issue #461；门禁 issue #493 / ADR-0068；拼音子序列基准 issue #514）
//!
//! 对 generate 产出的库跑 11 项查询基准并输出 min/avg/p95 报告：
//!
//! ```text
//! cargo run --bin ledger-perf -- bench [--db PATH] [--warmup N]
//!                                     [--iterations N] [--search TERM]
//!                                     [--search-pinyin TERM] [--max-p95-ms MS]
//! ```
//!
//! - 基准集：列表首页分页；深分页（OFFSET 逼近全量）；账户+日期范围筛选
//!   列表；全账户实时余额；月度汇总（5 年 60 个月）；分类占比；
//!   TransactionSearch 备注搜索两条基准并列（issue #514）：中文子串
//!   （默认「咖啡」，原文连续子串路径）与拼音子序列（默认「kf」，命中
//!   「买咖啡」→ mkf 等、不构成原文子串，拼音首字母子序列路径），均为
//!   CPU 密集全量扫描；净资产总览聚合；持仓列表；时点持仓（直接聚合标的交易）。
//! - 唯一接缝：全部经「现有 pub 查询函数 + 标准连接工厂打开文件库」调用，
//!   与 IPC 命令同一 SQL 路径；慢查询日志（perf_trace，≥100ms warn）经连接
//!   工厂自动挂载，基准内不重写 SQL。
//! - 每项预热 + 多次迭代，报 min/avg/p95，人读表格输出；p95 超阈值的基准
//!   在报告中以 ▲ 标记，SQL 级线索在日志「慢查询」条目中检索。
//! - 门禁（CI）：`--max-p95-ms 200` 时全部基准逐项判定 p95 ≤ 阈值，超标即
//!   非零退出（统计口径与豁免原则见 ADR-0068）；缺省只报告不判定。
//!
//! ## bench-import：批量导入写基准（issue #532）
//!
//! 对 generate 产出的库量测「批量导入固定行数」的写耗时，检验 ADR-0067
//! 写路径每行整体重算的 O(N²) 假设（#519 grilling 裁决：粒度合并是否立项
//! 由数裁决）：
//!
//! ```text
//! cargo run --bin ledger-perf -- bench-import [--db PATH] [--rows <CSV>]
//!                                             [--dedup <BOOL>] [--warmup N]
//!                                             [--iterations N]
//! ```
//!
//! - 量测矩阵：行数档（默认 50,100,200，按月导入真实量级：轻量/典型/上限月）
//!   × 两种分布——「同账户集中」全部落
//!   首账户（最坏形态）、「多账户均匀」按账户池轮转（最好形态）；报告每
//!   单元总耗时 min/avg/p95 与单行均摊 p95（n<20 时 p95=max，同 ADR-0068
//!   统计口径）。
//! - 每次迭代从 pristine 快照恢复（源库零改动），迭代间数据集规模固定，
//!   p95 是同一状态的真分位数；导入走批量编排权威 `TransactionBatch::run`
//!   （与 HTTP 批量导入/IPC 批量创建同一 SQL 路径），基准行为 expense，
//!   基准日期 = 数据集最大交易日期次日（去重身份不命中既有数据）。
//! - 正确性底线：每次导入完成后断言余额缓存与实时计算逐账户一致，失败即
//!   量测作废——基准必须跑在正确路径而非坏缓存路径上。
//! - 刷新段开销不新增观测代码：余额缓存重算的单条 SQL 耗时经既有耗时日志
//!   （perf_trace）归因——慢查询（≥100ms）warn 直接可见，全量明细需
//!   DEBUG 级日志。
//! - 纯观测：无门禁、不进 CI/check.sh，输出人工判读。
//!
//! # 实现边界
//!
//! 全部生成/基准/摘要逻辑封在本 bin 模块内部，产品 lib 不新增模块、
//! 零新增依赖（ADR-0062：确定性生成而非入库；被否决备选见该 ADR）。

mod bench;
mod bench_import;
mod generate;
mod investments;
mod plans;
mod rng;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::process::ExitCode;

/// 默认随机种子（固定值，保证可复现）。
pub(crate) const DEFAULT_SEED: u64 = 42;
/// 默认生成笔数（50 万）。
pub(crate) const DEFAULT_TRANSACTIONS: u64 = 500_000;
/// 默认锚定结束日期（刻意不锚定「今天」，避免数据随墙钟漂移）。
pub(crate) const DEFAULT_END_DATE: &str = "2025-12-31";

/// 默认输出路径：构建目标目录下（`src-tauri/target/ledger-perf/`），
/// 编译期常量拼接，天然被版本控制忽略、与真实用户库物理隔离。
pub(crate) fn default_out() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/ledger-perf/ledger-perf.db")
}

/// 顶部用法说明（与模块头注释同源，`--help` 输出）。
const USAGE: &str = "\
ledger-perf —— Ledger 性能基准工具

USAGE:
    ledger-perf <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
    generate       生成性能基准数据集（默认 50 万笔 Transaction 的多域画像 SQLite 库）
    bench          查询基准——11 项查询 × min/avg/p95 报告（issue #461）
    bench-import   批量导入写基准——固定行数 × 两种分布 × 总耗时/单行均摊 p95
                   （issue #532，纯观测无门禁）

bench OPTIONS:
    --db <PATH>            目标库文件（默认同 generate 输出路径，须已生成）
    --warmup <N>           每项基准预热次数（默认 3，不计入统计）
    --iterations <N>       每项基准计时迭代次数（默认 20，n=20 才成真 p95 分位数）
    --search <TERM>        中文子串搜索基准的关键字（默认 咖啡）
    --search-pinyin <TERM> 拼音子序列搜索基准的关键字（默认 kf）
    --max-p95-ms <MS>      默认门禁阈值（毫秒）：全部基准 p95 ≤ 各自阈值才退出
                           0，任何一项超标即失败（CI 用；缺省不判定；分项例外
                           机制与现行清单见 ADR-0068）
    -h, --help             打印本说明

bench-import OPTIONS:
    --db <PATH>            源库文件（默认同 generate 输出路径，须已生成；本命令不
                           修改源库——内部建 pristine 快照，每次迭代从快照恢复）
    --rows <CSV>           每档导入行数（默认 50,100,200；逗号分隔、保持次序）
    --dedup <BOOL>         批量导入去重开关（默认 true，HTTP 批量导入生产默认）
    --warmup <N>           每档预热次数（默认 1，不计入统计）
    --iterations <N>       每档计时迭代次数（默认 5；每次迭代从快照恢复，数据集
                           规模固定）
    -h, --help             打印本说明

generate OPTIONS:
    --seed <N>             随机种子（默认 42，同种子必出同库）
    --transactions <N>     生成笔数（默认 500000）
    --end-date <YYYY-MM-DD> 数据窗口锚定结束日期（默认 2025-12-31，不锚定「今天」）
    --out <PATH>           输出库文件路径（默认 src-tauri/target/ledger-perf/ledger-perf.db；
                           已存在会先删除再重建）
    -h, --help             打印本说明";

/// 解析后的 generate 子命令参数（默认值见各 const）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GenerateCli {
    pub seed: u64,
    pub transactions: u64,
    pub end_date: String,
    pub out: PathBuf,
}

impl Default for GenerateCli {
    fn default() -> Self {
        GenerateCli {
            seed: DEFAULT_SEED,
            transactions: DEFAULT_TRANSACTIONS,
            end_date: DEFAULT_END_DATE.to_string(),
            out: default_out(),
        }
    }
}

/// 参数解析结果：运行参数或帮助请求。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParsedArgs {
    Run(GenerateCli),
    Help,
}

/// 手写参数解析（零新增依赖）。返回 Err(消息) 表示用法错误。
pub(crate) fn parse_args(args: &[String]) -> Result<ParsedArgs, String> {
    let mut cli = GenerateCli::default();
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
            "--seed" => {
                let v = take_value(&mut i, inline_value)?;
                cli.seed = v
                    .parse::<u64>()
                    .map_err(|_| format!("--seed 需要非负整数，得到 {v:?}"))?;
            }
            "--transactions" => {
                let v = take_value(&mut i, inline_value)?;
                cli.transactions = v
                    .parse::<u64>()
                    .map_err(|_| format!("--transactions 需要非负整数，得到 {v:?}"))?;
            }
            "--end-date" => {
                cli.end_date = take_value(&mut i, inline_value)?;
            }
            "--out" => {
                cli.out = PathBuf::from(take_value(&mut i, inline_value)?);
            }
            "-h" | "--help" => return Ok(ParsedArgs::Help),
            other => return Err(format!("未知参数 {other:?}")),
        }
        i += 1;
    }
    Ok(ParsedArgs::Run(cli))
}

fn print_usage() {
    println!("{USAGE}");
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(sub) = args.first() else {
        print_usage();
        return ExitCode::from(2);
    };
    match sub.as_str() {
        "generate" => match parse_args(&args[1..]) {
            Ok(ParsedArgs::Help) => {
                print_usage();
                ExitCode::SUCCESS
            }
            Ok(ParsedArgs::Run(cli)) => match generate::run(cli) {
                Ok(()) => ExitCode::SUCCESS,
                Err(msg) => {
                    eprintln!("generate 失败：{msg}");
                    ExitCode::FAILURE
                }
            },
            Err(msg) => {
                eprintln!("参数错误：{msg}\n");
                print_usage();
                ExitCode::from(2)
            }
        },
        "bench" => match bench::parse_bench_args(&args[1..]) {
            Ok(bench::ParsedBench::Help) => {
                print_usage();
                ExitCode::SUCCESS
            }
            Ok(bench::ParsedBench::Run(cli)) => match bench::run(cli) {
                Ok(()) => ExitCode::SUCCESS,
                Err(msg) => {
                    eprintln!("bench 失败：{msg}");
                    ExitCode::FAILURE
                }
            },
            Err(msg) => {
                eprintln!("参数错误：{msg}\n");
                print_usage();
                ExitCode::from(2)
            }
        },
        "bench-import" => match bench_import::parse_bench_import_args(&args[1..]) {
            Ok(bench_import::ParsedBenchImport::Help) => {
                print_usage();
                ExitCode::SUCCESS
            }
            Ok(bench_import::ParsedBenchImport::Run(cli)) => match bench_import::run(cli) {
                Ok(()) => ExitCode::SUCCESS,
                Err(msg) => {
                    eprintln!("bench-import 失败：{msg}");
                    ExitCode::FAILURE
                }
            },
            Err(msg) => {
                eprintln!("参数错误：{msg}\n");
                print_usage();
                ExitCode::from(2)
            }
        },
        "-h" | "--help" => {
            print_usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("未知子命令 {other:?}\n");
            print_usage();
            ExitCode::from(2)
        }
    }
}
