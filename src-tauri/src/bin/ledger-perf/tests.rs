use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::NaiveDate;
use rusqlite::Connection;

use ledger_accounts as accounts;
use ledger_budget as budget;
use ledger_categories as categories;
use ledger_currencies as currencies;
use ledger_infra::db::{open_connection, open_connection_in};
use ledger_investment as investment;
use ledger_investment::InstrumentListFilter;
use ledger_merchants as merchants;
use ledger_scheduled as scheduled_transactions;
use ledger_transaction::TransactionListFilter;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::read::{get_transaction, list_transactions};
use ledger_transaction::search_transactions_internal;
use tauri_app_lib::test_support::{self, FIXED_NOW};

use super::bench::{self, BenchCli, BenchConfig, BenchMetrics};
use super::bench_common::{self, Outcome, Parsed};
use super::bench_import::{self, BenchImportCli, Distribution, ImportBenchConfig};
use super::bench_market::{self, BenchMarketCli, Spread};
use super::bench_sync::{
    self, BenchSyncCli, EntryForm, SOURCE_DEVICE_ID, SyncBenchConfig, generate_ops, generate_wire,
};
use super::books;
use super::generate::{
    GenCounts, GenerateCli, GenerateParams, NOTE_SUBJECTS, NOTE_SUFFIXES, PURCHASE_BRANDS,
    PURCHASE_CATEGORIES, PURCHASE_ITEM_MAX, PURCHASE_REPEAT_SKUS, generate_into,
    parse_generate_args, purchase_name_pool,
};
use ledger_accounts::{Account, AccountType};
use ledger_backup as backup_domain;
use ledger_transaction::compute_dedup_hash;

fn parse_bench_cli(args: &[&str]) -> Result<BenchCli, String> {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    match bench::parse_bench_args(&owned)? {
        Parsed::Run(cli) => Ok(cli),
        Parsed::Help => panic!("该输入应解析为运行参数"),
    }
}

fn run_cli(args: &[&str]) -> GenerateCli {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    match parse_generate_args(&owned).unwrap() {
        Parsed::Run(cli) => cli,
        Parsed::Help => panic!("该输入应解析为运行参数"),
    }
}

