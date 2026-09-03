//! 生成器正确性单测（issue #459 验收项：写在 bin 模块内部、随常规测试循环运行）。
//!
//! 断言接缝（spec #458 测试决策）：标准连接工厂
//! （[`tauri_app_lib::db::open_connection`] 打开生成的文件库、`open_in_memory`
//! 对照产品迁移路径）+ 现有查询函数层（accounts / categories / merchants /
//! transaction 读取接口）；仅 schema 级事实（user_version / foreign_key_check）
//! 与无既有读 API 的画像事实（fx_rate_history 行数）用 PRAGMA / 原生 SQL。
//! 测试一律用小规模参数（数百至数千笔），50 万笔默认规模只在手动验证时跑。

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::NaiveDate;
use rusqlite::Connection;

use tauri_app_lib::accounts;
use tauri_app_lib::budget;
use tauri_app_lib::categories;
use tauri_app_lib::currencies;
use tauri_app_lib::db::{init_db, open_connection, open_in_memory};
use tauri_app_lib::investment::{self, InstrumentListFilter};
use tauri_app_lib::merchants;
use tauri_app_lib::scheduled_transactions;
use tauri_app_lib::transaction::TransactionListFilter;
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::read::{get_transaction, list_transactions};

use super::bench::{self, BenchConfig};
use super::generate::{GenCounts, GenerateParams, generate_into};
use super::{GenerateCli, ParsedArgs, parse_args};

/// 解析并取运行参数（帮助请求在该测试套件中不该出现）。
fn run_cli(args: &[&str]) -> GenerateCli {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    match parse_args(&owned).unwrap() {
        ParsedArgs::Run(cli) => cli,
        ParsedArgs::Help => panic!("该输入应解析为运行参数"),
    }
}

// ---------------------------------------------------------------------------
// bench 冒烟（issue #461 验收项：小规模参数生成小库 → 跑完全部基准 → 产出全部指标）
// ---------------------------------------------------------------------------

