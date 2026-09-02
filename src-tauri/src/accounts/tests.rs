use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, ErrClass};
use crate::models::Account;
use crate::transaction::amount::{Measure, TransactionKind, TransferSide, signed_amount};

fn setup() -> rusqlite::Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn list_accounts(conn: &rusqlite::Connection) -> Vec<Account> {
    query_all(
        conn,
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden \
         FROM accounts WHERE is_deleted=0 AND is_hidden=0 ORDER BY created_at",
        [],
    )
    .unwrap()
}

fn insert_account(
    conn: &rusqlite::Connection,
    id: &str,
    name: &str,
    kind: &str,
    currency: &str,
    initial: i64,
) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0,0)",
        rusqlite::params![id, name, kind, currency, initial, now, now, 1, device_id()],
    ).unwrap();
}

fn insert_hidden_account(conn: &rusqlite::Connection, id: &str, name: &str, currency: &str) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden) \
         VALUES (?1,?2,'other',?3,0,?4,?5,?6,?7,0,1)",
        rusqlite::params![id, name, currency, now, now, 1, device_id()],
    ).unwrap();
}

fn insert_tx(
    conn: &rusqlite::Connection,
    id: &str,
    kind: &str,
    amount: i64,
    account_id: &str,
    to_account_id: Option<&str>,
) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'CNY',?3,?4,?5,NULL,NULL,NULL,'2026-01-15',?6,?7,?8,?9,0)",
        rusqlite::params![id, kind, amount, account_id, to_account_id, now, now, 1, device_id()],
    ).unwrap();
}

fn balance(conn: &rusqlite::Connection, account_id: &str) -> i64 {
    crate::db::balance::compute_balance(conn, account_id).unwrap()
}

#[test]
fn list_accounts_empty_initially() {
    let conn = setup();
    let accounts = list_accounts(&conn);
    assert!(accounts.is_empty());
}

#[test]
fn create_account_and_list() {
    let conn = setup();
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'bank','CNY',0,?3,?4,?5,?6,0)",
        rusqlite::params![id, "测试账户", now, now, 1, device_id()],
    ).unwrap();
    let accounts = list_accounts(&conn);
    assert!(accounts.iter().any(|a| a.id == id && a.name == "测试账户"));
}

#[test]
fn delete_account_soft_deletes() {
    let conn = setup();
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'cash','CNY',0,?3,?4,?5,?6,0)",
        rusqlite::params![id, "待删除", now, now, 1, device_id()],
    ).unwrap();
    assert!(list_accounts(&conn).iter().any(|a| a.id == id));
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    ).unwrap();
    assert!(!list_accounts(&conn).iter().any(|a| a.id == id));
}

#[test]
fn delete_account_soft_deletes_and_excludes_from_readback() {
    let conn = setup();
    insert_account(&conn, "acc-del-1", "现金", "cash", "CNY", 0);
    super::delete_account(&conn, "acc-del-1").unwrap();
    assert!(
        !list_accounts(&conn).iter().any(|a| a.id == "acc-del-1"),
        "删除后不应出现在读回结果中"
    );
}

#[test]
fn delete_account_returns_not_found_for_missing_id() {
    let conn = setup();
    let err = super::delete_account(&conn, "不存在的id").unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
    assert!(err.to_string().contains("账户不存在"));
}

#[test]
fn delete_account_returns_not_found_for_already_deleted() {
    let conn = setup();
    insert_account(&conn, "acc-del-2", "现金", "cash", "CNY", 0);
    super::delete_account(&conn, "acc-del-2").unwrap();
    let err = super::delete_account(&conn, "acc-del-2").unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Coded {
                class: ErrClass::NotFound,
                ..
            }
        ),
        "已删除账户应再次返回 404"
    );
}

#[test]
fn balance_starts_at_initial() {
    let conn = setup();
    insert_account(&conn, "acc-bal-1", "现金", "cash", "CNY", 10000);
    assert_eq!(balance(&conn, "acc-bal-1"), 10000);
}

#[test]
fn balance_adds_income() {
    let conn = setup();
    insert_account(&conn, "acc-bal-2", "现金", "cash", "CNY", 0);
    insert_tx(&conn, "tx1", "income", 5000, "acc-bal-2", None);
    assert_eq!(balance(&conn, "acc-bal-2"), 5000);
}

