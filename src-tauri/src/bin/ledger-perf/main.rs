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

pub(crate) const DEFAULT_SEED: u64 = 42;
pub(crate) const DEFAULT_TRANSACTIONS: u64 = 500_000;
pub(crate) const DEFAULT_END_DATE: &str = "2025-12-31";

pub(crate) fn default_out() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/ledger-perf/ledger-perf.db")
}

struct Subcommand {
    name: &'static str,
    summary: &'static str,
    flags: fn() -> Vec<FlagHelp>,
    options_order: u8,
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
