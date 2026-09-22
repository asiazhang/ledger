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

//! # 工具用法
//!
//! 命令行用法（子命令与 flag 全集）以 `--help` 的渲染输出为唯一真源——帮助
//! 从各子命令 flag 表渲染（issue #1696），本注释不复述用法，只叙述语义。
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
//! - 附属账本（issue #1630）：除主库外同时生成 2 本附属账本——固定 2,000 笔/
//!   本的小规模独立库（不随 `--transactions` 缩放），经同一生成路径产出全套
//!   画像（含标的交易与批次持仓）；每本一个子目录（`ledger-perf-books/
//!   book-NN/ledger.db`），与产品「一库 = 一本账」的目录形态一致（ADR-0089），
//!   根目录在主库文件同级、每次生成先清后建；每本由基础种子派生自己的种子
//!   （seed+1+序号），主库确定性不受影响。这是跨账本投资汇总基准（bench，
//!   ADR-0114）的消费夹具：删除本目录 → 该基准项前置探测失败、bench 运行红。
//! - 默认输出 `<src-tauri>/target/ledger-perf/ledger-perf.db`（构建目标目录下，
//!   天然被版本控制忽略、与真实用户库物理隔离，`--out` 可改为任意文件路径）。
//!   目标文件已存在时先删除再重建（保证从空库迁移 + 幂等重建）。
//! - 零新增依赖：随机数用手写可种子化 PRNG（本 bin `rng` 模块），参数用手写解析。
//! - 生成耗时采用一次性事务 + 关闭 fsync 的连接级 PRAGMA（`synchronous=OFF`、
//!   `journal_mode=MEMORY`，均不持久化进库文件），50 万笔在几十秒内完成；
//!   生成的库仅供性能基准消费，不是用户数据，崩溃重跑即可。
//!
//! ## bench：查询基准（issue #461；门禁 issue #493 / ADR-0068；拼音子序列基准 issue #514；
//! ## 商户占比与投资三项读基准 issue #1627；跨账本投资汇总基准 issue #1630）
//!
//! 对 generate 产出的库跑 16 项查询基准并输出 min/avg/p95 报告：
//!
//! 用法以 `cargo run --bin ledger-perf -- bench --help` 的渲染输出为准
//! （flag 表单一真源，issue #1696）。
//!
//! - 基准集：列表首页分页；深分页（OFFSET 逼近全量）；账户+日期范围筛选
//!   列表；全账户实时余额；月度汇总（5 年 60 个月）；分类占比；
//!   TransactionSearch 备注搜索两条基准并列（issue #514）：中文子串
//!   （默认「咖啡」，原文连续子串路径）与拼音子序列（默认「kf」，命中
//!   「买咖啡」→ mkf 等、不构成原文子串，拼音首字母子序列路径），均为
//!   CPU 密集全量扫描；净资产总览聚合；持仓列表；时点持仓（直接聚合标的交易）；
//!   商户占比（报表三件套第三件）；投资组合趋势（周采样 × 时点持仓嵌套循环）；
//!   资金加权收益率（XIRR 数值解）；财务自由度（issue #1627）；跨账本投资
//!   汇总（issue #1630：主库＝活动本连接 + 附属账本逐本只读建连，经壳层
//!   编排 pub 函数取数折算合并，与 IPC 命令同一编排路径；前置探测附属账本
//!   夹具，缺失即整体失败）。
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
//! 用法以 `cargo run --bin ledger-perf -- bench-import --help` 的渲染输出为
//! 准（flag 表单一真源，issue #1696）。
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
//! - 无门禁判定：写基准阈值未立（本地/CI 磁盘口径差异未测，阈值待
//!   CI 基线分布稳定后修订 ADR-0068 另立判据）；perf-bench CI 每日以
//!   最大档 200 行 × 两分布观测、只记录进 Summary；check.sh 不跑本基准。
//!
//! ## bench-sync：同步重放写基准（issue #1628）
//!
//! 对 generate 产出的库量测「apply_ops 循环逐条合并外来 op 流」的本地写耗时
//! （剥网络：S3 通道传输不属回归面，ADR-0068 网络边界豁免）：
//!
//! 用法以 `cargo run --bin ledger-perf -- bench-sync --help` 的渲染输出为准
//! （flag 表单一真源，issue #1696）。
//!
//! - 量测矩阵：op 数量档（默认 100,500,2000，按同步轮真实量级：日常轮/
//!   离线一周积压/离线一月上限形态）× 两种分布（同账户集中/多账户均匀）
//!   × 两个权威入口——wire 接入（`ingest_ops`，Transport 通道上的生产接缝）
//!   与进程内重放（`apply_ops`，已解析形态）；报告每单元总耗时 min/avg/p95
//!   与单 op 均摊 p95（n<20 时 p95=max，同 ADR-0068 统计口径）。
//! - op 流确定性生成（双端消费形态：A 端 `read_ops` → B 端重放），迭代前
//!   从 pristine 快照恢复隔离写副作用（快照机制与 bench-import 共用）；
//!   重放后的结果核验与余额缓存一致性断言失败即量测作废（照 bench-import
//!   既有断言模式）。
//! - 无门禁判定：写基准阈值未立（同 bench-import）；perf-bench CI 每日以
//!   最大档 2000 op × 两分布 × 两入口观测、只记录进 Summary；check.sh
//!   不跑本基准。
//!
//! ## bench-market：行情/价格历史批量 upsert 写基准（issue #1629）
//!
//! 对 generate 产出的库量测行情同步与价格历史补全的本地批量 upsert 耗时
//! （剥网络：行情源抓取不属回归面，ADR-0068 网络边界豁免）：
//!
//! 用法以 `cargo run --bin ledger-perf -- bench-market --help` 的渲染输出为
//! 准（flag 表单一真源，issue #1696）。
//!
//! - 量测矩阵：周采样点数档（默认 104,1040,5200，按同步落库真实量级：单只
//!   近两年整根首刷 / 部分批量重刷 / 全库价格线整根重刷）× 两种落位分布
//!   （单标的集中 / 多标的均匀）；报告每单元总耗时 min/avg/p95 与单点均摊
//!   p95（n<20 时 p95=max，同 ADR-0068 统计口径）。
//! - 落库走投资域价格写入单点（upsert_market_price / upsert_price_history，
//!   与生产同步/回填通道同一 SQL 路径），事务边界照生产形态「一只标的一个
//!   事务」；点流确定性生成，迭代前从 pristine 快照恢复隔离写副作用（快照
//!   机制与 bench-import / bench-sync 共用）；落库后行数恰增与现价一致性
//!   断言失败即量测作废（照 bench-import 既有断言模式）。
//! - 无门禁判定：写基准阈值未立（同 bench-import）；perf-bench CI 每日以
//!   最大档 5200 点 × 两分布观测、只记录进 Summary；check.sh 不跑本基准。
//!
//! # 实现边界
//!
//! 全部生成/基准/摘要逻辑封在本 bin 模块内部，产品 lib 不新增模块、
//! 零新增依赖（ADR-0062：确定性生成而非入库；被否决备选见该 ADR）。