#[test]
fn balance_subtracts_expense() {
    let conn = setup();
    insert_account(&conn, "acc-bal-3", "现金", "cash", "CNY", 10000);
    insert_tx(&conn, "tx2", "expense", 3000, "acc-bal-3", None);
    assert_eq!(balance(&conn, "acc-bal-3"), 7000);
}

#[test]
fn balance_adds_transfer_in() {
    let conn = setup();
    insert_account(&conn, "acc-a", "账户A", "cash", "CNY", 0);
    insert_account(&conn, "acc-b", "账户B", "cash", "CNY", 0);
    insert_tx(&conn, "tx3", "transfer", 2000, "acc-a", Some("acc-b"));
    assert_eq!(balance(&conn, "acc-a"), -2000);
    assert_eq!(balance(&conn, "acc-b"), 2000);
}

#[test]
fn balance_adds_refund() {
    let conn = setup();
    insert_account(&conn, "acc-bal-4", "现金", "cash", "CNY", 0);
    insert_tx(&conn, "tx4", "expense", 1000, "acc-bal-4", None);
    insert_tx(&conn, "tx5", "refund", 300, "acc-bal-4", None);
    assert_eq!(balance(&conn, "acc-bal-4"), -700);
}

#[test]
fn soft_deleted_transaction_excluded_from_balance() {
    let conn = setup();
    insert_account(&conn, "acc-bal-5", "现金", "cash", "CNY", 0);
    insert_tx(&conn, "tx6", "income", 5000, "acc-bal-5", None);
    assert_eq!(balance(&conn, "acc-bal-5"), 5000);
    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params!["tx6", now_iso(), device_id()],
    ).unwrap();
    assert_eq!(balance(&conn, "acc-bal-5"), 0);
}

#[test]
fn list_account_balances_returns_all_accounts() {
    let conn = setup();
    insert_account(&conn, "acc-list-1", "现金", "cash", "CNY", 10000);
    insert_account(&conn, "acc-list-2", "储蓄卡", "bank", "CNY", 50000);
    insert_tx(&conn, "tx7", "income", 3000, "acc-list-1", None);
    insert_tx(&conn, "tx8", "expense", 2000, "acc-list-2", None);
    let accounts = list_accounts(&conn);
    assert_eq!(accounts.len(), 2);
    assert_eq!(balance(&conn, "acc-list-1"), 13000);
    assert_eq!(balance(&conn, "acc-list-2"), 48000);
}

// 余额口径测试夹具：一行 = 一笔交易（kind 用 Amount 接缝的 TransactionKind 枚举表述）。
struct FlowRow {
    id: &'static str,
    kind: TransactionKind,
    amount: i64,
    account_id: &'static str,
    to_account_id: Option<&'static str>,
}

/// 期望余额由度量矩阵（`signed_amount` × `AccountFlow`）在 Rust 侧对夹具逐行求和得出，
/// 断言 `compute_balance` 与 `compute_all_balances` 均与之一致——测试不复制生产 SQL。
/// 覆盖全部 8 种 kind（含 buy/sell/dividend/split 与 transfer 双侧）。
#[test]
fn balance_computed_via_account_flow_measure() {
    let conn = setup();
    insert_account(&conn, "acc-flow-m", "现金", "cash", "CNY", 1000);
    insert_account(&conn, "acc-flow-n", "证券", "investment", "CNY", 0);
    let initial = |account: &str| -> i64 {
        match account {
            "acc-flow-m" => 1000,
            _ => 0,
        }
    };

    let rows = vec![
        FlowRow {
            id: "fm1",
            kind: TransactionKind::Income,
            amount: 5000,
            account_id: "acc-flow-m",
            to_account_id: None,
        },
        FlowRow {
            id: "fm2",
            kind: TransactionKind::Expense,
            amount: 1200,
            account_id: "acc-flow-m",
            to_account_id: None,
        },
        FlowRow {
            id: "fm3",
            kind: TransactionKind::Refund,
            amount: 300,
            account_id: "acc-flow-m",
            to_account_id: None,
        },
        FlowRow {
            id: "fm4",
            kind: TransactionKind::Transfer,
            amount: 800,
            account_id: "acc-flow-m",
            to_account_id: Some("acc-flow-n"),
        },
        FlowRow {
            id: "fm5",
            kind: TransactionKind::Buy,
            amount: 2000,
            account_id: "acc-flow-n",
            to_account_id: None,
        },
        FlowRow {
            id: "fm6",
            kind: TransactionKind::Sell,
            amount: 1500,
            account_id: "acc-flow-n",
            to_account_id: None,
        },
        FlowRow {
            id: "fm7",
            kind: TransactionKind::Dividend,
            amount: 60,
            account_id: "acc-flow-n",
            to_account_id: None,
        },
        FlowRow {
            id: "fm8",
            kind: TransactionKind::Split,
            amount: 9999,
            account_id: "acc-flow-n",
            to_account_id: None,
        },
    ];
    for r in &rows {
        insert_tx(
            &conn,
            r.id,
            r.kind.as_str(),
            r.amount,
            r.account_id,
            r.to_account_id,
        );
    }

    let expected = |account: &str| -> i64 {
        initial(account)
            + rows
                .iter()
                .filter(|r| r.account_id == account || r.to_account_id == Some(account))
                .map(|r| {
                    let side = if r.account_id == account {
                        TransferSide::Out
                    } else {
                        TransferSide::In
                    };
                    signed_amount(r.kind, r.amount, Measure::AccountFlow(side))
                })
                .sum::<i64>()
    };

    // acc-flow-m = 1000 +5000 −1200 +300 −800 = 4300
    assert_eq!(balance(&conn, "acc-flow-m"), 4300);
    // acc-flow-n = +800 −2000 +1500 +60 +0 = 360
    assert_eq!(balance(&conn, "acc-flow-n"), 360);

    let all = crate::db::balance::compute_all_balances(&conn).unwrap();
    for id in ["acc-flow-m", "acc-flow-n"] {
        assert_eq!(
            *all.get(id).unwrap_or(&0),
            expected(id),
            "批量余额与度量不一致: {id}"
        );
        assert_eq!(
            balance(&conn, id),
            expected(id),
            "单个余额与度量不一致: {id}"
        );
    }
}