#[test]
fn bench_smoke_runs_all_benchmarks() {
    let (_dir, path) = temp_db("bench-smoke");
    build_with_books(&path, 1_000, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    let conn = open_connection(&path).unwrap();

    let results = bench::run_benchmarks(
        &conn,
        &BenchConfig {
            warmup: 0,
            iterations: 2,
            search_term: "咖啡".to_string(),
            purchase_search_term: "猫粮".to_string(),
            books_dir: books::attached_books_root(&path),
        },
    )
    .unwrap();

    let names: Vec<&str> = results.iter().map(|r| r.name).collect();
    assert_eq!(
        names,
        [
            "列表首页分页",
            "深分页",
            "账户日期筛选列表",
            "全账户实时余额",
            "月度汇总",
            "分类占比",
            "商户占比",
            "备注搜索拼音过滤",
            "商品名搜索",
            "净资产总览",
            "持仓列表",
            "时点持仓",
            "投资组合趋势",
            "资金加权收益率",
            "财务自由度",
            "跨账本投资汇总",
        ]
    );
    for r in &results {
        assert_eq!(r.iterations, 2, "基准 {} 迭代次数应与配置一致", r.name);
        assert!(
            r.min_ms.is_finite() && r.min_ms >= 0.0,
            "基准 {} min 非法",
            r.name
        );
        assert!(r.avg_ms >= r.min_ms, "基准 {} avg 应不小于 min", r.name);
        assert!(r.p95_ms >= r.min_ms, "基准 {} p95 应不小于 min", r.name);
        assert!(!r.context.is_empty(), "基准 {} 应携带规模备注", r.name);
    }
    let search = results
        .iter()
        .find(|r| r.name == "备注搜索拼音过滤")
        .unwrap();
    assert!(
        search.context.contains("命中"),
        "搜索基准备注应含命中数：{}",
        search.context
    );
    let purchase_search = results.iter().find(|r| r.name == "商品名搜索").unwrap();
    assert!(
        purchase_search.context.contains("命中"),
        "商品名搜索基准备注应含命中数：{}",
        purchase_search.context
    );
    let cross_book = results.iter().find(|r| r.name == "跨账本投资汇总").unwrap();
    assert!(
        cross_book
            .context
            .contains(&format!("{} 本计入", books::ATTACHED_BOOK_TOTAL + 1)),
        "跨账本基准备注应报告计入本数（附属 + 主库）：{}",
        cross_book.context
    );
}

#[test]
fn bench_fails_fast_when_attached_books_missing() {
    let (_dir, path) = temp_db("cross-book-missing");
    build(&path, 1_000, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    let conn = open_connection(&path).unwrap();

    let err = bench::run_benchmarks(
        &conn,
        &BenchConfig {
            warmup: 0,
            iterations: 1,
            search_term: "咖啡".to_string(),
            purchase_search_term: "猫粮".to_string(),
            books_dir: books::attached_books_root(&path),
        },
    )
    .unwrap_err();
    assert!(
        err.contains("附属账本"),
        "探测错误应指向附属账本夹具缺失：{err}"
    );
}

#[test]
fn attached_books_fixture_is_complete_multibook_dataset() {
    let (_dir, path) = temp_db("attached-books");
    build_with_books(&path, 1_000, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    let main = open_connection(&path).unwrap();
    let version = ledger_infra::db::schema_version(&main).unwrap();
    let discovered =
        books::discover_attached_books(&books::attached_books_root(&path), version).unwrap();
    assert_eq!(
        discovered.len(),
        books::ATTACHED_BOOK_TOTAL,
        "附属账本本数应与常量一致"
    );

    for book in &discovered {
        let conn =
            open_connection(book.dir.join(ledger_infra::db::data_location::DB_FILE_NAME)).unwrap();
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total as u64, books::ATTACHED_BOOK_TRANSACTIONS);
        let trades: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE kind IN ('buy','sell')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(trades > 0, "附属账本应含标的交易");
        let holdings = investment::list_holdings(&conn).unwrap();
        assert!(!holdings.is_empty(), "附属账本应有批次持仓");
        let instruments = investment::list_instruments(&conn, &InstrumentListFilter::default())
            .unwrap()
            .items;
        assert_eq!(instruments.len(), 20);
        assert_eq!(
            ledger_transaction::amount::default_currency_code(&conn).unwrap(),
            "CNY"
        );
    }

    std::fs::remove_dir_all(&discovered[0].dir).unwrap();
    assert!(
        books::discover_attached_books(&books::attached_books_root(&path), version).is_err(),
        "附属账本缺本时前置探测必须失败"
    );
}

#[test]
fn attached_books_deterministic_across_regenerations() {
    let end = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
    let (_dir_a, path_a) = temp_db("books-det-a");
    let (_dir_b, path_b) = temp_db("books-det-b");
    build_with_books(&path_a, 1_000, end);
    build_with_books(&path_b, 1_000, end);

    let book_db =
        |root: &Path, i: usize| books::attached_book_db_path(&books::attached_books_root(root), i);
    for i in 0..books::ATTACHED_BOOK_TOTAL {
        let digest_a = digest_db(&book_db(&path_a, i)).unwrap();
        let digest_b = digest_db(&book_db(&path_b, i)).unwrap();
        assert_eq!(
            digest_a, digest_b,
            "同参数两次生成的附属账本（book-{:02}）摘要必须一致",
            i
        );
    }
    let d0 = digest_db(&book_db(&path_a, 0)).unwrap();
    let d1 = digest_db(&book_db(&path_a, 1)).unwrap();
    assert_ne!(
        d0["transactions"], d1["transactions"],
        "种子派生应使各附属账本内容互不相同"
    );
}

fn parse_bench_import_cli(args: &[&str]) -> Result<BenchImportCli, String> {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    match bench_import::parse_bench_import_args(&owned)? {
        Parsed::Run(cli) => Ok(cli),
        Parsed::Help => panic!("该输入应解析为运行参数"),
    }
}

fn small_import_cfg() -> ImportBenchConfig {
    ImportBenchConfig {
        rows: vec![50],
        dedup: true,
        warmup: 1,
        iterations: 2,
    }
}

#[test]
fn bench_import_cli_defaults_cover_typical_and_stress_tiers() {
    let cli = parse_bench_import_cli(&[]).unwrap();
    assert_eq!(
        cli.rows,
        vec![50, 100, 200],
        "默认矩阵应按月导入真实量级校准：轻量/典型/上限月（单批最多上百笔）"
    );
    assert!(
        cli.dedup,
        "默认去重应与 HTTP 批量导入生产默认（dedup=true）一致"
    );
    assert_eq!(cli.warmup, 1, "导入迭代成本高，预热默认 1 次");
    assert_eq!(cli.iterations, 5);
    assert_eq!(cli.db, super::default_out());
}

#[test]
fn bench_import_cli_parses_overrides() {
    let cli = parse_bench_import_cli(&[
        "--rows",
        "100,200",
        "--dedup",
        "false",
        "--warmup",
        "0",
        "--iterations",
        "2",
    ])
    .unwrap();
    assert_eq!(cli.rows, vec![100, 200]);
    assert!(!cli.dedup);
    assert_eq!(cli.warmup, 0);
    assert_eq!(cli.iterations, 2);
}

#[test]
fn bench_import_cli_rejects_bad_values() {
    assert!(
        parse_bench_import_cli(&["--rows", ""]).is_err(),
        "空矩阵应报错"
    );
    assert!(
        parse_bench_import_cli(&["--rows", "0"]).is_err(),
        "行数 0 应报错"
    );
    assert!(
        parse_bench_import_cli(&["--rows", "1,x"]).is_err(),
        "非数档位应报错"
    );
    assert!(
        parse_bench_import_cli(&["--rows", "100,100"]).is_err(),
        "重复档位应报错"
    );
    assert!(
        parse_bench_import_cli(&["--dedup", "yes"]).is_err(),
        "非布尔应报错"
    );
    assert!(
        parse_bench_import_cli(&["--iterations", "0"]).is_err(),
        "迭代 0 应报错"
    );
}

#[test]
fn bench_import_cli_error_messages_are_verbatim() {
    assert_eq!(
        parse_bench_import_cli(&["--rows", "1,x"]).unwrap_err(),
        "--rows 档位需要非负整数，得到 \"x\""
    );
    assert_eq!(
        parse_bench_import_cli(&["--rows", "0"]).unwrap_err(),
        "--rows 档位必须大于 0"
    );
    assert_eq!(
        parse_bench_import_cli(&["--rows", "100,100"]).unwrap_err(),
        "--rows 档位重复：100"
    );
    assert_eq!(
        parse_bench_import_cli(&["--rows", "1000001"]).unwrap_err(),
        "--rows 档位超过上限 1000000：1000001"
    );
    assert_eq!(
        parse_bench_import_cli(&["--dedup", "yes"]).unwrap_err(),
        "布尔参数需要 true/false，得到 \"yes\""
    );
    assert_eq!(
        parse_bench_import_cli(&["--warmup", "x"]).unwrap_err(),
        "--warmup 需要非负整数，得到 \"x\""
    );
    assert_eq!(
        parse_bench_import_cli(&["--iterations", "0"]).unwrap_err(),
        "--iterations 至少为 1"
    );
    assert_eq!(
        parse_bench_import_cli(&["--rows"]).unwrap_err(),
        "--rows 缺少值"
    );
    assert_eq!(
        parse_bench_import_cli(&["--unknown", "1"]).unwrap_err(),
        "未知参数 \"--unknown\""
    );
}

#[test]
fn bench_import_rows_are_deterministic_dedup_unique_and_distribution_shaped() {
    let accounts: Vec<String> = (0..3).map(|i| format!("acc-{i}")).collect();
    let date = "2026-01-01";

    let concentrated =
        bench_import::generate_inputs(5, &accounts, Distribution::Concentrated, "CNY", date);
    let replay =
        bench_import::generate_inputs(5, &accounts, Distribution::Concentrated, "CNY", date);
    let uniform = bench_import::generate_inputs(5, &accounts, Distribution::Uniform, "CNY", date);

    let hashes = |rows: &[ledger_transaction::TransactionInput]| {
        rows.iter().map(compute_dedup_hash).collect::<Vec<_>>()
    };
    assert_eq!(
        hashes(&concentrated),
        hashes(&replay),
        "同参数两次生成应逐行同身份（可复现规格）"
    );

    assert!(
        concentrated
            .iter()
            .all(|i| i.account_id.as_deref() == Some(accounts[0].as_str())),
        "同账户集中应全部落首账户"
    );
    let uniform_accounts: Vec<&str> = uniform
        .iter()
        .map(|i| i.account_id.as_deref().unwrap_or(""))
        .collect();
    assert_eq!(
        uniform_accounts,
        vec!["acc-0", "acc-1", "acc-2", "acc-0", "acc-1"],
        "多账户均匀应按序轮转"
    );

    for rows in [&concentrated, &uniform] {
        let mut seen = std::collections::HashSet::new();
        for h in hashes(rows) {
            assert!(seen.insert(h), "批内去重身份必须全异（否则行会被去重跳过）");
        }
    }

    for (i, input) in concentrated.iter().enumerate() {
        assert_eq!(input.kind, TransactionKind::Expense);
        assert_eq!(
            input.amount_cents,
            bench_import::BASE_AMOUNT_CENTS + i as i64
        );
        assert_eq!(input.date, date);
        assert_eq!(input.currency_code, "CNY");
    }
}

fn acct(id: &str, kind: AccountType, ccy: &str) -> Account {
    Account {
        id: id.to_string(),
        name: id.to_string(),
        kind,
        currency_code: ccy.to_string(),
        initial_balance_cents: 0,
        created_at: FIXED_NOW.to_string(),
        updated_at: FIXED_NOW.to_string(),
        version: 1,
        device_id: "test".to_string(),
        is_deleted: false,
        is_hidden: false,
        is_archived: false,
        credit_limit_cents: None,
        statement_day: None,
        due_day: None,
    }
}

#[test]
fn bench_import_eligible_accounts_filter_excludes_investment_and_foreign() {
    let conn = test_support::open();
    let all = vec![
        acct("a-cny-cash", AccountType::Cash, "CNY"),
        acct("a-cny-inv", AccountType::Investment, "CNY"),
        acct("a-usd-bank", AccountType::Bank, "USD"),
        acct("a-cny-credit", AccountType::Credit, "CNY"),
    ];
    assert_eq!(
        bench_import::eligible_account_ids(&conn, &all).unwrap(),
        vec!["a-cny-cash".to_string(), "a-cny-credit".to_string()],
        "投资户与外币户不进导入基准账户池（本位币折算与投资副作用都不属被测路径）"
    );
}

#[test]
fn bench_import_cache_assertion_accepts_healthy_and_rejects_drift() {
    let (_dir, path) = temp_db("bench-import-assert");
    build(&path, 500, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    let conn = open_connection(&path).unwrap();

    bench_import::assert_cache_matches_realtime(&conn).unwrap();

    conn.execute(
        "UPDATE account_balance_cache SET balance_cents = balance_cents + 1",
        [],
    )
    .unwrap();
    let err = bench_import::assert_cache_matches_realtime(&conn).unwrap_err();
    assert!(err.contains("缓存"), "漂移错误应指向缓存不一致：{err}");

    conn.execute("DELETE FROM account_balance_cache", [])
        .unwrap();
    assert!(
        bench_import::assert_cache_matches_realtime(&conn).is_err(),
        "缓存行缺失应让断言失败（否则会静默跑在坏缓存路径上）"
    );
}

#[test]
fn bench_import_smoke_runs_matrix_and_produces_all_metrics() {
    let (_dir, path) = temp_db("bench-import-smoke");
    build(&path, 2_000, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());

    let results = bench_import::run_benchmark(&path, &small_import_cfg()).unwrap();

    let names: Vec<String> = results.iter().map(|r| r.name.clone()).collect();
    assert_eq!(names, ["导入 50 行·同账户集中", "导入 50 行·多账户均匀"],);
    let distributions: Vec<Distribution> = results.iter().map(|r| r.distribution).collect();
    assert_eq!(
        distributions,
        [Distribution::Concentrated, Distribution::Uniform]
    );
    for r in &results {
        assert_eq!(r.rows, 50, "指标行应携带行数档：{}", r.name);
        assert!(
            r.min_ms.is_finite() && r.min_ms >= 0.0,
            "{} min 非法",
            r.name
        );
        assert!(r.avg_ms >= r.min_ms, "{} avg 应不小于 min", r.name);
        assert!(r.p95_ms >= r.min_ms, "{} p95 应不小于 min", r.name);
        assert!(
            r.per_row_p95_ms > 0.0 && r.per_row_p95_ms <= r.p95_ms,
            "{} 单行均摊 p95 应在 (0, p95] 内",
            r.name
        );
        assert!(
            r.context.contains("单行均摊"),
            "规模备注应携带单行均摊口径：{}",
            r.context
        );
    }
}

#[test]
fn p95_nearest_rank_is_true_quantile_at_n20() {
    let ten: Vec<f64> = (1..=10).map(|i| i as f64).collect();
    assert_eq!(bench_common::percentile_ms(&ten, 0.95), 10.0);
    let twenty: Vec<f64> = (1..=20).map(|i| i as f64).collect();
    assert_eq!(bench_common::percentile_ms(&twenty, 0.95), 19.0);
}

#[test]
fn bench_common_tier_csv_keeps_order_and_enforces_nonzero_unique_cap() {
    let ok = bench_common::parse_tier_csv(" 104 , 1040 ", "--points").unwrap();
    assert_eq!(
        ok,
        vec![104, 1040],
        "应去除空白并保持给定次序（矩阵展开次序即报告次序）"
    );

    let err = bench_common::parse_tier_csv("1000001", "--points").unwrap_err();
    assert!(
        err.contains("--points") && err.contains("上限"),
        "超上限应报错并带参数名：{err}"
    );
    assert!(
        bench_common::parse_tier_csv("1000000", "--points").is_ok(),
        "恰在上界的档位应放行（刻意压测不被误伤）"
    );

    assert!(bench_common::parse_tier_csv("0", "--rows").is_err());
    assert!(bench_common::parse_tier_csv("5,5", "--ops").is_err());
    assert!(bench_common::parse_tier_csv("", "--rows").is_err());
    let err = bench_common::parse_tier_csv("1,x", "--rows").unwrap_err();
    assert!(err.contains("--rows"), "非数档位报错应带参数名：{err}");
}

#[test]
fn bench_common_tier_cap_is_wired_into_all_three_write_cli_parsers() {
    assert!(
        parse_bench_import_cli(&["--rows", "1000001"]).is_err(),
        "--rows 档位上限应接线"
    );
    assert!(
        parse_bench_sync_cli(&["--ops", "1000001"]).is_err(),
        "--ops 档位上限应接线"
    );
    assert!(
        parse_bench_market_cli(&["--points", "1000001"]).is_err(),
        "--points 档位上限应接线（采样日运算防 chrono 日期越界）"
    );
    assert_eq!(
        parse_bench_market_cli(&["--points", "1000000"])
            .unwrap()
            .points,
        vec![1_000_000],
        "恰在上界的档位应原样放行"
    );
}

#[test]
fn bench_common_cli_missing_value_errors_with_flag_name() {
    let err = parse_bench_import_cli(&["--rows"]).unwrap_err();
    assert!(
        err.contains("--rows") && err.contains("缺少值"),
        "缺值错误应带参数名：{err}"
    );
}

const HELP_GOLDEN: &str = r#"ledger-perf —— Ledger 性能基准工具

USAGE:
    ledger-perf <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
    generate      生成性能基准数据集（默认 50 万笔 Transaction 的多域画像 SQLite
                  库 + 2 本附属账本小库，issue #1630）
    bench         查询基准——16 项查询 × min/avg/p95 报告（issue #461）
    bench-import  批量导入写基准——固定行数 × 两种分布 × 总耗时/单行均摊 p95
                  （issue #532，纯观测无门禁）
    bench-sync    同步重放写基准——op 流重放（ingest_ops/apply_ops 权威入口）×
                  两种分布 × 总耗时/单 op 均摊 p95（issue #1628，剥网络，纯观测
                  无门禁）
    bench-market  行情/价格历史批量 upsert 写基准——点数档 × 两种分布 × 总耗
                  时/单点均摊 p95（issue #1629，剥网络，纯观测无门禁）

bench OPTIONS:
    --db <PATH>               目标库文件（默认同 generate 输出路径，须已生成）
    --warmup <N>              每项基准预热次数（默认 3，不计入统计）
    --iterations <N>          每项基准计时迭代次数（默认 20，n=20 才成真 p95 分
                              位数）
    --search <TERM>           中文子串搜索基准的关键字（默认 咖啡）
    --purchase-search <TERM>  商品名搜索基准的关键字（默认 猫粮）
    --max-p95-ms <MS>         默认门禁阈值（毫秒）：全部基准 p95 ≤ 各自阈值才退
                              出 0，任何一项超标即失败（CI 用；缺省不判定；分项
                              例外机制与现行清单见 ADR-0068）
    -h, --help                打印本说明

bench-import OPTIONS:
    --db <PATH>               源库文件（默认同 generate 输出路径，须已生成；本命
                              令不修改源库——内部建 pristine 快照，每次迭代从快
                              照恢复）
    --rows <CSV>              每档导入行数（默认 50,100,200；逗号分隔、保持次
                              序；单档上限 1000000，issue #1650）
    --dedup <BOOL>            批量导入去重开关（默认 true，HTTP 批量导入生产默
                              认）
    --warmup <N>              每档预热次数（默认 1，不计入统计）
    --iterations <N>          每档计时迭代次数（默认 5；每次迭代从快照恢复，数据
                              集规模固定）
    -h, --help                打印本说明

bench-sync OPTIONS:
    --db <PATH>               源库文件（默认同 generate 输出路径，须已生成；本命
                              令不修改源库——内部建 pristine 快照，每次迭代从快
                              照恢复）
    --ops <CSV>               每档重放 op 条数（默认 100,500,2000；逗号分隔、保
                              持次序；单档上限 1000000，issue #1650）
    --warmup <N>              每档预热次数（默认 1，不计入统计）
    --iterations <N>          每档计时迭代次数（默认 5；每次迭代从快照恢复，数据
                              集规模固定）
    -h, --help                打印本说明

bench-market OPTIONS:
    --db <PATH>               源库文件（默认同 generate 输出路径，须已生成；本命
                              令不修改源库——内部建 pristine 快照，每次迭代从快
                              照恢复）
    --points <CSV>            每档周采样点数（默认 104,1040,5200；逗号分隔、保持
                              次序；单档上限 1000000，采样日运算防 chrono 日期越
                              界，issue #1650）
    --warmup <N>              每档预热次数（默认 1，不计入统计）
    --iterations <N>          每档计时迭代次数（默认 5；每次迭代从快照恢复，数据
                              集规模固定）
    -h, --help                打印本说明

generate OPTIONS:
    --seed <N>                随机种子（默认 42，同种子必出同库）
    --transactions <N>        生成笔数（默认 500000）
    --end-date <YYYY-MM-DD>   数据窗口锚定结束日期（默认 2025-12-31，不锚定「今
                              天」）
    --out <PATH>              输出库文件路径（默认
                              src-tauri/target/ledger-perf/ledger-perf.db；已存
                              在会先删除再重建）
    -h, --help                打印本说明"#;

#[test]
fn usage_help_matches_golden() {
    assert_eq!(
        super::render_usage(),
        HELP_GOLDEN,
        "全文帮助与 golden 不一致——帮助措辞/组装/次序变化须有意修订 golden（issue #1679）"
    );
}

#[test]
fn help_renders_every_flag_registered_in_tables() {
    let usage = super::render_usage();
    for sub in super::SUBCOMMANDS {
        let flags = (sub.flags)();
        assert!(!flags.is_empty(), "{} 应登记 flag 表", sub.name);
        assert!(
            usage.contains(&format!("{} OPTIONS:", sub.name)),
            "{} 的 OPTIONS 节应出现在全文帮助中",
            sub.name
        );
        for f in &flags {
            assert!(
                usage.contains(f.flag),
                "{} 的 flag「{}」应出现在全文帮助中",
                sub.name,
                f.flag
            );
        }
    }
}

const EXPECTED_FLAGS: &[(&str, &[&str])] = &[
    (
        "generate",
        &["--seed", "--transactions", "--end-date", "--out"],
    ),
    (
        "bench",
        &[
            "--db",
            "--warmup",
            "--iterations",
            "--search",
            "--purchase-search",
            "--max-p95-ms",
        ],
    ),
    (
        "bench-import",
        &["--db", "--rows", "--dedup", "--warmup", "--iterations"],
    ),
    ("bench-sync", &["--db", "--ops", "--warmup", "--iterations"]),
    (
        "bench-market",
        &["--db", "--points", "--warmup", "--iterations"],
    ),
];

fn dispatch_entry(name: &str) -> &'static super::Subcommand {
    super::SUBCOMMANDS
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("子命令 {name} 应登记在 dispatch 表"))
}

#[test]
fn flag_tables_match_pinned_inventory() {
    assert_eq!(
        EXPECTED_FLAGS.len(),
        super::SUBCOMMANDS.len(),
        "名单应覆盖全部 dispatch 子命令"
    );
    for (name, expected) in EXPECTED_FLAGS {
        let flags = (dispatch_entry(name).flags)();
        let registered: Vec<&str> = flags.iter().map(|f| f.key()).collect();
        assert_eq!(
            registered, *expected,
            "{name} 的 flag 表与名单不一致——增删 flag 须同步钉住名单（双向全等）"
        );
    }
}

#[test]
fn every_expected_flag_is_accepted_by_parsers() {
    for (name, keys) in EXPECTED_FLAGS {
        let entry = dispatch_entry(name);
        for k in *keys {
            let key = k.to_string();
            match (entry.execute)(std::slice::from_ref(&key)) {
                Outcome::ParamError(msg) => {
                    assert_eq!(msg, format!("{key} 缺少值"), "{name} 的 {key} 应报缺值错误")
                }
                other => panic!("{name} 传 {key} 应报参数错误，得到 {other:?}"),
            }
        }
    }
}

#[test]
fn unknown_flag_is_rejected_by_every_subcommand() {
    for sub in super::SUBCOMMANDS {
        match (sub.execute)(&["--definitely-unknown".to_string()]) {
            Outcome::ParamError(msg) => assert_eq!(msg, "未知参数 \"--definitely-unknown\""),
            other => panic!("{} 应拒绝未知参数，得到 {:?}", sub.name, other),
        }
    }
}

#[test]
fn every_subcommand_help_flag_requests_full_help() {
    for sub in super::SUBCOMMANDS {
        assert_eq!(
            (sub.execute)(&["--help".to_string()]),
            Outcome::Help,
            "{} --help 应请求帮助",
            sub.name
        );
        assert_eq!(
            (sub.execute)(&["-h".to_string()]),
            Outcome::Help,
            "{} -h 应请求帮助",
            sub.name
        );
    }
}

fn assert_default_pins(usage: &str, pins: &[(&str, &str)]) {
    for (flag, expected) in pins {
        assert!(
            usage.contains(expected),
            "渲染帮助应含 {flag} 的默认值显示「{expected}」"
        );
    }
}

#[test]
fn generate_help_pins_default_values() {
    let d = GenerateCli::default();
    assert_eq!(
        [
            format!("默认 {}", d.seed),
            format!("默认 {}", d.transactions),
            format!("默认 {}", d.end_date),
        ],
        ["默认 42", "默认 500000", "默认 2025-12-31"]
    );
    assert!(
        d.out
            .to_string_lossy()
            .ends_with("src-tauri/target/ledger-perf/ledger-perf.db"),
        "--out 默认路径应为构建目标目录下的 ledger-perf.db，实际：{}",
        d.out.display()
    );
    assert_default_pins(
        &super::render_usage(),
        &[
            ("--seed <N>", "默认 42"),
            ("--transactions <N>", "默认 500000"),
            ("--end-date <YYYY-MM-DD>", "默认 2025-12-31"),
            (
                "--out <PATH>",
                "src-tauri/target/ledger-perf/ledger-perf.db",
            ),
        ],
    );
}

#[test]
fn bench_help_pins_default_values() {
    let d = BenchCli::default();
    assert_eq!(
        [
            format!("默认 {}", d.warmup),
            format!("默认 {}", d.iterations),
            format!("默认 {}", d.search),
        ],
        ["默认 3", "默认 20", "默认 咖啡"],
    );
    assert_eq!(
        d.max_p95_ms, None,
        "默认不启用门禁（帮助应显「缺省不判定」）"
    );
    assert_eq!(d.db, super::default_out(), "--db 默认同 generate 输出路径");
    assert_default_pins(
        &super::render_usage(),
        &[
            ("--db <PATH>", "默认同 generate 输出路径"),
            ("--warmup <N>", "默认 3"),
            ("--iterations <N>", "默认 20"),
            ("--search <TERM>", "默认 咖啡"),
            ("--max-p95-ms <MS>", "缺省不判定"),
        ],
    );
}

#[test]
fn bench_import_help_pins_default_values() {
    let d = BenchImportCli::default();
    let rows_csv = d
        .rows
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        [
            format!("默认 {rows_csv}"),
            format!("默认 {}", d.dedup),
            format!("默认 {}", d.warmup),
            format!("默认 {}", d.iterations),
        ],
        ["默认 50,100,200", "默认 true", "默认 1", "默认 5"]
    );
    assert_eq!(d.db, super::default_out());
    assert_default_pins(
        &super::render_usage(),
        &[
            ("--db <PATH>", "默认同 generate 输出路径"),
            ("--rows <CSV>", "默认 50,100,200"),
            ("--dedup <BOOL>", "默认 true"),
            ("--warmup <N>", "默认 1"),
            ("--iterations <N>", "默认 5"),
        ],
    );
}