mod bench;
mod bench_common;
mod bench_import;
mod bench_market;
mod bench_sync;
mod books;
mod generate;
mod investments;
mod plans;
mod rng;
mod snapshot;

#[cfg(test)]
mod tests;

use bench_common::{FlagHelp, Outcome, flag_helps};
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

/// 子命令登记（dispatch 表驱动，issue #1696）：子命令名、简介、flag 表投影
/// 与 run 入口，新增子命令只登记本表一处。SUBCOMMANDS 列举沿表序；OPTIONS
/// 节次序与之现状不同（generate 垫底），以 [`Self::options_order`] 显式登记
/// 在同一条目内——两序都随表维护，不另立次序真源。
struct Subcommand {
    name: &'static str,
    summary: &'static str,
    /// flag 表的帮助投影（渲染消费；解析走各子命令自己的表）。
    flags: fn() -> Vec<FlagHelp>,
    /// 本子命令 OPTIONS 节在 `--help` 全文中的次序。
    options_order: u8,
    /// run 入口（解析 + 运行 → 共享结局形态）。
    execute: fn(&[String]) -> Outcome,
}

const SUBCOMMANDS: &[Subcommand] = &[
    Subcommand {
        name: "generate",
        summary: "生成性能基准数据集（默认 50 万笔 Transaction 的多域画像 SQLite 库 + 2 本附属账本小库，issue #1630）",
        flags: || flag_helps(generate::FLAGS),
        options_order: 4,
        execute: generate::execute,
    },
    Subcommand {
        name: "bench",
        summary: "查询基准——16 项查询 × min/avg/p95 报告（issue #461）",
        flags: || flag_helps(bench::FLAGS),
        options_order: 0,
        execute: bench::execute,
    },
    Subcommand {
        name: "bench-import",
        summary: "批量导入写基准——固定行数 × 两种分布 × 总耗时/单行均摊 p95（issue #532，纯观测无门禁）",
        flags: || flag_helps(bench_import::FLAGS),
        options_order: 1,
        execute: bench_import::execute,
    },
    Subcommand {
        name: "bench-sync",
        summary: "同步重放写基准——op 流重放（ingest_ops/apply_ops 权威入口）× 两种分布 × 总耗时/单 op 均摊 p95（issue #1628，剥网络，纯观测无门禁）",
        flags: || flag_helps(bench_sync::FLAGS),
        options_order: 2,
        execute: bench_sync::execute,
    },
    Subcommand {
        name: "bench-market",
        summary: "行情/价格历史批量 upsert 写基准——点数档 × 两种分布 × 总耗时/单点均摊 p95（issue #1629，剥网络，纯观测无门禁）",
        flags: || flag_helps(bench_market::FLAGS),
        options_order: 3,
        execute: bench_market::execute,
    },
];