#[test]
fn compute_all_balances_matches_per_account() {
    let conn = setup();
    insert_account(&conn, "acc-bulk-1", "现金", "cash", "CNY", 10000);
    insert_account(&conn, "acc-bulk-2", "储蓄卡", "bank", "CNY", 50000);
    insert_account(&conn, "acc-bulk-3", "信用卡", "credit", "CNY", 0);
    insert_tx(&conn, "tx-b1", "income", 5000, "acc-bulk-1", None);
    insert_tx(&conn, "tx-b2", "expense", 2000, "acc-bulk-1", None);
    insert_tx(&conn, "tx-b3", "expense", 1500, "acc-bulk-2", None);
    insert_tx(
        &conn,
        "tx-b4",
        "transfer",
        3000,
        "acc-bulk-1",
        Some("acc-bulk-2"),
    );
    insert_tx(&conn, "tx-b5", "refund", 500, "acc-bulk-1", None);
    insert_tx(&conn, "tx-b6", "dividend", 60, "acc-bulk-3", None);

    let all = crate::db::balance::compute_all_balances(&conn).unwrap();

    for id in ["acc-bulk-1", "acc-bulk-2", "acc-bulk-3"] {
        let expected = balance(&conn, id);
        let got = *all.get(id).unwrap_or(&0);
        assert_eq!(
            got, expected,
            "余额不一致: {id}, 期望 {expected}, 得到 {got}"
        );
    }
}

#[test]
fn compute_all_balances_excludes_soft_deleted_accounts() {
    let conn = setup();
    insert_account(&conn, "acc-active", "活动账户", "cash", "CNY", 1000);
    insert_account(&conn, "acc-deleted", "已删除", "cash", "CNY", 2000);
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params!["acc-deleted", now_iso(), device_id()],
    ).unwrap();

    let all = crate::db::balance::compute_all_balances(&conn).unwrap();
    assert!(all.contains_key("acc-active"), "应包含活动账户");
    assert!(!all.contains_key("acc-deleted"), "不应包含已删除账户");
}

#[test]
fn list_accounts_excludes_hidden_accounts() {
    let conn = setup();
    insert_account(&conn, "acc-normal", "现金", "cash", "CNY", 0);
    insert_hidden_account(&conn, "acc-hidden", "无(CNY)", "CNY");

    let accounts = list_accounts(&conn);
    assert!(
        accounts.iter().any(|a| a.id == "acc-normal"),
        "应包含普通账户"
    );
    assert!(
        !accounts.iter().any(|a| a.id == "acc-hidden"),
        "不应包含黑洞账户"
    );
}