#[test]
fn bench_sync_help_pins_default_values() {
    let d = BenchSyncCli::default();
    let ops_csv = d
        .ops
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        [
            format!("默认 {ops_csv}"),
            format!("默认 {}", d.warmup),
            format!("默认 {}", d.iterations),
        ],
        ["默认 100,500,2000", "默认 1", "默认 5"]
    );
    assert_eq!(d.db, super::default_out());
    assert_default_pins(
        &super::render_usage(),
        &[
            ("--db <PATH>", "默认同 generate 输出路径"),
            ("--ops <CSV>", "默认 100,500,2000"),
            ("--warmup <N>", "默认 1"),
            ("--iterations <N>", "默认 5"),
        ],
    );
}

#[test]
fn bench_market_help_pins_default_values() {
    let d = BenchMarketCli::default();
    let points_csv = d
        .points
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        [
            format!("默认 {points_csv}"),
            format!("默认 {}", d.warmup),
            format!("默认 {}", d.iterations),
        ],
        ["默认 104,1040,5200", "默认 1", "默认 5"]
    );
    assert_eq!(d.db, super::default_out());
    assert_default_pins(
        &super::render_usage(),
        &[
            ("--db <PATH>", "默认同 generate 输出路径"),
            ("--points <CSV>", "默认 104,1040,5200"),
            ("--warmup <N>", "默认 1"),
            ("--iterations <N>", "默认 5"),
        ],
    );
}

#[test]
fn bench_cli_projects_into_config_via_from() {
    let cli = parse_bench_cli(&[
        "--db",
        "/tmp/custom-ledger.db",
        "--warmup",
        "7",
        "--iterations",
        "9",
        "--search",
        "牛奶",
    ])
    .unwrap();
    let db = cli.db.clone();
    let cfg = BenchConfig::from(cli);
    assert_eq!(cfg.warmup, 7);
    assert_eq!(cfg.iterations, 9);
    assert_eq!(cfg.search_term, "牛奶");
    assert_eq!(
        cfg.books_dir,
        books::attached_books_root(&db),
        "books_dir 应由 --db 同级派生（From 投影内派生，不进 Config 字段）"
    );
}

#[test]
fn write_bench_clis_project_into_configs_via_from() {
    let cli = parse_bench_import_cli(&[
        "--rows",
        "7,9",
        "--dedup",
        "false",
        "--warmup",
        "3",
        "--iterations",
        "4",
    ])
    .unwrap();
    let cfg = ImportBenchConfig::from(cli);
    assert_eq!(cfg.rows, vec![7, 9]);
    assert!(!cfg.dedup);
    assert_eq!(cfg.warmup, 3);
    assert_eq!(cfg.iterations, 4);

    let cli = parse_bench_sync_cli(&["--ops", "8", "--warmup", "2", "--iterations", "6"]).unwrap();
    let cfg = SyncBenchConfig::from(cli);
    assert_eq!(cfg.ops, vec![8]);
    assert_eq!(cfg.warmup, 2);
    assert_eq!(cfg.iterations, 6);

    let cli =
        parse_bench_market_cli(&["--points", "9", "--warmup", "4", "--iterations", "7"]).unwrap();
    let cfg = bench_market::MarketBenchConfig::from(cli);
    assert_eq!(cfg.points, vec![9]);
    assert_eq!(cfg.warmup, 4);
    assert_eq!(cfg.iterations, 7);
}

#[test]
fn bench_common_summarize_matches_min_avg_nearest_rank_p95() {
    let s = bench_common::summarize(vec![
        Duration::from_millis(30),
        Duration::from_millis(10),
        Duration::from_millis(20),
        Duration::from_millis(40),
    ]);
    assert_eq!(s.min_ms, 10.0);
    assert_eq!(s.avg_ms, 25.0);
    assert_eq!(s.p95_ms, 40.0);
}