/// 全文帮助（OPTIONS / SUBCOMMANDS 块从 dispatch 数据渲染，issue #1696）：
/// 渲染规则与折行收口在 [`bench_common::render_help`]，全文由 tests 的 inline
/// golden 钉住。
fn render_usage() -> String {
    let subcommands: Vec<(&str, &str)> = SUBCOMMANDS.iter().map(|s| (s.name, s.summary)).collect();
    let mut sections: Vec<(u8, &str, Vec<FlagHelp>)> = SUBCOMMANDS
        .iter()
        .map(|s| (s.options_order, s.name, (s.flags)()))
        .collect();
    sections.sort_by_key(|(order, _, _)| *order);
    let sections: Vec<(&str, Vec<FlagHelp>)> = sections
        .into_iter()
        .map(|(_, name, flags)| (name, flags))
        .collect();
    bench_common::render_help(&subcommands, &sections)
}

fn print_usage() {
    println!("{}", render_usage());
}

/// bin 进程级接缝接线（生产 main 与测试建库单点共用；幂等，先装者优先）：
/// 写路径副作用接缝（issue #1090 / #1091）——bench-import 的批量导入走核心
/// 交易域写入协议，余额刷新实现须先注册；注册点在下层（核心交易域/定时计划域/
/// 备份域），实现由账户域/定时计划域/备份域提供；期次落账置脏与追补触发两条
/// 接缝同形对装（#1091）。交易域接缝接线（issue #1092 / #1180）：六向实现经
/// 组合入口一次装入。测试侧（tests::build 建库单点）必须经本单点接线——
/// 同形复制多处会让「新增钩子只装一处」的漂移重新引入顺序依赖 flake
///（与壳层启动、test_support::open、BDD world 各自的单点同款纪律）。
pub(crate) fn install_process_wiring() {
    ledger_accounts::balance::install_balance_refresh_hook();
    ledger_scheduled::install_plan_source_hook();
    ledger_scheduled::auto_run::register_after_occurrence_hook(
        ledger_backup::occurrence_dirty_hook,
    );
    ledger_backup::register_catch_up_hook(ledger_scheduled::auto_run::catch_up_hook);
    tauri_app_lib::transaction_wiring::install_all();
}

fn main() -> ExitCode {
    install_process_wiring();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(sub) = args.first() else {
        print_usage();
        return ExitCode::from(2);
    };
    if let Some(entry) = SUBCOMMANDS.iter().find(|s| s.name == sub.as_str()) {
        return match (entry.execute)(&args[1..]) {
            Outcome::Ok => ExitCode::SUCCESS,
            Outcome::Help => {
                print_usage();
                ExitCode::SUCCESS
            }
            Outcome::ParamError(msg) => {
                eprintln!("参数错误：{msg}\n");
                print_usage();
                ExitCode::from(2)
            }
            Outcome::Failed(msg) => {
                eprintln!("{} 失败：{msg}", entry.name);
                ExitCode::FAILURE
            }
        };
    }
    if sub == "-h" || sub == "--help" {
        print_usage();
        return ExitCode::SUCCESS;
    }
    eprintln!("未知子命令 {sub:?}\n");
    print_usage();
    ExitCode::from(2)
}