#[test]
fn list_accounts_for_api_includes_hidden_with_flag() {
    let conn = setup();
    insert_account(&conn, "acc-normal", "现金", "cash", "CNY", 0);
    insert_hidden_account(&conn, "acc-hidden", "无(CNY)", "CNY");

    let accounts = super::list_accounts_for_api(&conn).unwrap();
    let hidden = accounts.iter().find(|a| a.id == "acc-hidden").unwrap();
    assert!(hidden.is_hidden, "API 应返回 is_hidden=true 的黑洞账户");
    let normal = accounts.iter().find(|a| a.id == "acc-normal").unwrap();
    assert!(!normal.is_hidden);
}

#[test]
fn hidden_account_transaction_visible_in_transaction_list() {
    let conn = setup();
    insert_hidden_account(&conn, "acc-hidden", "无(CNY)", "CNY");
    insert_tx(&conn, "tx-hidden", "expense", 3000, "acc-hidden", None);

    let rows = crate::transaction::list_transactions_internal(
        &conn,
        &crate::models::TransactionListFilter::default(),
    )
    .unwrap();
    assert!(
        rows.items
            .iter()
            .any(|t| t.id == "tx-hidden" && t.account_id == "acc-hidden"),
        "黑洞账户的交易应仍在交易列表中"
    );
}

#[test]
fn hidden_account_balance_excluded_from_all_balances() {
    let conn = setup();
    insert_hidden_account(&conn, "acc-hidden", "无(CNY)", "CNY");
    insert_tx(&conn, "tx-h", "income", 5000, "acc-hidden", None);

    let all = crate::db::balance::compute_all_balances(&conn).unwrap();
    assert!(
        !all.contains_key("acc-hidden"),
        "compute_all_balances 不应包含黑洞账户"
    );
}

#[test]
fn hidden_account_transactions_included_in_reports() {
    let conn = setup();
    insert_account(&conn, "acc-normal", "现金", "cash", "CNY", 0);
    insert_hidden_account(&conn, "acc-hidden", "无(CNY)", "CNY");
    insert_tx(&conn, "tx-normal", "income", 1000, "acc-normal", None);
    insert_tx(&conn, "tx-hidden", "expense", 2000, "acc-hidden", None);

    let summary = crate::reports::monthly_summary_rows(&conn, 2026)
        .unwrap()
        .remove(0);
    assert_eq!(summary.income_cents, 1000);
    assert_eq!(summary.expense_cents, 2000, "黑洞账户的支出应计入报表");
}