#[test]
fn bench_common_name_column_width_takes_longest_and_never_saturates() {
    assert_eq!(
        bench_common::name_column_width(["导入 200 行·同账户集中", "x"]),
        23
    );
    assert_eq!(
        bench_common::name_column_width(["行情 5200 点·多标的均匀"]),
        24
    );
    assert_eq!(bench_common::name_column_width([] as [&str; 0]), 4);
}

#[test]
fn bench_common_metric_table_header_aligns_labels_over_columns() {
    assert_eq!(
        bench_common::metric_table_header(10),
        "基准             min        avg        p95  规模备注（毫秒）"
    );
}

#[test]
fn bench_cli_gate_option_parses_and_defaults_off() {
    let cli = parse_bench_cli(&[]).unwrap();
    assert_eq!(cli.iterations, 20, "默认迭代次数应为 20");
    assert_eq!(cli.max_p95_ms, None, "默认不启用门禁");

    let cli = parse_bench_cli(&["--iterations", "20", "--max-p95-ms", "200"]).unwrap();
    assert_eq!(cli.iterations, 20);
    assert_eq!(cli.max_p95_ms, Some(200.0));

    let cli = parse_bench_cli(&["--max-p95-ms=200.5"]).unwrap();
    assert_eq!(cli.max_p95_ms, Some(200.5));

    for bad in ["abc", "-1", "0", "NaN", "inf"] {
        assert!(
            parse_bench_cli(&["--max-p95-ms", bad]).is_err(),
            "--max-p95-ms {bad} 应报错"
        );
    }
    assert!(parse_bench_cli(&["--max-p95-ms"]).is_err(), "缺值报错");
}

#[test]
fn bench_cli_error_messages_are_verbatim() {
    assert_eq!(
        parse_bench_cli(&["--warmup", "x"]).unwrap_err(),
        "--warmup 需要非负整数，得到 \"x\""
    );
    assert_eq!(
        parse_bench_cli(&["--iterations", "abc"]).unwrap_err(),
        "--iterations 需要非负整数，得到 \"abc\""
    );
    assert_eq!(
        parse_bench_cli(&["--max-p95-ms", "abc"]).unwrap_err(),
        "--max-p95-ms 需要正数（毫秒），得到 \"abc\""
    );
    assert_eq!(
        parse_bench_cli(&["--max-p95-ms", "-1"]).unwrap_err(),
        "--max-p95-ms 需要正数（毫秒），得到 \"-1\""
    );
    assert_eq!(
        parse_bench_cli(&["--iterations", "0"]).unwrap_err(),
        "--iterations 至少为 1"
    );
    assert_eq!(parse_bench_cli(&["--db"]).unwrap_err(), "--db 缺少值");
    assert_eq!(
        parse_bench_cli(&["--unknown", "1"]).unwrap_err(),
        "未知参数 \"--unknown\""
    );
}

#[test]
fn gate_failures_lists_only_over_threshold_items() {
    let mk = |name: &'static str, p95: f64| BenchMetrics {
        name,
        context: format!("p95={p95}"),
        min_ms: p95,
        avg_ms: p95,
        p95_ms: p95,
        iterations: 20,
    };
    let results = vec![
        mk("列表首页分页", 10.0),
        mk("深分页", 200.0),
        mk("月度汇总", 250.5),
        mk("时点持仓", 1000.0),
    ];
    assert_eq!(
        bench::gate_failures(&results, 200.0),
        vec![
            "月度汇总（p95 250.50ms > 阈值 200ms）",
            "时点持仓（p95 1000.00ms > 阈值 200ms）",
        ],
        "只列超标项（含各自阈值），等号边界达标"
    );

    let ok = vec![mk("列表首页分页", 199.9), mk("深分页", 200.0)];
    assert!(bench::gate_failures(&ok, 200.0).is_empty());
}

#[test]
fn gate_search_benchmarks_on_default_line_after_exception_revoked() {
    let mk = |name: &'static str, p95: f64| BenchMetrics {
        name,
        context: String::new(),
        min_ms: p95,
        avg_ms: p95,
        p95_ms: p95,
        iterations: 20,
    };
    let results = vec![
        mk("列表首页分页", 250.0),
        mk("备注搜索拼音过滤", 313.0),
        mk("月度汇总", 313.0),
    ];
    assert_eq!(
        bench::gate_failures(&results, 200.0),
        vec![
            "列表首页分页（p95 250.00ms > 阈值 200ms）",
            "备注搜索拼音过滤（p95 313.00ms > 阈值 200ms）",
            "月度汇总（p95 313.00ms > 阈值 200ms）",
        ],
        "例外表为空：搜索基准与其它基准共用默认线，逐项判定含各自阈值"
    );

    let measured = vec![mk("备注搜索拼音过滤", 69.52), mk("分类占比", 99.12)];
    assert!(bench::gate_failures(&measured, 200.0).is_empty());

    assert!(
        bench::PER_BENCH_MAX_P95_MS.is_empty(),
        "分项例外表应恰为 ADR-0068 声明的清单（当前为空）"
    );
}

fn digest_db(path: &PathBuf) -> Result<BTreeMap<String, String>, String> {
    use rusqlite::types::ValueRef;
    use sha2::Digest;

    const AUDIT_COLS: [&str; 2] = ["created_at", "updated_at"];
    let conn = open_connection(path).map_err(|e| e.to_string())?;
    const TABLES: [&str; 20] = [
        "accounts",
        "categories",
        "merchants",
        "transactions",
        "transaction_purchases",
        "exchange_rates",
        "fx_rate_history",
        "currencies",
        "instruments",
        "security_transactions",
        "security_lots",
        "security_lot_sales",
        "market_prices",
        "price_history",
        "budgets",
        "scheduled_transactions",
        "scheduled_transaction_occurrences",
        "installment_plans",
        "subscription_plans",
        "scheduled_transfer_plans",
    ];
    let mut out = BTreeMap::new();
    for table in TABLES {
        let mut stmt = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .map_err(|e| e.to_string())?;
        let col_count = stmt.column_count();
        let keep: Vec<bool> = (0..col_count)
            .map(|c| !AUDIT_COLS.contains(&stmt.column_name(c).unwrap_or_default()))
            .collect();
        let mut hasher = sha2::Sha256::new();
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            for (col, &keep_col) in keep.iter().enumerate() {
                if !keep_col {
                    continue;
                }
                match row.get_ref(col).map_err(|e| e.to_string())? {
                    ValueRef::Null => hasher.update(b"\x00N"),
                    ValueRef::Integer(i) => hasher.update(i.to_le_bytes()),
                    ValueRef::Real(f) => hasher.update(f.to_le_bytes()),
                    ValueRef::Text(t) => {
                        hasher.update(t);
                        hasher.update(b"|");
                    }
                    ValueRef::Blob(b) => hasher.update(b),
                }
            }
            hasher.update(b"\n");
        }
        let hex: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        out.insert(table.to_string(), hex);
    }
    Ok(out)
}

fn user_version(conn: &Connection) -> Result<i64, String> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())
}

const PROFILE_N: u64 = 4000;

fn temp_db(tag: &str) -> (tauri_app_lib::test_support::ScratchDir, PathBuf) {
    let dir = tauri_app_lib::test_support::ScratchDir::new(&format!("perf-test-{tag}"));
    let db = dir.join(ledger_infra::db::data_location::DB_FILE_NAME);
    (dir, db)
}

fn build(path: &Path, transactions: u64, end_date: NaiveDate) -> GenCounts {
    crate::install_process_wiring();
    let mut conn = open_connection_in(path.parent().expect("临时库目录")).unwrap();
    generate_into(
        &mut conn,
        &GenerateParams {
            seed: 42,
            transactions,
            end_date,
        },
    )
    .unwrap()
}

fn build_with_books(path: &Path, transactions: u64, end_date: NaiveDate) -> GenCounts {
    let counts = build(path, transactions, end_date);
    books::generate_attached_books(
        path,
        &GenerateParams {
            seed: 42,
            transactions,
            end_date,
        },
    )
    .unwrap();
    counts
}

#[test]
fn generated_schema_matches_product_migration_path() {
    let (_dir, path) = temp_db("schema");
    build(&path, 300, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());

    let conn = open_connection(&path).unwrap();
    let mem = test_support::open();

    let file_version = user_version(&conn).unwrap();
    assert_eq!(file_version, user_version(&mem).unwrap());
    assert!(
        file_version > 0,
        "迁移应已应用：user_version = {file_version}"
    );

    let mut stmt = conn.prepare("PRAGMA foreign_key_check").unwrap();
    let mut rows = stmt.query([]).unwrap();
    let violation = rows.next().unwrap();
    assert!(violation.is_none(), "外键完整性违例：{:?}", violation);
}

#[test]
fn profile_matches_spec_counts() {
    let (_dir, path) = temp_db("profile-counts");
    let counts = build(
        &path,
        PROFILE_N,
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
    );
    let conn = open_connection(&path).unwrap();

    let mem = test_support::open();

    let mem_accounts = accounts::list_accounts(&mem).unwrap().len();
    assert_eq!(
        accounts::list_accounts(&conn).unwrap().len(),
        mem_accounts + 50
    );

    let mem_categories = categories::list_categories(&mem, false).unwrap().len();
    let cats = categories::list_categories(&conn, false).unwrap();
    assert_eq!(cats.len(), mem_categories + 40);
    assert_eq!(
        cats.iter().filter(|c| c.name.starts_with("基准")).count(),
        40
    );

    assert_eq!(merchants::list_merchants(&conn, false).unwrap().len(), 800);

    let mem_currencies = currencies::list_currencies(&mem).unwrap().len();
    assert_eq!(
        currencies::list_currencies(&conn).unwrap().len(),
        mem_currencies
    );

    let active = list_transactions(&conn, &TransactionListFilter::default())
        .unwrap()
        .total;
    assert_eq!(
        (active as u64) + counts.deleted_transactions as u64,
        PROFILE_N
    );
    assert_eq!(counts.transactions as u64, PROFILE_N);
}

#[test]
fn profile_kind_mix_and_soft_delete() {
    let (_dir, path) = temp_db("profile-mix");
    build(
        &path,
        PROFILE_N,
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
    );
    let conn = open_connection(&path).unwrap();

    let kind_total = |k: TransactionKind| {
        list_transactions(
            &conn,
            &TransactionListFilter {
                kinds: Some(vec![k]),
                ..Default::default()
            },
        )
        .unwrap()
        .total as f64
    };
    let n = PROFILE_N as f64;
    let transfers = kind_total(TransactionKind::Transfer);
    assert!(
        (transfers / n - 0.08).abs() < 0.012,
        "转账占比偏离 8%：{transfers}"
    );
    let refunds = kind_total(TransactionKind::Refund);
    assert!(
        (refunds / n - 0.02).abs() < 0.008,
        "退款占比偏离 2%：{refunds}"
    );

    let active = list_transactions(&conn, &TransactionListFilter::default())
        .unwrap()
        .total as f64;
    let deleted = n - active;
    assert!(
        (deleted / n - 0.01).abs() < 0.006,
        "软删除占比偏离 1%：{deleted}"
    );
}

