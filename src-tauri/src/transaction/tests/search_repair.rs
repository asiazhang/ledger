//! 拼音辅助数据一键修复测试（issue #513）：显式回填命令的领域函数——积压全量
//! 回填、幂等（重复执行不重复计数）、不破坏已回填行、收敛判定与修复后拼音
//! 搜索不漏。搜索入口惰性回填的既有行为由 [`super::search`] 守护，不在此重复。

use rusqlite::Connection;

use super::super::search::{repair_note_pinyin, search_transactions_internal};
use crate::db::{init_db, open_in_memory};
use crate::error::Result;
use crate::transaction::{NotePinyinRepairStage, TransactionSearchResult};

fn setup() -> Connection {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    conn
}

fn search(conn: &Connection, query: &str) -> Result<TransactionSearchResult> {
    search_transactions_internal(conn, query, 1, 20, None, None, None, None)
}

/// V018 之前的存量行形态：note 有值、拼音列可手工指定（None = NULL 积压行）。
fn insert_txn_note_pinyin(
    conn: &Connection,
    id: &str,
    account_id: &str,
    note: Option<&str>,
    date: &str,
    note_pinyin: Option<&str>,
) {
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,note_pinyin,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'expense',1000,'CNY',1000,?2,NULL,NULL,NULL,?3,?4,?5,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        rusqlite::params![id, account_id, note, note_pinyin, date],
    )
    .unwrap();
}

fn note_pinyin_of(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT note_pinyin FROM transactions WHERE id=?1",
        rusqlite::params![id],
        |r| r.get(0),
    )
    .unwrap()
}

fn backlog_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE note_pinyin IS NULL AND note IS NOT NULL",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        rusqlite::params![id, name, kind, currency],
    )
    .unwrap();
}

/// 积压全量回填：报告回填行数、判定收敛，列值与现算规则一致，拼音搜索不漏。
#[test]
fn repair_backfills_all_backlog_and_converges() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn_note_pinyin(&conn, "t1", "a1", Some("万科物业"), "2026-02-01", None);
    insert_txn_note_pinyin(&conn, "t2", "a1", Some("招商银行转账"), "2026-02-02", None);
    insert_txn_note_pinyin(&conn, "t3", "a1", Some("买咖啡"), "2026-02-03", None);
    assert_eq!(backlog_count(&conn), 3);

    let report = repair_note_pinyin(&conn);
    assert_eq!(report.backfilled, 3);
    assert!(report.converged);
    assert!(report.failure.is_none());

    // 列值与现算规则一致（多音字前字规则同 Writer 接缝）。
    assert_eq!(note_pinyin_of(&conn, "t1").as_deref(), Some("wkwy"));
    assert_eq!(note_pinyin_of(&conn, "t2").as_deref(), Some("zsyhzz"));
    assert_eq!(note_pinyin_of(&conn, "t3").as_deref(), Some("mkf"));

    // 修复后拼音搜索不漏（读路径消费已回填列）。
    for (term, id) in [("wy", "t1"), ("zsyh", "t2"), ("mkf", "t3")] {
        let res = search(&conn, term).unwrap();
        assert_eq!(res.total, 1, "词条 {term} 应命中");
        assert_eq!(res.items[0].id, id);
    }
}

/// 幂等：重复执行不重复计数、不破坏已回填行，且仍收敛。
#[test]
fn repair_is_idempotent() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn_note_pinyin(&conn, "t1", "a1", Some("万科物业"), "2026-02-01", None);
    let first = repair_note_pinyin(&conn);
    assert_eq!(first.backfilled, 1);
    assert!(first.converged);

    let second = repair_note_pinyin(&conn);
    assert_eq!(second.backfilled, 0, "重复执行不得重复计数");
    assert!(second.converged);
    assert!(second.failure.is_none());
    assert_eq!(note_pinyin_of(&conn, "t1").as_deref(), Some("wkwy"));
}