#[test]
fn seed_contains_black_hole_accounts_for_cny_and_hkd() {
    let conn = setup();
    let mut stmt = conn
        .prepare(
            "SELECT name, currency_code, is_hidden FROM accounts WHERE is_hidden=1 ORDER BY currency_code",
        )
        .unwrap();
    let rows: Vec<(String, String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(
        rows,
        vec![
            ("无(CNY)".to_string(), "CNY".to_string(), 1),
            ("无(HKD)".to_string(), "HKD".to_string(), 1),
        ],
        "种子应预置 CNY/HKD 两个黑洞账户"
    );
}

// ---------------------------------------------------------------------------
// update_account（编辑账户）
// ---------------------------------------------------------------------------

use crate::models::{AccountBalanceAdjustInput, AccountUpdateInput};

fn find_black_hole(conn: &rusqlite::Connection, currency: &str) -> Option<String> {
    conn.query_row(
        "SELECT id FROM accounts WHERE is_deleted=0 AND is_hidden=1 AND currency_code=?1 LIMIT 1",
        rusqlite::params![currency],
        |r| r.get(0),
    )
    .ok()
}

#[test]
fn update_account_renames_and_bumps_version() {
    let conn = setup();
    insert_account(&conn, "acc-u1", "旧名", "cash", "CNY", 0);
    super::update_account(
        &conn,
        "acc-u1",
        AccountUpdateInput {
            name: Some("新名".into()),
            currency_code: None,
        },
    )
    .unwrap();
    let account = super::get_account(&conn, "acc-u1").unwrap();
    assert_eq!(account.name, "新名");
    assert_eq!(account.currency_code, "CNY", "未传字段保持原值");
    assert_eq!(account.version, 2, "编辑应递增 version");
}

#[test]
fn update_account_rejects_empty_name() {
    let conn = setup();
    insert_account(&conn, "acc-u2", "现金", "cash", "CNY", 0);
    let err = super::update_account(
        &conn,
        "acc-u2",
        AccountUpdateInput {
            name: Some("   ".into()),
            currency_code: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    assert!(err.to_string().contains("名称不能为空"));
}

#[test]
fn update_account_rejects_currency_change_when_has_transactions() {
    let conn = setup();
    insert_account(&conn, "acc-u3", "现金", "cash", "CNY", 0);
    insert_tx(&conn, "tx-u3", "income", 1000, "acc-u3", None);
    let err = super::update_account(
        &conn,
        "acc-u3",
        AccountUpdateInput {
            name: None,
            currency_code: Some("HKD".into()),
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    assert!(err.to_string().contains("不能修改币种"));
    // 币种未被改动
    assert_eq!(
        super::get_account(&conn, "acc-u3").unwrap().currency_code,
        "CNY"
    );
}

#[test]
fn update_account_allows_currency_change_without_transactions() {
    let conn = setup();
    insert_account(&conn, "acc-u4", "现金", "cash", "CNY", 0);
    super::update_account(
        &conn,
        "acc-u4",
        AccountUpdateInput {
            name: None,
            currency_code: Some("HKD".into()),
        },
    )
    .unwrap();
    assert_eq!(
        super::get_account(&conn, "acc-u4").unwrap().currency_code,
        "HKD"
    );
}

#[test]
fn update_account_rejects_unknown_currency() {
    let conn = setup();
    insert_account(&conn, "acc-u5", "现金", "cash", "CNY", 0);
    let err = super::update_account(
        &conn,
        "acc-u5",
        AccountUpdateInput {
            name: None,
            currency_code: Some("XYZ".into()),
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    assert!(err.to_string().contains("未知币种"));
}

#[test]
fn update_account_returns_not_found_for_missing_id() {
    let conn = setup();
    let err = super::update_account(
        &conn,
        "不存在的id",
        AccountUpdateInput {
            name: Some("任意".into()),
            currency_code: None,
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
}

// ---------------------------------------------------------------------------
// adjust_account_balance（余额调整，ADR-0026 黑洞转账）
// ---------------------------------------------------------------------------

fn adjust(conn: &rusqlite::Connection, id: &str, target: i64) -> Result<(String, bool), AppError> {
    super::adjust_account_balance(
        conn,
        id,
        &AccountBalanceAdjustInput {
            target_balance_cents: target,
            date: "2026-09-15".into(),
            note: None,
        },
    )
}

/// 为非本位币测试补汇率（余额调整经 Writer 接缝折算本位币，缺汇率报错）。
fn ensure_rate(conn: &rusqlite::Connection, code: &str, rate: f64) {
    conn.execute(
        "INSERT OR IGNORE INTO currencies (code, name, symbol, decimal_places) VALUES (?1, ?1, '$', 2)",
        rusqlite::params![code],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO exchange_rates (id, base_code, quote_code, rate, priced_at, source, updated_at, version, device_id) \
         VALUES (?1, ?2, 'CNY', ?3, '2026-01-01T00:00:00Z', 'manual', ?4, 1, ?5)",
        rusqlite::params![new_uuid(), code, rate, now_iso(), device_id()],
    )
    .unwrap();
}

#[test]
fn adjust_up_creates_transfer_from_black_hole_and_reaches_target() {
    let conn = setup();
    insert_account(&conn, "acc-adj-1", "现金", "cash", "CNY", 0);
    insert_tx(&conn, "tx-adj-1", "income", 5000, "acc-adj-1", None);
    assert_eq!(balance(&conn, "acc-adj-1"), 5000);

    // Δ=+2000 → 从「无(CNY)」转入；种子黑洞已存在，不新建
    let (tx_id, created) = adjust(&conn, "acc-adj-1", 7000).unwrap();
    assert!(!created, "种子已预置 无(CNY)，不应新建黑洞账户");
    let black_hole = find_black_hole(&conn, "CNY").unwrap();
    let (kind, amount, account_id, to_account_id, note): (
        String,
        i64,
        String,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT kind, amount_cents, account_id, to_account_id, note FROM transactions WHERE id=?1",
            rusqlite::params![tx_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(kind, "transfer");
    assert_eq!(amount, 2000);
    assert_eq!(account_id, black_hole, "Δ>0 应从黑洞账户转出");
    assert_eq!(to_account_id.as_deref(), Some("acc-adj-1"));
    assert_eq!(note.as_deref(), Some("余额调整"), "缺省备注为「余额调整」");
    assert_eq!(balance(&conn, "acc-adj-1"), 7000, "调整后余额应等于目标值");
}

#[test]
fn adjust_down_creates_transfer_to_black_hole() {
    let conn = setup();
    insert_account(&conn, "acc-adj-2", "现金", "cash", "CNY", 5000);
    let (tx_id, created) = adjust(&conn, "acc-adj-2", 4000).unwrap();
    assert!(!created);
    let black_hole = find_black_hole(&conn, "CNY").unwrap();
    let (account_id, to_account_id, amount): (String, Option<String>, i64) = conn
        .query_row(
            "SELECT account_id, to_account_id, amount_cents FROM transactions WHERE id=?1",
            rusqlite::params![tx_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(amount, 1000, "Δ<0 转账金额为差额绝对值");
    assert_eq!(account_id, "acc-adj-2", "Δ<0 应从目标账户转出");
    assert_eq!(to_account_id.as_deref(), Some(black_hole.as_str()));
    assert_eq!(balance(&conn, "acc-adj-2"), 4000);
}

#[test]
fn adjust_creates_black_hole_for_missing_currency() {
    let conn = setup();
    ensure_rate(&conn, "USD", 1.0); // MVP 汇率 1:1
    insert_account(&conn, "acc-adj-3", "美元户", "cash", "USD", 0);
    let (_tx_id, created) = adjust(&conn, "acc-adj-3", 9900).unwrap();
    assert!(created, "缺失币种的黑洞账户应按需自动创建");
    let black_hole = find_black_hole(&conn, "USD").unwrap();
    let (name, kind, is_hidden): (String, String, i64) = conn
        .query_row(
            "SELECT name, type, is_hidden FROM accounts WHERE id=?1",
            rusqlite::params![black_hole],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(name, "无(USD)", "自动建的黑洞账户与种子同形");
    assert_eq!(kind, "other");
    assert_eq!(is_hidden, 1);
    assert_eq!(balance(&conn, "acc-adj-3"), 9900);
}

#[test]
fn adjust_reuses_existing_black_hole_for_same_currency() {
    let conn = setup();
    ensure_rate(&conn, "HKD", 1.0); // MVP 汇率 1:1
    insert_account(&conn, "acc-adj-4", "港币户", "cash", "HKD", 0);
    let _ = adjust(&conn, "acc-adj-4", 100).unwrap();
    let (_tx_id2, created2) = adjust(&conn, "acc-adj-4", 300).unwrap();
    assert!(!created2, "第二次调整应复用已有黑洞账户");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE is_hidden=1 AND currency_code='HKD'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "同币种黑洞账户不应重复创建");
    assert_eq!(balance(&conn, "acc-adj-4"), 300);
    // 两笔调整交易独立存在，均可删除撤销（普通 transfer，无特殊标记）
    let txs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE (account_id=(\
             SELECT id FROM accounts WHERE is_hidden=1 AND currency_code='HKD' LIMIT 1\
             ) OR to_account_id=(\
             SELECT id FROM accounts WHERE is_hidden=1 AND currency_code='HKD' LIMIT 1\
             )) AND is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(txs >= 2);
}

#[test]
fn adjust_zero_delta_errors_without_writing() {
    let conn = setup();
    insert_account(&conn, "acc-adj-5", "现金", "cash", "CNY", 12345);
    let err = adjust(&conn, "acc-adj-5", 12345).unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    assert!(err.to_string().contains("无需调整"));
    let tx_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tx_count, 0, "零差额不应产生任何写入");
}

#[test]
fn adjust_rejects_hidden_account() {
    let conn = setup();
    insert_hidden_account(&conn, "acc-adj-6", "无(CNY)", "CNY");
    let err = adjust(&conn, "acc-adj-6", 100).unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    assert!(err.to_string().contains("黑洞账户不支持余额调整"));
}

#[test]
fn adjust_returns_not_found_for_missing_account() {
    let conn = setup();
    let err = adjust(&conn, "不存在的id", 100).unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
}

#[test]
fn adjust_deleted_adjustment_tx_reverts_balance() {
    let conn = setup();
    insert_account(&conn, "acc-adj-7", "现金", "cash", "CNY", 0);
    let (tx_id, _) = adjust(&conn, "acc-adj-7", 5000).unwrap();
    assert_eq!(balance(&conn, "acc-adj-7"), 5000);
    // 调整产生的转账就是普通 transfer：删除即撤销调整
    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![tx_id, now_iso(), device_id()],
    )
    .unwrap();
    assert_eq!(balance(&conn, "acc-adj-7"), 0, "删除调整交易即恢复原余额");
}