#[test]
fn profile_merchant_long_tail() {
    let (_dir, path) = temp_db("profile-merchants");
    build(
        &path,
        PROFILE_N,
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
    );
    let conn = open_connection(&path).unwrap();

    let mut counts: Vec<i64> = merchants::transaction_counts(&conn)
        .unwrap()
        .into_iter()
        .map(|c| c.transaction_count)
        .collect();
    assert_eq!(counts.len(), 800);
    counts.sort_by(|a, b| b.cmp(a));
    let attached: f64 = counts.iter().sum::<i64>() as f64;
    let top20: f64 = counts[..20].iter().sum::<i64>() as f64;
    let share = top20 / attached;
    assert!(
        (share - 0.60).abs() < 0.05,
        "top 20 商户流水占比偏离 60%：{share}"
    );
    assert!(
        counts[799] as f64 / attached < 0.005,
        "最尾部商户占比应远小于均值"
    );
}

#[test]
fn profile_foreign_currency_and_fx_history() {
    let (_dir, path) = temp_db("profile-fx");
    build(
        &path,
        PROFILE_N,
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
    );
    let conn = open_connection(&path).unwrap();

    let foreign: u64 = accounts::list_accounts(&conn)
        .unwrap()
        .iter()
        .filter(|a| a.currency_code != "CNY")
        .map(|a| {
            list_transactions(
                &conn,
                &TransactionListFilter {
                    involving_account_id: Some(a.id.clone()),
                    ..Default::default()
                },
            )
            .unwrap()
            .total as u64
        })
        .sum();
    let share = foreign as f64 / PROFILE_N as f64;
    assert!((share - 0.08).abs() < 0.03, "外币流水占比偏离 8%：{share}");

    let sample = list_transactions(
        &conn,
        &TransactionListFilter {
            limit: Some(500),
            ..Default::default()
        },
    )
    .unwrap()
    .items;
    for t in &sample {
        if t.currency_code == "CNY" {
            assert_eq!(t.amount_native_cents, t.amount_cents);
        } else {
            assert!(
                t.amount_native_cents != t.amount_cents,
                "外币应折算：{}",
                t.id
            );
        }
    }

    let rate_pairs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM exchange_rates WHERE quote_code='CNY'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rate_pairs, 3);
    let history: i64 = conn
        .query_row("SELECT COUNT(*) FROM fx_rate_history", [], |r| r.get(0))
        .unwrap();
    assert!(
        history >= 500,
        "汇率历史应全窗口填充（≥250 周 × 2 币对）：{history}"
    );
    let history_span: (String, String) = conn
        .query_row(
            "SELECT MIN(trade_date), MAX(trade_date) FROM fx_rate_history",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(history_span.1, "2025-12-31", "历史应填充到锚定结束日");
}

#[test]
fn refund_chains_reference_earlier_expenses() {
    let (_dir, path) = temp_db("refund-chain");
    build(
        &path,
        PROFILE_N,
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
    );
    let conn = open_connection(&path).unwrap();

    let refunds = list_transactions(
        &conn,
        &TransactionListFilter {
            kinds: Some(vec![TransactionKind::Refund]),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(refunds.total > 0, "应有退款链");
    let mut resolved = 0usize;
    for r in refunds.items.iter().take(50) {
        let orig_id = r.refund_of_transaction_id.clone().unwrap();
        let orig = match get_transaction(&conn, &orig_id) {
            Ok(orig) => orig,
            Err(_) => continue,
        };
        resolved += 1;
        assert_eq!(orig.kind, TransactionKind::Expense, "退款必须指向支出");
        assert_eq!(orig.account_id, r.account_id, "退款账户继承原支出");
        assert_eq!(orig.currency_code, r.currency_code, "退款币种继承原支出");
        assert_eq!(orig.category_id, r.category_id, "退款分类继承原支出");
        assert!(r.amount_cents <= orig.amount_cents, "退款不超过原金额");
        assert!(r.date.as_str() >= orig.date.as_str(), "退款不早于原支出");
    }
    assert!(
        resolved > 25,
        "至少半数样本的原支出应可读（未软删）：{resolved}"
    );
}

#[test]
fn transfers_have_two_accounts() {
    let (_dir, path) = temp_db("transfers");
    build(&path, 800, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    let conn = open_connection(&path).unwrap();

    let transfers = list_transactions(
        &conn,
        &TransactionListFilter {
            kinds: Some(vec![TransactionKind::Transfer]),
            limit: Some(50),
            ..Default::default()
        },
    )
    .unwrap();
    for t in &transfers.items {
        let to = t.to_account_id.as_ref().expect("转账必须有转入账户");
        assert_ne!(
            t.account_id.as_deref(),
            Some(to.as_str()),
            "转账两端不同账户"
        );
        assert!(t.category_id.is_none(), "转账不挂分类");
        assert!(t.merchant_id.is_none(), "转账不挂商户");
    }
}

#[test]
fn same_seed_produces_identical_digest() {
    let (_dir_a, path_a) = temp_db("det-a");
    let (_dir_b, path_b) = temp_db("det-b");
    let (_dir_c, path_c) = temp_db("det-c");
    let end = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
    build(&path_a, 1500, end);
    build(&path_b, 1500, end);
    let mut conn = open_connection_in(path_c.parent().unwrap()).unwrap();
    generate_into(
        &mut conn,
        &GenerateParams {
            seed: 43,
            transactions: 1500,
            end_date: end,
        },
    )
    .unwrap();
    drop(conn);

    let digest_a = digest_db(&path_a).unwrap();
    let digest_b = digest_db(&path_b).unwrap();
    let digest_c = digest_db(&path_c).unwrap();
    assert_eq!(digest_a, digest_b, "同种子两次生成的全表有序摘要必须一致");
    assert_ne!(
        digest_a["transactions"], digest_c["transactions"],
        "不同种子应产出不同交易数据"
    );
    assert_ne!(
        digest_a["transaction_purchases"], digest_c["transaction_purchases"],
        "不同种子应产出不同购买项语料（摘要含购买项子表，issue #1915）"
    );
    assert_eq!(digest_a["currencies"], digest_c["currencies"]);
}

#[test]
fn purchase_corpus_pool_shape_and_keyword_isolation() {
    let pool = purchase_name_pool();
    assert_eq!(
        pool.len(),
        PURCHASE_BRANDS.len() * PURCHASE_CATEGORIES.len() + PURCHASE_REPEAT_SKUS.len(),
        "池规模应与三轴构成一致（品牌 × 品类交叉 + 复购单品块）"
    );
    assert!(
        pool.len() >= 300,
        "池应达数百个名称的 SKU 级多样性：{}",
        pool.len()
    );
    assert!(pool.iter().all(|n| !n.is_empty()), "名称池不应含空条目");

    let hits: Vec<&String> = pool.iter().filter(|n| n.contains("猫粮")).collect();
    assert_eq!(
        hits.len(),
        2,
        "「猫粮」系名称应恰为复购单品块两条：{hits:?}"
    );
    let p_item = hits.len() as f64 / pool.len() as f64;
    let p_order = (1..=PURCHASE_ITEM_MAX as i32)
        .map(|items| 1.0 - (1.0 - p_item).powi(items))
        .sum::<f64>()
        / PURCHASE_ITEM_MAX as f64;
    assert!(
        p_order <= 0.02,
        "默认关键字的订单级命中率应 ≤2%（真实复购单品画像）：{p_order:.4}"
    );

    for name in &pool {
        for token in NOTE_SUBJECTS
            .iter()
            .chain(NOTE_SUFFIXES.iter())
            .filter(|t| !t.is_empty())
        {
            assert!(
                !name.contains(token),
                "商品名「{name}」不应含备注素材词面「{token}」"
            );
        }
        assert!(
            !name.contains("咖啡"),
            "商品名「{name}」不应含备注搜索基准关键字「咖啡」"
        );
    }
}

#[test]
fn purchase_corpus_keyword_hit_count_is_pinned() {
    let (_dir, path) = temp_db("purchase-hit-pin");
    build(
        &path,
        10_000,
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
    );
    let conn = open_connection(&path).unwrap();

    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM transaction_purchases", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        rows, 1_929,
        "购买项行数应钉住（订单率 × 每单件数的确定性事实）"
    );

    let result =
        search_transactions_internal(&conn, "猫粮", 1, 20, None, None, None, None).unwrap();
    assert_eq!(
        result.total, 14,
        "「猫粮」命中订单数应钉住（默认关键字命中数百单量级的语料画像，10k 笔库缩影）"
    );
}

#[test]
fn cli_defaults_and_overrides_parse() {
    let cli = run_cli(&[]);
    assert_eq!(cli, GenerateCli::default());
    assert_eq!(cli.seed, 42);
    assert_eq!(cli.transactions, 500_000);
    assert_eq!(cli.end_date, "2025-12-31");
    assert!(
        cli.out
            .to_string_lossy()
            .ends_with("target/ledger-perf/ledger-perf.db")
    );

    let cli = run_cli(&[
        "--seed",
        "7",
        "--transactions=123",
        "--end-date",
        "2024-06-30",
        "--out",
        "/tmp/x.db",
    ]);
    assert_eq!(cli.seed, 7);
    assert_eq!(cli.transactions, 123);
    assert_eq!(cli.end_date, "2024-06-30");
    assert_eq!(cli.out, PathBuf::from("/tmp/x.db"));

    assert!(
        parse_generate_args(&["--nope".to_string()]).is_err(),
        "未知参数报错"
    );
    assert!(
        parse_generate_args(&["--seed".to_string()]).is_err(),
        "缺值报错"
    );
    assert!(
        parse_generate_args(&["--seed".to_string(), "abc".to_string()]).is_err(),
        "非整数报错"
    );
    assert_eq!(
        parse_generate_args(&["--help".to_string()]),
        Ok(Parsed::Help),
        "--help 请求"
    );
}

#[test]
fn generate_cli_error_messages_are_verbatim() {
    let owned = |args: &[&str]| -> Vec<String> { args.iter().map(|s| s.to_string()).collect() };
    assert_eq!(
        parse_generate_args(&owned(&["--seed", "abc"])).unwrap_err(),
        "--seed 需要非负整数，得到 \"abc\""
    );
    assert_eq!(
        parse_generate_args(&owned(&["--transactions", "x"])).unwrap_err(),
        "--transactions 需要非负整数，得到 \"x\""
    );
    assert_eq!(
        parse_generate_args(&owned(&["--seed"])).unwrap_err(),
        "--seed 缺少值"
    );
    assert_eq!(
        parse_generate_args(&owned(&["--nope"])).unwrap_err(),
        "未知参数 \"--nope\""
    );
}

#[test]
fn params_take_effect_on_output() {
    let (dir, path) = temp_db("params");
    let end = NaiveDate::from_ymd_opt(2023, 9, 30).unwrap();
    let counts = build(&path, 777, end);

    assert!(path.exists(), "--out 指定路径应有库文件");
    let conn = open_connection(&path).unwrap();
    let active = list_transactions(&conn, &TransactionListFilter::default())
        .unwrap()
        .total;
    assert_eq!(
        (active as u64) + counts.deleted_transactions as u64,
        777,
        "--transactions 应决定生成笔数"
    );
    let after_end = list_transactions(
        &conn,
        &TransactionListFilter {
            from: Some("2023-10-01".to_string()),
            ..Default::default()
        },
    )
    .unwrap()
    .total;
    assert_eq!(after_end, 0, "不应有晚于 --end-date 的交易");
    let before_window = list_transactions(
        &conn,
        &TransactionListFilter {
            to: Some("2018-09-30".to_string()),
            ..Default::default()
        },
    )
    .unwrap()
    .total;
    assert_eq!(before_window, 0, "不应有早于窗口起点（end 前 5 年）的交易");
    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn regeneration_overwrites_existing_file() {
    let (dir, path) = temp_db("overwrite");
    let cli = || GenerateCli {
        seed: 42,
        transactions: 500,
        end_date: "2025-12-31".to_string(),
        out: path.clone(),
    };
    super::generate::run(cli()).unwrap();
    super::generate::run(cli()).unwrap();
    let conn = open_connection(&path).unwrap();
    assert_eq!(
        merchants::list_merchants(&conn, false).unwrap().len(),
        800,
        "重复生成不叠加商户"
    );
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 500, "重复生成不叠加交易");
    drop(conn);
    for i in 0..books::ATTACHED_BOOK_TOTAL {
        let book_conn = open_connection(books::attached_book_db_path(
            &books::attached_books_root(&path),
            i,
        ))
        .unwrap();
        let total: i64 = book_conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            total as u64,
            books::ATTACHED_BOOK_TRANSACTIONS,
            "重复生成不叠加附属账本交易（book-{:02}）",
            i
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

const INVESTMENT_PROFILE_N: u64 = 20_000;

#[test]
fn profile_investments_holdings_and_trades() {
    let (_dir, path) = temp_db("profile-investments");
    let counts = build(
        &path,
        INVESTMENT_PROFILE_N,
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
    );
    let conn = open_connection(&path).unwrap();

    let instruments = investment::list_instruments(&conn, &InstrumentListFilter::default())
        .unwrap()
        .items;
    assert_eq!(instruments.len(), 20);
    assert_eq!(
        instruments
            .iter()
            .filter(|i| i.source == investment::MANUAL_SOURCE
                && i.kind == investment::InstrumentType::Fund)
            .count(),
        3,
        "场外基金标的手动来源"
    );

    let (buys, sells) = (counts.buy_trades as f64, counts.sell_trades as f64);
    let n = INVESTMENT_PROFILE_N as f64;
    assert!(
        (buys / n - 0.004).abs() < 0.002,
        "买入占比偏离 0.4%：{buys}"
    );
    assert!(
        (sells / n - 0.002).abs() < 0.0015,
        "卖出占比偏离 0.2%：{sells}"
    );

    let misplaced: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions t JOIN accounts a ON a.id=t.account_id \
             WHERE t.kind IN ('buy','sell') AND a.type != 'investment'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(misplaced, 0, "标的交易必须落在投资账户");
    let deleted_trades: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE kind IN ('buy','sell') AND is_deleted=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        deleted_trades, 0,
        "标的交易不软删（产品删除会回滚批次副作用）"
    );

    let holdings = investment::list_holdings(&conn).unwrap();
    assert!(!holdings.is_empty(), "持仓视图应非空");
    for h in &holdings {
        assert!(h.quantity > 0.0);
        assert!(
            h.market_value_cents.is_some(),
            "持仓 {} 市值应可折算",
            h.instrument_id
        );
        assert!(h.unrealized_pnl_cents.is_some());
        assert!(h.latest_price_cents.is_some());
    }
    let h = &holdings[0];
    let price = h.latest_price_cents.unwrap();
    let expected = (h.quantity * price as f64 / 100.0).round() as i64;
    assert_eq!(
        h.market_value_cents.unwrap(),
        expected,
        "同币种市值 = 数量×现价÷100"
    );

    let mut stmt = conn
        .prepare(
            "SELECT l.initial_quantity, l.remaining_quantity, \
             (SELECT COALESCE(SUM(s.quantity),0) FROM security_lot_sales s WHERE s.lot_id=l.id) \
             FROM security_lots l",
        )
        .unwrap();
    let mut rows = stmt.query([]).unwrap();
    let mut checked = 0;
    while let Some(row) = rows.next().unwrap() {
        let initial: f64 = row.get(0).unwrap();
        let remaining: f64 = row.get(1).unwrap();
        let sold: f64 = row.get(2).unwrap();
        assert!(
            (initial - remaining - sold).abs() < 1e-6,
            "批次闭合失败：{initial} − {remaining} ≠ {sold}"
        );
        checked += 1;
    }
    assert!(checked > 0, "应有批次可校验");

    let partial: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots \
             WHERE remaining_quantity > 0 AND remaining_quantity < initial_quantity",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(partial > 0, "应有部分卖出批次");
    let exhausted: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE remaining_quantity = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(exhausted > 0, "应有清仓批次");

    let sell_before_buy: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lot_sales s \
             JOIN security_lots l ON l.id = s.lot_id \
             JOIN transactions tb ON tb.id = l.buy_transaction_id \
             JOIN transactions ts ON ts.id = s.sell_transaction_id \
             WHERE ts.date < tb.date",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sell_before_buy, 0, "不得存在早于买入日的卖出");

    let fund_price_deviation: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions st \
             JOIN instruments i ON i.id = st.instrument_id \
             JOIN transactions t ON t.id = st.transaction_id \
             WHERE i.instrument_type = 'fund' AND st.action = 'buy' \
             AND st.price_cents > 1.01 * (SELECT ph.price_cents FROM price_history ph \
                 WHERE ph.instrument_id = st.instrument_id AND ph.trade_date <= t.date \
                 ORDER BY ph.trade_date DESC LIMIT 1) + 1 \
             OR (i.instrument_type = 'fund' AND st.action = 'buy' \
             AND st.price_cents < 0.99 * (SELECT ph.price_cents FROM price_history ph \
                 WHERE ph.instrument_id = st.instrument_id AND ph.trade_date <= t.date \
                 ORDER BY ph.trade_date DESC LIMIT 1) - 1)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fund_price_deviation, 0, "基金反算单价应贴合当日价格线");
}

#[test]
fn profile_market_data_shape() {
    let (_dir, path) = temp_db("profile-market-data");
    build(&path, 2_000, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    let conn = open_connection(&path).unwrap();

    let prices = investment::list_market_prices(&conn).unwrap();
    assert_eq!(prices.len(), 20);
    let per_instrument: i64 = conn
        .query_row(
            "SELECT MIN(c) FROM (SELECT COUNT(*) AS c FROM price_history GROUP BY instrument_id)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        per_instrument >= 250,
        "每标的周采样应覆盖全窗口：{per_instrument}"
    );
    let history_total: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(history_total, per_instrument * 20);

    let mismatch: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM market_prices mp \
             WHERE mp.price_cents != (SELECT ph.price_cents FROM price_history ph \
               WHERE ph.instrument_id = mp.instrument_id ORDER BY ph.trade_date DESC LIMIT 1) \
             OR mp.priced_at != (SELECT ph.trade_date FROM price_history ph \
               WHERE ph.instrument_id = mp.instrument_id ORDER BY ph.trade_date DESC LIMIT 1)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mismatch, 0, "现价必须等于该标的最新历史点");

    let funds_without_nav: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM market_prices mp JOIN instruments i ON i.id=mp.instrument_id \
             WHERE i.instrument_type='fund' AND mp.nav_date IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(funds_without_nav, 0, "基金现价应带净值日期");
    let stocks_with_nav: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM market_prices mp JOIN instruments i ON i.id=mp.instrument_id \
             WHERE i.instrument_type != 'fund' AND mp.nav_date IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stocks_with_nav, 0, "非基金现价不应有净值日期");
    let bad_source: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM market_prices WHERE source != 'eastmoney'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bad_source, 0);

    let hkd_instruments: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE currency_code='HKD' AND market='hk'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(hkd_instruments > 0, "应有港股标的");
    let hkd_rate: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM exchange_rates WHERE base_code='HKD' AND quote_code='CNY'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hkd_rate, 1);
}