/// 无积压（新库/已收敛库）一键修复：零回填、收敛、无失败。
#[test]
fn repair_on_converged_db_is_noop() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    let report = repair_note_pinyin(&conn);
    assert_eq!(report.backfilled, 0);
    assert!(report.converged);
    assert!(report.failure.is_none());

    // Writer 正常写入的行不构成积压，也不被改写。
    insert_txn_note_pinyin(&conn, "t1", "a1", Some("买咖啡"), "2026-02-03", Some("mkf"));
    let report = repair_note_pinyin(&conn);
    assert_eq!(report.backfilled, 0);
    assert!(report.converged);
    assert_eq!(note_pinyin_of(&conn, "t1").as_deref(), Some("mkf"));
}

/// 不破坏已回填行：只补 NULL 积压，已有列值（含手工脏值）原样保留、不计入回填数。
#[test]
fn repair_preserves_existing_filled_rows() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn_note_pinyin(
        &conn,
        "t1",
        "a1",
        Some("万科物业"),
        "2026-02-01",
        Some("wkwy"),
    );
    // 手工脏值（与现算规则不一致）：修复不触碰（派生列允许漂移，审计不在此）。
    insert_txn_note_pinyin(
        &conn,
        "t2",
        "a1",
        Some("招商银行转账"),
        "2026-02-02",
        Some("dirty"),
    );
    insert_txn_note_pinyin(&conn, "t3", "a1", Some("买咖啡"), "2026-02-03", None);

    let report = repair_note_pinyin(&conn);
    assert_eq!(report.backfilled, 1, "只计 NULL 积压行");
    assert!(report.converged);
    assert_eq!(note_pinyin_of(&conn, "t1").as_deref(), Some("wkwy"));
    assert_eq!(note_pinyin_of(&conn, "t2").as_deref(), Some("dirty"));
    assert_eq!(note_pinyin_of(&conn, "t3").as_deref(), Some("mkf"));
}

/// 无备注行不构成积压：note_pinyin 恒 NULL 但不计积压、不影响收敛判定。
#[test]
fn repair_ignores_noteless_rows() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn_note_pinyin(&conn, "t1", "a1", Some("万科物业"), "2026-02-01", None);
    insert_txn_note_pinyin(&conn, "t2", "a1", None, "2026-02-02", None);

    let report = repair_note_pinyin(&conn);
    assert_eq!(report.backfilled, 1);
    assert!(report.converged, "无备注行的 NULL 列不构成积压");
    assert_eq!(note_pinyin_of(&conn, "t2"), None);
}

/// 失败注入（验收：失败时报告原因，不静默）：探测阶段失败——表缺失使探针
/// 查询出错，报告携带 Probe 阶段与底层消息、零回填、收敛保守置否。
#[test]
fn repair_reports_probe_failure() {
    let conn = setup();
    conn.execute("DROP TABLE transactions", []).unwrap();

    let report = repair_note_pinyin(&conn);
    let failure = report.failure.expect("探测失败应报告原因");
    assert!(matches!(failure.stage, NotePinyinRepairStage::Probe));
    assert!(!failure.message.is_empty());
    assert_eq!(report.backfilled, 0);
    assert!(!report.converged);
}

/// 失败注入（验收：失败时报告原因，不静默）：写入阶段失败——触发器对
/// UPDATE RAISE(ABORT)，报告携带 Write 阶段与底层消息，剩余积压如实报告
/// （收敛否），且事务回滚后已回填行不损。
#[test]
fn repair_reports_write_failure_and_stays_honest() {
    let conn = setup();
    insert_account(&conn, "a1", "现金", "cash", "CNY");
    insert_txn_note_pinyin(&conn, "t1", "a1", Some("万科物业"), "2026-02-01", None);
    conn.execute(
        "CREATE TRIGGER injected_abort BEFORE UPDATE ON transactions \
         BEGIN SELECT RAISE(ABORT, 'injected write failure'); END",
        [],
    )
    .unwrap();

    let report = repair_note_pinyin(&conn);
    let failure = report.failure.expect("写入失败应报告原因");
    assert!(matches!(failure.stage, NotePinyinRepairStage::Write));
    assert!(failure.message.contains("injected write failure"));
    assert_eq!(report.backfilled, 0, "失败批不计回填数");
    assert!(!report.converged, "剩余积压如实报告");
    assert_eq!(note_pinyin_of(&conn, "t1"), None, "失败批已回滚");
}