#[test]
fn bench_smoke_runs_all_ten_benchmarks() {
    let (_dir, path) = temp_db("bench-smoke");
    build(&path, 1_000, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    let conn = open_connection(&path).unwrap();

    let results = bench::run_benchmarks(
        &conn,
        &BenchConfig {
            warmup: 0,
            iterations: 2,
            search_term: "咖啡".to_string(),
        },
    )
    .unwrap();

    // 名单钉住：10 项基准一个不少、顺序稳定（增删基准必须显式更新本断言）。
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
            "备注搜索拼音过滤",
            "净资产总览",
            "持仓列表",
            "时点持仓",
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
    // 搜索基准确实产出命中（生成器备注池保证「咖啡」可命中），拼音过滤路径被驱动。
    let search = results
        .iter()
        .find(|r| r.name == "备注搜索拼音过滤")
        .unwrap();
    assert!(
        search.context.contains("命中"),
        "搜索基准备注应含命中数：{}",
        search.context
    );
}

// ---------------------------------------------------------------------------
// 确定性摘要与 schema 观察工具（tests 专用；同种子两次生成的全表有序摘要必须一致）
// ---------------------------------------------------------------------------

/// 打开库并按行序计算各表内容摘要（SHA-256 hex）。表按名字排序（BTreeMap），
/// 行按 rowid（即生成序），值按列序原样并入哈希。
///
/// `created_at` / `updated_at` 审计时间列不并入摘要：迁移种子行（V004 默认
/// 分类与黑洞账户）的审计列取 `strftime('now')` 墙钟值，属产品种子行为、
/// 不归生成器管；生成器自身产出的全部业务字段（id/金额/kind/日期/引用/软删
/// 标志等）逐列参与比对——同种子两次生成摘要相等当且仅当这些字段全等。
fn digest_db(path: &PathBuf) -> Result<BTreeMap<String, String>, String> {
    use rusqlite::types::ValueRef;
    use sha2::Digest;

    const AUDIT_COLS: [&str; 2] = ["created_at", "updated_at"];
    let conn = open_connection(path).map_err(|e| e.to_string())?;
    // 全部生成器写入面：核心交易域 7 表（#459）+ 投资域 6 表与预算/计划域 6 表
    // （issue #460）。新增表必须进摘要清单——确定性验收「含新增表」的落点。
    const TABLES: [&str; 19] = [
        "accounts",
        "categories",
        "merchants",
        "transactions",
        "exchange_rates",
        "fx_rate_history",
        "currencies",
        // 投资域（issue #460）。
        "instruments",
        "security_transactions",
        "security_lots",
        "security_lot_sales",
        "market_prices",
        "price_history",
        // 预算与定时计划域（issue #460）。
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

/// 读 user_version（与产品迁移路径产出的内存库比对）。
fn user_version(conn: &Connection) -> Result<i64, String> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())
}

/// 小规模画像测试的生成笔数（确定性：分布断言的容差按此规模校准）。
const PROFILE_N: u64 = 4000;

fn temp_db(tag: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("ledger-perf-test-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    (dir.clone(), dir.join(format!("{tag}.db")))
}

/// 经完整路径生成小库：标准连接工厂打开 → 迁移 → 生成。
fn build(path: &PathBuf, transactions: u64, end_date: NaiveDate) -> GenCounts {
    let mut conn = open_connection(path).unwrap();
    init_db(&mut conn).unwrap();
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

// ---------------------------------------------------------------------------
// schema 保真：迁移路径一致 + 外键完整
// ---------------------------------------------------------------------------

#[test]
fn generated_schema_matches_product_migration_path() {
    let (_dir, path) = temp_db("schema");
    build(&path, 300, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());

    let conn = open_connection(&path).unwrap();
    let mut mem = open_in_memory().unwrap();
    init_db(&mut mem).unwrap();

    // user_version 与产品迁移路径产出的内存库一致（建库不复制 DDL 的直接证据）。
    let file_version = user_version(&conn).unwrap();
    assert_eq!(file_version, user_version(&mem).unwrap());
    assert!(
        file_version > 0,
        "迁移应已应用：user_version = {file_version}"
    );

    // PRAGMA foreign_key_check 无违例。
    let mut stmt = conn.prepare("PRAGMA foreign_key_check").unwrap();
    let mut rows = stmt.query([]).unwrap();
    let violation = rows.next().unwrap();
    assert!(violation.is_none(), "外键完整性违例：{:?}", violation);
}

// ---------------------------------------------------------------------------
// 画像分布（4000 笔小样本，容差按样本规模校准）
// ---------------------------------------------------------------------------

#[test]
fn profile_matches_spec_counts() {
    let (_dir, path) = temp_db("profile-counts");
    let counts = build(
        &path,
        PROFILE_N,
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
    );
    let conn = open_connection(&path).unwrap();

    // 基线 = 产品迁移路径产出的内存库（含默认种子，种子数不硬编码）：
    // 文件库各参考数据量 = 基线 + 生成量。
    let mut mem = open_in_memory().unwrap();
    init_db(&mut mem).unwrap();

    // 账户：生成 50 个（与 list_accounts 同口径，不含隐藏黑洞种子）。
    let mem_accounts = accounts::list_accounts(&mem).unwrap().len();
    assert_eq!(
        accounts::list_accounts(&conn).unwrap().len(),
        mem_accounts + 50
    );

    // 分类：生成 40 个（名字带「基准」前缀），总数 = 种子基线 + 40。
    let mem_categories = categories::list_categories(&mem, false).unwrap().len();
    let cats = categories::list_categories(&conn, false).unwrap();
    assert_eq!(cats.len(), mem_categories + 40);
    assert_eq!(
        cats.iter().filter(|c| c.name.starts_with("基准")).count(),
        40
    );

    // 商户 800 个（迁移不种子商户）。
    assert_eq!(merchants::list_merchants(&conn, false).unwrap().len(), 800);

    // 币种字典全部来自迁移种子（生成器不新增币种）。
    let mem_currencies = currencies::list_currencies(&mem).unwrap().len();
    assert_eq!(
        currencies::list_currencies(&conn).unwrap().len(),
        mem_currencies
    );

    // 交易总数 = 参数笔数（列表 total 为未删除口径，软删量由 GenCounts 补回）。
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
                kind: Some(k),
                ..Default::default()
            },
        )
        .unwrap()
        .total as f64
    };
    let n = PROFILE_N as f64;
    // 转账约 8%、退款约 2%、软删除约 1%（容差 ±3σ 量级，确定性下数值恒定）。
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

    // 挂商户流水（未删除）按商户计数，top 20 商户应占约 60%。
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
    // 长尾形态：最尾部商户单户占比应远小于账户均值（薄尾）。
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

    // 4 个外币账户（2 USD + 1 EUR + 1 HKD 投资户）/ 50 ≈ 8% 流水（涉及账户口径，
    // 含转账转入侧；issue #460 加入 HKD 投资户后外币账户多一个）。
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

    // 外币交易 native 列按落库汇率折算（native ≠ raw），CNY 交易 1:1。
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

    // 当前汇率三行（USD/EUR/HKD → CNY）与全历史周采样（≈261 周 × 3 币对）。
    // 这两项无既有读 API（当前汇率读经 Writer 折算内部、历史经走势聚合），
    // 以原生 SQL 断言画像行数。
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

// ---------------------------------------------------------------------------
// 退款链与转账形态（经查询函数层）
// ---------------------------------------------------------------------------

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
            kind: Some(TransactionKind::Refund),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(refunds.total > 0, "应有退款链");
    let mut resolved = 0usize;
    for r in refunds.items.iter().take(50) {
        let orig_id = r.refund_of_transaction_id.clone().unwrap();
        // 原支出被软删时 get_transaction（未删除口径）NotFound：产品删除不级联退款
        // （refund_of 仅硬删 SET NULL），退款链照常指向，属合法数据形态，跳过即可。
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
            kind: Some(TransactionKind::Transfer),
            limit: Some(50),
            ..Default::default()
        },
    )
    .unwrap();
    for t in &transfers.items {
        let to = t.to_account_id.as_ref().expect("转账必须有转入账户");
        assert_ne!(&t.account_id, to, "转账两端不同账户");
        assert!(t.category_id.is_none(), "转账不挂分类");
        assert!(t.merchant_id.is_none(), "转账不挂商户");
    }
}