#[test]
fn trade_amounts_follow_product_invariants() {
    let (_dir, path) = temp_db("trade-invariants");
    build(
        &path,
        INVESTMENT_PROFILE_N,
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
    );
    let conn = open_connection(&path).unwrap();

    let mut stmt = conn
        .prepare(
            "SELECT t.kind, t.amount_cents, st.action, st.quantity, st.price_cents, st.fee_cents, \
             i.instrument_type FROM transactions t \
             JOIN security_transactions st ON st.transaction_id=t.id \
             JOIN instruments i ON i.id=st.instrument_id",
        )
        .unwrap();
    let mut rows = stmt.query([]).unwrap();
    let mut checked = 0;
    while let Some(row) = rows.next().unwrap() {
        let kind: String = row.get(0).unwrap();
        let amount: i64 = row.get(1).unwrap();
        let quantity: f64 = row.get(3).unwrap();
        let price: i64 = row.get(4).unwrap();
        let fee: i64 = row.get(5).unwrap();
        let itype: String = row.get(6).unwrap();
        let gross = (quantity * price as f64 / 100.0).round() as i64;
        if itype == "fund" {
            let derived = if kind == "buy" {
                ((amount - fee) as f64 * 100.0 / quantity).round() as i64
            } else {
                ((amount + fee) as f64 * 100.0 / quantity).round() as i64
            };
            assert_eq!(price, derived, "基金单价应由金额反算");
        } else if kind == "buy" {
            assert_eq!(amount, gross + fee, "买入金额 = 数量×单价÷100 + 费");
        } else {
            assert_eq!(amount, gross - fee, "卖出金额 = 数量×单价÷100 − 费");
            assert!(fee <= gross, "卖出费不超过毛收入");
        }
        checked += 1;
    }
    assert!(checked > 0, "应有标的交易可校验");
}

