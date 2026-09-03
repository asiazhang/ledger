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
//! ## bench：查询基准（issue #461）
//!
//! 对 generate 产出的库跑 10 项查询基准并输出 min/avg/p95 报告：
//!
//! ```text
//! cargo run --bin ledger-perf -- bench [--db PATH] [--warmup N]
//!                                     [--iterations N] [--search TERM]
//! ```
//!
//! - 基准集：列表首页分页；深分页（OFFSET 逼近全量）；账户+日期范围筛选
//!   列表；全账户实时余额；月度汇总（5 年 60 个月）；分类占比；TransactionSearch
//!   备注搜索（含拼音过滤，CPU 密集）；净资产总览聚合；持仓列表；时点持仓
//!   （直接聚合标的交易）。
//! - 唯一接缝：全部经「现有 pub 查询函数 + 标准连接工厂打开文件库」调用，
//!   与 IPC 命令同一 SQL 路径；慢查询日志（perf_trace，≥100ms warn）经连接
//!   工厂自动挂载，基准内不重写 SQL。
//! - 每项预热 + 多次迭代，报 min/avg/p95，人读表格输出；p95 超阈值的基准
//!   在报告中以 ▲ 标记，SQL 级线索在日志「慢查询」条目中检索。
//!
//! # 实现边界
//!
//! 全部生成/基准/摘要逻辑封在本 bin 模块内部，产品 lib 不新增模块、
//! 零新增依赖（ADR-0062：确定性生成而非入库；被否决备选见该 ADR）。

mod bench;
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
    generate    生成性能基准数据集（默认 50 万笔 Transaction 的多域画像 SQLite 库）
    bench       查询基准——10 项查询 × min/avg/p95 报告（issue #461）

bench OPTIONS:
    --db <PATH>            目标库文件（默认同 generate 输出路径，须已生成）
    --warmup <N>           每项基准预热次数（默认 3，不计入统计）
    --iterations <N>       每项基准计时迭代次数（默认 10）
    --search <TERM>        备注搜索基准的关键字（默认 咖啡）
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