// ---------------------------------------------------------------------------
// 确定性：同种子同摘要、异种子异数据
// ---------------------------------------------------------------------------

#[test]
fn same_seed_produces_identical_digest() {
    let (_dir_a, path_a) = temp_db("det-a");
    let (_dir_b, path_b) = temp_db("det-b");
    let (_dir_c, path_c) = temp_db("det-c");
    let end = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
    build(&path_a, 1500, end);
    build(&path_b, 1500, end);
    // 不同种子：同规模对照。
    let mut conn = open_connection(&path_c).unwrap();
    init_db(&mut conn).unwrap();
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
    // 种子只影响生成内容，不影响迁移种子行（currencies 两库一致）。
    assert_eq!(digest_a["currencies"], digest_c["currencies"]);
}

// ---------------------------------------------------------------------------
// 参数解析与参数生效
// ---------------------------------------------------------------------------

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

    assert!(parse_args(&["--nope".to_string()]).is_err(), "未知参数报错");
    assert!(parse_args(&["--seed".to_string()]).is_err(), "缺值报错");
    assert!(
        parse_args(&["--seed".to_string(), "abc".to_string()]).is_err(),
        "非整数报错"
    );
    assert_eq!(
        parse_args(&["--help".to_string()]),
        Ok(ParsedArgs::Help),
        "--help 请求"
    );
}

#[test]
fn params_take_effect_on_output() {
    // --transactions / --end-date / --out 全部生效。
    let (dir, path) = temp_db("params");
    let end = NaiveDate::from_ymd_opt(2023, 9, 30).unwrap();
    let counts = build(&path, 777, end);

    assert!(path.exists(), "--out 指定路径应有库文件");
    let conn = open_connection(&path).unwrap();
    // 列表 total 为未删除口径：active + 软删 = 参数笔数。
    let active = list_transactions(&conn, &TransactionListFilter::default())
        .unwrap()
        .total;
    assert_eq!(
        (active as u64) + counts.deleted_transactions as u64,
        777,
        "--transactions 应决定生成笔数"
    );
    // 全部数据落在锚定窗口内：窗口外区间为零。
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
    // 重复生成（默认 --out 撞文件场景）：经真实入口 run() 走先删后建，
    // 从空库迁移重建，画像不被二次灌入。
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
    // 全表行数（含软删）恒等于参数笔数。
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 500, "重复生成不叠加交易");
    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// 投资域画像（issue #460：标的/价格线/标的交易/持仓视图）