#[test]
fn profile_budgets_and_scheduled_plans() {
    let (_dir, path) = temp_db("profile-plans");
    let counts = build(&path, 2_000, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    let conn = open_connection(&path).unwrap();

    let budgets = budget::list_budgets(&conn).unwrap();
    assert_eq!(budgets.len(), 6);
    assert_eq!(counts.budgets, 6);
    assert_eq!(
        budgets
            .iter()
            .filter(|b| b.period == budget::BudgetPeriod::Monthly)
            .count(),
        4
    );
    let mut seen: Vec<(String, String)> = Vec::new();
    for b in &budgets {
        assert!(b.amount_cents > 0);
        let kind: String = conn
            .query_row(
                "SELECT kind FROM categories WHERE id=?1",
                rusqlite::params![b.category_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "expense", "预算只能挂支出分类");
        let key = (
            b.category_id.clone(),
            format!("{:?}", b.period).to_lowercase(),
        );
        assert!(!seen.contains(&key), "分类+周期不得重复：{key:?}");
        seen.push(key);
    }

    let plans = scheduled_transactions::list_plans(&conn).unwrap();
    assert_eq!(plans.len(), 8);
    let kind_count = |k: scheduled_transactions::ScheduledKind| {
        plans.iter().filter(|p| p.core.kind == k).count()
    };
    assert_eq!(
        kind_count(scheduled_transactions::ScheduledKind::Installment),
        3
    );
    assert_eq!(
        kind_count(scheduled_transactions::ScheduledKind::Subscription),
        3
    );
    assert_eq!(
        kind_count(scheduled_transactions::ScheduledKind::ScheduledTransfer),
        2
    );
    assert_eq!(
        plans.iter().filter(|p| p.core.status == "paused").count(),
        1
    );
    for (table, expected) in [
        ("installment_plans", 3),
        ("subscription_plans", 3),
        ("scheduled_transfer_plans", 2),
    ] {
        let n: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, expected, "{table} 行数应与计划形态一一对应");
    }

    let status_count = |s: &str| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM scheduled_transaction_occurrences WHERE status=?1",
            rusqlite::params![s],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(status_count("completed"), 37);
    assert_eq!(status_count("pending"), 21);
    assert_eq!(status_count("failed"), 2);
    assert_eq!(status_count("cancelled"), 1);
    let orphan_completed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM scheduled_transaction_occurrences \
             WHERE status='completed' AND transaction_id IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphan_completed, 0, "completed 期次必须关联交易");
    let linked_non_completed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM scheduled_transaction_occurrences \
             WHERE status != 'completed' AND transaction_id IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(linked_non_completed, 0, "非 completed 期次不关联交易");
    assert_eq!(
        counts.scheduled_occurrences as i64,
        status_count("completed")
            + status_count("pending")
            + status_count("failed")
            + status_count("cancelled")
    );

    let shape: Vec<(String, i64)> = conn
        .prepare(
            "SELECT o.status, COUNT(*) FROM scheduled_transaction_occurrences o \
             JOIN transactions t ON t.id=o.transaction_id WHERE o.status='completed' \
             GROUP BY o.status",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(shape.len(), 1);
    assert_eq!(shape[0].1, 37);
    let amount_mismatch: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM scheduled_transaction_occurrences o \
             JOIN transactions t ON t.id=o.transaction_id \
             JOIN scheduled_transactions s ON s.id=o.scheduled_transaction_id \
             WHERE t.amount_cents != s.amount_cents OR t.is_deleted != 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount_mismatch, 0, "期次交易金额应等于计划金额且未删除");
    let bad_kind: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM scheduled_transaction_occurrences o \
             JOIN transactions t ON t.id=o.transaction_id \
             JOIN scheduled_transactions s ON s.id=o.scheduled_transaction_id \
             WHERE (s.kind = 'scheduled_transfer' AND (t.kind != 'transfer' OR t.to_account_id IS NULL)) \
                OR (s.kind != 'scheduled_transfer' AND t.kind != 'expense')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bad_kind, 0, "期次交易 kind 应随计划形态");

    assert_eq!(
        counts.transactions as u64, 2_000,
        "交易总数应等于 --transactions（含预留期次交易）"
    );
}

fn parse_bench_sync_cli(args: &[&str]) -> Result<BenchSyncCli, String> {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    match bench_sync::parse_bench_sync_args(&owned)? {
        Parsed::Run(cli) => Ok(cli),
        Parsed::Help => panic!("该输入应解析为运行参数"),
    }
}

fn small_sync_cfg() -> SyncBenchConfig {
    SyncBenchConfig {
        ops: vec![50],
        warmup: 1,
        iterations: 2,
    }
}

#[test]
fn bench_sync_cli_defaults_cover_sync_round_tiers() {
    let cli = parse_bench_sync_cli(&[]).unwrap();
    assert_eq!(
        cli.ops,
        vec![100, 500, 2000],
        "默认矩阵应按同步轮真实量级校准：日常轮 / 离线一周积压 / 离线一月上限形态"
    );
    assert_eq!(cli.warmup, 1, "同步重放迭代成本高，预热默认 1 次");
    assert_eq!(cli.iterations, 5);
    assert_eq!(cli.db, super::default_out());
}

#[test]
fn bench_sync_cli_parses_overrides_and_rejects_bad_values() {
    let cli =
        parse_bench_sync_cli(&["--ops", "100,200", "--warmup", "0", "--iterations", "2"]).unwrap();
    assert_eq!(cli.ops, vec![100, 200]);
    assert_eq!(cli.warmup, 0);
    assert_eq!(cli.iterations, 2);
    let cli = parse_bench_sync_cli(&["--ops=100"]).unwrap();
    assert_eq!(cli.ops, vec![100]);

    assert!(
        parse_bench_sync_cli(&["--ops", ""]).is_err(),
        "空矩阵应报错"
    );
    assert!(
        parse_bench_sync_cli(&["--ops", "0"]).is_err(),
        "op 档 0 应报错"
    );
    assert!(
        parse_bench_sync_cli(&["--ops", "1,x"]).is_err(),
        "非数档位应报错"
    );
    assert!(
        parse_bench_sync_cli(&["--ops", "100,100"]).is_err(),
        "重复档位应报错"
    );
    assert!(
        parse_bench_sync_cli(&["--iterations", "0"]).is_err(),
        "迭代 0 应报错"
    );
    assert!(
        parse_bench_sync_cli(&["--unknown", "1"]).is_err(),
        "未知参数应报错"
    );
}

#[test]
fn bench_sync_cli_error_messages_are_verbatim() {
    assert_eq!(
        parse_bench_sync_cli(&["--ops", "1,x"]).unwrap_err(),
        "--ops 档位需要非负整数，得到 \"x\""
    );
    assert_eq!(
        parse_bench_sync_cli(&["--ops", "0"]).unwrap_err(),
        "--ops 档位必须大于 0"
    );
    assert_eq!(
        parse_bench_sync_cli(&["--ops", "100,100"]).unwrap_err(),
        "--ops 档位重复：100"
    );
    assert_eq!(
        parse_bench_sync_cli(&["--warmup", "x"]).unwrap_err(),
        "--warmup 需要非负整数，得到 \"x\""
    );
    assert_eq!(
        parse_bench_sync_cli(&["--iterations", "0"]).unwrap_err(),
        "--iterations 至少为 1"
    );
    assert_eq!(
        parse_bench_sync_cli(&["--ops"]).unwrap_err(),
        "--ops 缺少值"
    );
    assert_eq!(
        parse_bench_sync_cli(&["--unknown", "1"]).unwrap_err(),
        "未知参数 \"--unknown\""
    );
}

#[test]
fn bench_sync_ops_are_deterministic_and_distribution_shaped() {
    let accounts: Vec<String> = (0..3).map(|i| format!("acc-{i}")).collect();
    let date = "2026-01-01";
    let schema_version = 99;

    let concentrated = generate_ops(
        5,
        &accounts,
        Distribution::Concentrated,
        "CNY",
        date,
        schema_version,
    );
    let replay = generate_ops(
        5,
        &accounts,
        Distribution::Concentrated,
        "CNY",
        date,
        schema_version,
    );
    let uniform = generate_ops(
        5,
        &accounts,
        Distribution::Uniform,
        "CNY",
        date,
        schema_version,
    );

    assert_eq!(concentrated, replay, "同参数两次生成应逐条相同");

    let account_of = |op: &ledger_sync_engine::SyncOp| match &op.command {
        ledger_sync_engine::DomainCommand::Transaction(
            ledger_transaction::TransactionCommand::Create { row, .. },
        ) => row.account_id.clone(),
        other => panic!("op 流应全为交易创建命令，得到 {other:?}"),
    };
    assert!(
        concentrated
            .iter()
            .all(|op| account_of(op) == Some(accounts[0].clone())),
        "同账户集中应全部落首账户"
    );
    let uniform_accounts: Vec<String> = uniform
        .iter()
        .map(|op| account_of(op).unwrap_or_default())
        .collect();
    assert_eq!(
        uniform_accounts,
        vec!["acc-0", "acc-1", "acc-2", "acc-0", "acc-1"],
        "多账户均匀应按序轮转"
    );

    let mut op_ids = std::collections::HashSet::new();
    let mut txn_ids = std::collections::HashSet::new();
    for (i, op) in concentrated.iter().enumerate() {
        assert!(op_ids.insert(op.op_id.clone()), "op_id 应全新全异");
        assert_eq!(op.device_id, SOURCE_DEVICE_ID, "源端设备应恒定");
        assert_eq!(op.clock, i as i64 + 1, "逻辑时钟应自 1 单调递增");
        assert_eq!(op.schema_version, schema_version);
        let ledger_sync_engine::DomainCommand::Transaction(
            ledger_transaction::TransactionCommand::Create { id, row, .. },
        ) = &op.command
        else {
            panic!("op 流应全为交易创建命令");
        };
        assert!(txn_ids.insert(id.clone()), "交易 id 应全新全异");
        assert_eq!(row.kind, TransactionKind::Expense);
        assert_eq!(
            row.amount_cents,
            super::bench_import::BASE_AMOUNT_CENTS + i as i64
        );
        assert_eq!(row.amount_native_cents, row.amount_cents);
        assert_eq!(row.fx_rate_used, None);
        assert_eq!(row.fx_rate_source, None);
        assert_eq!(row.currency_code, "CNY");
        assert_eq!(row.date, date);
    }
}

#[test]
fn bench_sync_wire_form_roundtrips_to_identical_ops() {
    let accounts: Vec<String> = (0..2).map(|i| format!("acc-{i}")).collect();
    let stream = generate_ops(4, &accounts, Distribution::Uniform, "CNY", "2026-01-01", 99);
    let wire = generate_wire(&stream).unwrap();

    assert_eq!(wire.len(), stream.len());
    for (raw, op) in wire.iter().zip(&stream) {
        let parsed: ledger_sync_engine::SyncOp = serde_json::from_str(raw).unwrap();
        assert_eq!(&parsed, op, "wire 报文反解应与原 op 逐字段相等");
    }
}

#[test]
fn bench_sync_smoke_runs_matrix_and_produces_all_metrics() {
    ledger_accounts::balance::install_balance_refresh_hook();
    ledger_scheduled::install_plan_source_hook();
    ledger_scheduled::auto_run::register_after_occurrence_hook(
        backup_domain::occurrence_dirty_hook,
    );
    backup_domain::register_catch_up_hook(ledger_scheduled::auto_run::catch_up_hook);
    tauri_app_lib::transaction_wiring::install_all();

    let (_dir, path) = temp_db("bench-sync-smoke");
    build(&path, 2_000, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());

    let results = bench_sync::run_benchmark(&path, &small_sync_cfg()).unwrap();

    let names: Vec<String> = results.iter().map(|r| r.name.clone()).collect();
    assert_eq!(
        names,
        [
            "同步 50 op·同账户集中·wire 接入",
            "同步 50 op·同账户集中·进程内重放",
            "同步 50 op·多账户均匀·wire 接入",
            "同步 50 op·多账户均匀·进程内重放",
        ]
    );
    let distributions: Vec<Distribution> = results.iter().map(|r| r.distribution).collect();
    assert_eq!(
        distributions,
        [
            Distribution::Concentrated,
            Distribution::Concentrated,
            Distribution::Uniform,
            Distribution::Uniform,
        ],
        "分布轴展开次序：同账户集中在前（Distribution::ALL 稳定清单）"
    );
    let entries: Vec<EntryForm> = results.iter().map(|r| r.entry).collect();
    assert_eq!(
        entries,
        [
            EntryForm::Ingest,
            EntryForm::Apply,
            EntryForm::Ingest,
            EntryForm::Apply,
        ],
        "入口轴展开次序：生产通道形态在前（EntryForm::ALL 稳定清单）"
    );
    for r in &results {
        assert_eq!(r.ops, 50, "指标行应携带 op 档：{}", r.name);
        assert!(
            r.min_ms.is_finite() && r.min_ms >= 0.0,
            "{} min 非法",
            r.name
        );
        assert!(r.avg_ms >= r.min_ms, "{} avg 应不小于 min", r.name);
        assert!(r.p95_ms >= r.min_ms, "{} p95 应不小于 min", r.name);
        assert!(
            r.per_op_p95_ms > 0.0 && r.per_op_p95_ms <= r.p95_ms,
            "{} 单 op 均摊 p95 应在 (0, p95] 内",
            r.name
        );
        assert!(
            r.context.contains("单 op 均摊")
                && (r.context.contains("ingest_ops") || r.context.contains("apply_ops")),
            "规模备注应携带单 op 均摊口径与入口锚点：{}",
            r.context
        );
    }
}

