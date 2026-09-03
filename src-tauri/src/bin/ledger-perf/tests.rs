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
use tauri_app_lib::categories;
use tauri_app_lib::currencies;
use tauri_app_lib::db::{init_db, open_connection, open_in_memory};
use tauri_app_lib::merchants;
use tauri_app_lib::transaction::TransactionListFilter;
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::read::{get_transaction, list_transactions};

use super::generate::{GenCounts, GenerateParams, generate_into};
use super::{GenerateCli, parse_args};

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
    const TABLES: [&str; 7] = [
        "accounts",
        "categories",
        "merchants",
        "transactions",
        "exchange_rates",
        "fx_rate_history",
        "currencies",
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
    // 长尾形态：尾部商户基本有流水、但单户占比很小。
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

    // 3 个外币账户（2 USD + 1 EUR）/ 50 ≈ 6% 流水（涉及账户口径，含转账转入侧）。
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
    assert!((share - 0.06).abs() < 0.025, "外币流水占比偏离 6%：{share}");

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

    // 当前汇率两行（USD/EUR → CNY）与全历史周采样（≈261 周 × 2 币对）。
    // 这两项无既有读 API（当前汇率读经 Writer 折算内部、历史经走势聚合），
    // 以原生 SQL 断言画像行数。
    let rate_pairs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM exchange_rates WHERE quote_code='CNY'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rate_pairs, 2);
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
    let cli = parse_args(&[]).unwrap();
    assert_eq!(cli, GenerateCli::default());
    assert_eq!(cli.seed, 42);
    assert_eq!(cli.transactions, 500_000);
    assert_eq!(cli.end_date, "2025-12-31");
    assert!(
        cli.out
            .to_string_lossy()
            .ends_with("target/ledger-perf/ledger-perf.db")
    );

    let cli = parse_args(&[
        "--seed".to_string(),
        "7".to_string(),
        "--transactions=123".to_string(),
        "--end-date".to_string(),
        "2024-06-30".to_string(),
        "--out".to_string(),
        "/tmp/x.db".to_string(),
    ])
    .unwrap();
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
