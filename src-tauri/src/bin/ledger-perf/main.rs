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
//! ## generate：生成性能形似真实的核心交易域大库（issue #459）
//!
//! 一条命令在本地生成默认 50 万笔 Transaction 的 SQLite 库：
//!
//! - 建库经应用自身的迁移应用路径（`db::open_connection` → `db::init_db`），
//!   不复制任何 DDL——schema 与 user_version 永远和真实产品一致（迁移是唯一事实来源）。
//! - 数据画像（核心交易域，ADR-0062）：50 个账户（现金/储蓄/信用卡/钱包混合，
//!   含少量 USD/EUR 账户）；40 个分类（迁移自带默认种子分类之外另生成）；
//!   800 个商户呈长尾（top 20 占挂商户流水的约 60%）；DefaultCurrency 为 CNY，
//!   USD/EUR 少量且 `fx_rate_history` 全历史（窗口内每周一采样）填充；
//!   转账约 8%、退款链约 2%（`refund_of_transaction_id` 指向更早的支出）、
//!   交易软删除约 1%。投资域 / Budget / ScheduledTransaction 填充见 issue #460。
//! - 确定性：固定默认种子 42（`--seed` 覆盖）；默认 500,000 笔（`--transactions`）；
//!   锚定结束日期 2025-12-31（`--end-date`），数据落在其前约 5 年窗口内，
//!   不锚定「今天」；全部时间字段（id 时间戳位 / created_at / updated_at）由
//!   种子与日期推导，无墙钟参与——同参数两次生成产出逐字节同构的库。
//! - 默认输出 `<src-tauri>/target/ledger-perf/ledger-perf.db`（构建目标目录下，
//!   天然被版本控制忽略、与真实用户库物理隔离，`--out` 可改为任意文件路径）。
//!   目标文件已存在时先删除再重建（保证从空库迁移 + 幂等重建）。
//! - 零新增依赖：随机数用手写可种子化 PRNG（本 bin `rng` 模块），参数用手写解析。
//! - 生成耗时采用一次性事务 + 关闭 fsync 的连接级 PRAGMA（`synchronous=OFF`、
//!   `journal_mode=MEMORY`，均不持久化进库文件），50 万笔在几十秒内完成；
//!   生成的库仅供性能基准消费，不是用户数据，崩溃重跑即可。
//!
//! ## bench：查询基准（issue #461，尚未实现）
//!
//! 在生成的库上测量与 IPC 命令相同的查询函数，报 min/avg/p95。
//!
//! # 实现边界
//!
//! 全部生成/基准/摘要逻辑封在本 bin 模块内部，产品 lib 不新增模块、
//! 零新增依赖（ADR-0062：确定性生成而非入库；被否决备选见该 ADR）。

mod generate;
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
    generate    生成性能基准数据集（默认 50 万笔 Transaction 的 SQLite 库）
    bench       查询基准（尚未实现，见 issue #461）

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

/// 手写参数解析（零新增依赖）。返回 Err(消息) 表示用法错误。
pub(crate) fn parse_args(args: &[String]) -> Result<GenerateCli, String> {
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
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("未知参数 {other:?}")),
        }
        i += 1;
    }
    Ok(cli)
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
            Ok(cli) => match generate::run(cli) {
                Ok(()) => ExitCode::SUCCESS,
                Err(msg) => {
                    eprintln!("generate 失败：{msg}");
                    ExitCode::FAILURE
                }
            },
            Err(msg) if msg.is_empty() => {
                print_usage();
                ExitCode::SUCCESS
            }
            Err(msg) => {
                eprintln!("参数错误：{msg}\n");
                print_usage();
                ExitCode::from(2)
            }
        },
        "bench" => {
            eprintln!("bench 子命令尚未实现（issue #461）");
            ExitCode::from(2)
        }
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