fn parse_bench_market_cli(args: &[&str]) -> Result<BenchMarketCli, String> {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    match bench_market::parse_bench_market_args(&owned)? {
        Parsed::Run(cli) => Ok(cli),
        Parsed::Help => panic!("该输入应解析为运行参数"),
    }
}

#[test]
fn bench_market_cli_defaults_cover_market_refill_tiers() {
    let cli = parse_bench_market_cli(&[]).unwrap();
    assert_eq!(
        cli.points,
        vec![104, 1040, 5200],
        "默认矩阵应按同步落库真实量级校准：单只近两年整根 / 部分批量重刷 / 全库价格线量级"
    );
    assert_eq!(cli.warmup, 1, "批量落库迭代成本高，预热默认 1 次");
    assert_eq!(cli.iterations, 5);
    assert_eq!(cli.db, super::default_out());
}

#[test]
fn bench_market_cli_parses_overrides_and_rejects_bad_values() {
    let cli = parse_bench_market_cli(&["--points", "10,20", "--warmup", "0", "--iterations", "2"])
        .unwrap();
    assert_eq!(cli.points, vec![10, 20]);
    assert_eq!(cli.warmup, 0);
    assert_eq!(cli.iterations, 2);
    let cli = parse_bench_market_cli(&["--points=10"]).unwrap();
    assert_eq!(cli.points, vec![10]);

    assert!(
        parse_bench_market_cli(&["--points", ""]).is_err(),
        "空矩阵应报错"
    );
    assert!(
        parse_bench_market_cli(&["--points", "0"]).is_err(),
        "点数档 0 应报错"
    );
    assert!(
        parse_bench_market_cli(&["--points", "1,x"]).is_err(),
        "非数档位应报错"
    );
    assert!(
        parse_bench_market_cli(&["--points", "10,10"]).is_err(),
        "重复档位应报错"
    );
    assert!(
        parse_bench_market_cli(&["--iterations", "0"]).is_err(),
        "迭代 0 应报错"
    );
    assert!(
        parse_bench_market_cli(&["--unknown", "1"]).is_err(),
        "未知参数应报错"
    );
}

#[test]
fn bench_market_cli_error_messages_are_verbatim() {
    assert_eq!(
        parse_bench_market_cli(&["--points", "1,x"]).unwrap_err(),
        "--points 档位需要非负整数，得到 \"x\""
    );
    assert_eq!(
        parse_bench_market_cli(&["--points", "0"]).unwrap_err(),
        "--points 档位必须大于 0"
    );
    assert_eq!(
        parse_bench_market_cli(&["--points", "10,10"]).unwrap_err(),
        "--points 档位重复：10"
    );
    assert_eq!(
        parse_bench_market_cli(&["--warmup", "x"]).unwrap_err(),
        "--warmup 需要非负整数，得到 \"x\""
    );
    assert_eq!(
        parse_bench_market_cli(&["--iterations", "0"]).unwrap_err(),
        "--iterations 至少为 1"
    );
    assert_eq!(
        parse_bench_market_cli(&["--points"]).unwrap_err(),
        "--points 缺少值"
    );
    assert_eq!(
        parse_bench_market_cli(&["--unknown", "1"]).unwrap_err(),
        "未知参数 \"--unknown\""
    );
}

fn market_pool() -> Vec<bench_market::PoolInstrument> {
    use bench_market::PoolInstrument;
    vec![
        PoolInstrument {
            instrument_id: "inst-stock-0".to_string(),
            currency_code: "CNY".to_string(),
            is_fund: false,
            source: ledger_investment::prices::TENCENT_PRICE_SOURCE,
        },
        PoolInstrument {
            instrument_id: "inst-stock-1".to_string(),
            currency_code: "HKD".to_string(),
            is_fund: false,
            source: ledger_investment::prices::TENCENT_PRICE_SOURCE,
        },
        PoolInstrument {
            instrument_id: "inst-fund-2".to_string(),
            currency_code: "CNY".to_string(),
            is_fund: true,
            source: ledger_investment::prices::SINA_PRICE_SOURCE,
        },
    ]
}

#[test]
fn bench_market_plan_is_deterministic_and_spread_shaped() {
    use bench_market::generate_price_plan;
    let pool = market_pool();
    let anchor = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();

    let concentrated = generate_price_plan(5, &pool, Spread::Concentrated, anchor);
    let replay = generate_price_plan(5, &pool, Spread::Concentrated, anchor);
    let uniform = generate_price_plan(5, &pool, Spread::Uniform, anchor);

    assert_eq!(concentrated, replay, "同参数两次生成应逐条相同");

    assert_eq!(concentrated.len(), 1, "单标的集中应只有首标的一条序列");
    assert_eq!(concentrated[0].pool_index, 0, "单标的集中应落首标的");
    assert_eq!(concentrated[0].points.len(), 5);
    assert_eq!(
        uniform.iter().map(|s| s.pool_index).collect::<Vec<_>>(),
        vec![0, 1, 2],
        "多标的均匀应按池序覆盖每只标的"
    );
    assert_eq!(uniform[0].points.len(), 2);
    assert_eq!(uniform[1].points.len(), 2);
    assert_eq!(uniform[2].points.len(), 1);
    let block_dates = |s: &bench_market::PlannedSeries| -> Vec<String> {
        s.points.iter().map(|p| p.trade_date.clone()).collect()
    };
    assert_eq!(
        block_dates(&uniform[0]),
        ["2026-01-09", "2026-01-16"],
        "均匀分布每只应得一段连续周（首块）"
    );
    assert_eq!(
        block_dates(&uniform[1]),
        ["2026-01-23", "2026-01-30"],
        "第二块应紧接首块之后逐周递进"
    );
    assert_eq!(block_dates(&uniform[2]), ["2026-02-06"], "末块应收短不越界");

    assert_eq!(
        concentrated.iter().map(|s| s.points.len()).sum::<usize>(),
        5
    );
    assert_eq!(uniform.iter().map(|s| s.points.len()).sum::<usize>(), 5);

    let dates: Vec<&str> = concentrated[0]
        .points
        .iter()
        .map(|p| p.trade_date.as_str())
        .collect();
    assert_eq!(
        dates,
        [
            "2026-01-09",
            "2026-01-16",
            "2026-01-23",
            "2026-01-30",
            "2026-02-06"
        ],
        "采样周应自锚点周一逐周递进、采样日取当周周五"
    );

    let prices: Vec<i64> = concentrated[0]
        .points
        .iter()
        .map(|p| p.price_units)
        .collect();
    assert_eq!(
        prices,
        vec![
            bench_market::BASE_PRICE_UNITS,
            bench_market::BASE_PRICE_UNITS + 1,
            bench_market::BASE_PRICE_UNITS + 2,
            bench_market::BASE_PRICE_UNITS + 3,
            bench_market::BASE_PRICE_UNITS + 4,
        ],
        "价格应逐点递增（BASE + 全局序号）"
    );
    assert_eq!(
        uniform[1].points[1].price_units,
        bench_market::BASE_PRICE_UNITS + 3,
        "均匀分布的价格应随全局序号递增（第二块末点 = 全局第 3 点）"
    );
}

fn small_market_cfg() -> bench_market::MarketBenchConfig {
    bench_market::MarketBenchConfig {
        points: vec![50],
        warmup: 1,
        iterations: 2,
    }
}

#[test]
fn bench_market_probe_surveys_channel_pool_and_anchor_week() {
    let (_dir, path) = temp_db("bench-market-probe");
    build(&path, 300, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    let conn = open_connection(&path).unwrap();

    let probe = bench_market::probe_market_dataset(&conn).unwrap();

    assert_eq!(
        probe.pool.len(),
        20,
        "标的池应覆盖生成库全部行情/净值通道标的（手动/恒定/无来源不属同步采集面）"
    );
    assert_eq!(
        probe.pool.iter().filter(|p| p.is_fund).count(),
        3,
        "场外基金应随字典形态标记（priced_at=净值日期、来源新浪）"
    );
    assert!(
        probe.pool.iter().all(
            |p| p.source == ledger_investment::prices::TENCENT_PRICE_SOURCE
                || p.source == ledger_investment::prices::SINA_PRICE_SOURCE
        ),
        "来源标记应按通道映射（场内腾讯/场外新浪，ADR-0130 决策 7）"
    );
    assert_eq!(
        probe.anchor_monday,
        NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
        "锚点周应为数据集最大交易日期次日（2026-01-01）后的第一个周一"
    );
    assert!(
        probe.pristine_history_rows > 0,
        "生成库应已有价格历史行（周采样全窗口）"
    );
}

#[test]
fn bench_market_smoke_runs_matrix_and_produces_all_metrics() {
    let (_dir, path) = temp_db("bench-market-smoke");
    build(&path, 2_000, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    let pristine = {
        let conn = open_connection(&path).unwrap();
        bench_market::count_price_history_rows(&conn).unwrap()
    };

    let results = bench_market::run_benchmark(&path, &small_market_cfg()).unwrap();

    let names: Vec<String> = results.iter().map(|r| r.name.clone()).collect();
    assert_eq!(names, ["行情 50 点·单标的集中", "行情 50 点·多标的均匀"],);
    let spreads: Vec<Spread> = results.iter().map(|r| r.spread).collect();
    assert_eq!(
        spreads,
        [Spread::Concentrated, Spread::Uniform],
        "分布轴展开次序：单标的集中在前（Spread::ALL 稳定清单）"
    );
    for r in &results {
        assert_eq!(r.points, 50, "指标行应携带点数档：{}", r.name);
        assert!(
            r.min_ms.is_finite() && r.min_ms >= 0.0,
            "{} min 非法",
            r.name
        );
        assert!(r.avg_ms >= r.min_ms, "{} avg 应不小于 min", r.name);
        assert!(r.p95_ms >= r.min_ms, "{} p95 应不小于 min", r.name);
        assert!(
            r.per_point_p95_ms > 0.0 && r.per_point_p95_ms <= r.p95_ms,
            "{} 单点均摊 p95 应在 (0, p95] 内",
            r.name
        );
        assert!(
            r.context.contains("单点均摊") && r.context.contains("点"),
            "规模备注应携带单点均摊口径：{}",
            r.context
        );
    }

    let conn = open_connection(&path).unwrap();
    assert_eq!(
        bench_market::count_price_history_rows(&conn).unwrap(),
        pristine,
        "源库不应被基准修改（pristine 快照机制的源库侧闭环）"
    );
}