// ---------------------------------------------------------------------------

/// 投资域画像测试的生成笔数：标的交易份额 0.6%，2 万笔 ≈ 120 笔标的交易，
/// 足以覆盖部分卖出/清仓形态且仍在单测耗时预算内。
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

    // 标的字典 20 行；来源随产品通道（同步 eastmoney / 场外基金 manual）。
    let instruments = investment::list_instruments(&conn, &InstrumentListFilter::default())
        .unwrap()
        .items;
    assert_eq!(instruments.len(), 20);
    assert_eq!(
        instruments
            .iter()
            .filter(|i| i.source == "manual" && i.kind == investment::InstrumentType::Fund)
            .count(),
        3,
        "场外基金标的手动来源"
    );

    // 标的交易份额 ≈ 0.6%（买 0.4% + 卖 0.2%；默认 50 万笔下即约 3000 笔）。
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

    // 标的交易全部落在投资账户（同币种纪律的账户面），且不软删。
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

    // 持仓视图非空、市值全部可折算（有现价 + 汇率可达 → 不为 NULL）。
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
    // 同币种持仓的市值口径：quantity × 现价 ÷ 100（价格刻度万分之一元）。
    let h = &holdings[0];
    let price = h.latest_price_cents.unwrap();
    let expected = (h.quantity * price as f64 / 100.0).round() as i64;
    assert_eq!(
        h.market_value_cents.unwrap(),
        expected,
        "同币种市值 = 数量×现价÷100"
    );

    // 批次账本闭合：每批次 初始 − 剩余 == Σ 卖出匹配量；耗尽批次剩余恰为 0。
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

    // 卖出形态覆盖：部分卖出（0 < 剩余 < 初始）与清仓（剩余 = 0）都存在。
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

    // 因果序：卖出不早于其匹配批次的买入日（产品不可产出先卖后买的数据）。
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

    // 基金买入份额按行情推导：反算单价应贴合该日价格线（±1%），不独立抽样。
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

    // 现价缓存每标的恰一行；价格线全窗口周采样。
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
    // 周唯一：每标的行数 == 标的 × 周数不重复（UNIQUE 约束保证，这里验证行数守恒）。
    let history_total: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(history_total, per_instrument * 20);

    // 现价 = 最新历史点映像（MarketPrice 语义），行情日期同步到末次采样。
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

    // 场外基金现价带净值日期；股票/ETF 恒 NULL；来源为同步通道。
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

    // 港股标的以 HKD 计价，且 HKD→CNY 汇率可达（港股市值可折算的前提）。
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

    // 交易行金额与扩展表的数量/单价/费用按产品公式闭合（issue #302 口径）：
    // 非基金 buy：金额 = 数量×单价÷100 + 费；sell：金额 = 数量×单价÷100 − 费；
    // 基金以金额权威：buy 单价 = (金额−费)×100÷数量，sell 单价 = (金额+费)×100÷数量。
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

// ---------------------------------------------------------------------------
// 预算与定时计划画像（issue #460）
// ---------------------------------------------------------------------------

#[test]
fn profile_budgets_and_scheduled_plans() {
    let (_dir, path) = temp_db("profile-plans");
    let counts = build(&path, 2_000, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    let conn = open_connection(&path).unwrap();

    // 预算：6 条 = 月度 4 + 年度 2，全部挂支出分类，「分类 + 周期」不重复。
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

    // 计划：8 个 = 分期 3 / 订阅 3 / 定时转账 2，含 1 个 paused。
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
    // 三种形态各有扩展表行，且与计划一一对应。
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

    // 期次状态结构恒定（日期由锚定结束日推导，与种子无关）：
    // completed 37 / pending 21 / failed 2 / cancelled 1；仅 completed 关联交易。
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

    // 期次交易的形态：分期/订阅 → expense（挂计划分类），定时转账 → transfer；
    // 金额恒等于计划金额（每期固定），交易存在且未删除。
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

    // 期次交易从 --transactions 预算预留：总数 = 参数笔数（规模 ≥ 期次交易数时）。
    assert_eq!(
        counts.transactions as u64, 2_000,
        "交易总数应等于 --transactions（含预留期次交易）"
    );
}
