//! 进程内搜索候选缓存测试（issue #493 / ADR-0027 修订记录预留手段的兑现）：
//! 快照口径（软删排除、列表序、派生列透传）、连接身份判别（异连接不串快照）
//! 与写后失效语义（失效后重建反映最新数据）。

use rusqlite::params;

use super::super::search_cache;
use crate::db::{init_db, open_in_memory};

fn setup() -> rusqlite::Connection {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    conn
}

/// 直插一个账户（候选行的 account_id 外键引用）。
fn insert_account(conn: &rusqlite::Connection, id: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'测试','cash','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id],
    )
    .unwrap();
}

/// 直插一笔交易（绕过 Writer：本模块只测缓存快照机制本身，不测写入接缝）。
fn insert_txn(
    conn: &rusqlite::Connection,
    id: &str,
    note: Option<&str>,
    note_pinyin: Option<&str>,
    date: &str,
    is_deleted: i64,
) {
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,note,note_pinyin,\
          date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'expense',100,'CNY',100,'a1',?2,?3,?4,\
                 '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',?5)",
        params![id, note, note_pinyin, date, is_deleted],
    )
    .unwrap();
}

#[test]
fn snapshot_excludes_deleted_and_keeps_list_order() {
    let conn = setup();
    insert_account(&conn, "a1");
    // 顺序故意乱插：快照必须按列表序（date DESC）给出；软删行不入快照。
    insert_txn(&conn, "t-old", Some("旧"), Some("j"), "2026-02-01", 0);
    insert_txn(&conn, "t-del", Some("删"), Some("sh"), "2026-02-02", 1);
    insert_txn(&conn, "t-new", Some("新"), Some("x"), "2026-02-03", 0);

    let ids = search_cache::with_shared_rows(&conn, |rows| {
        rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>()
    })
    .unwrap();
    assert_eq!(
        ids,
        vec!["t-new".to_string(), "t-old".to_string()],
        "快照按列表序（date 降序）且排除软删行"
    );

    // 派生列透传（NULL 保留为 None，匹配侧现算兜底语义不受影响）。
    // 本测试直插不经 Writer 挂点，手动失效后重建快照（域级「写后自动失效」
    // 由 transaction::tests::search 的 cache_path_write_then_search_stays_fresh 钉住）。
    insert_txn(&conn, "t-nopinyin", Some("无拼音"), None, "2026-02-04", 0);
    search_cache::invalidate();
    let has_pinyin = search_cache::with_shared_rows(&conn, |rows| {
        rows.iter()
            .find(|r| r.id == "t-nopinyin")
            .map(|r| r.note_pinyin.is_none())
            .unwrap()
    })
    .unwrap();
    assert!(has_pinyin, "note_pinyin 缺失应透传为 None");
}

#[test]
fn invalidate_and_connection_identity() {
    let conn = setup();
    insert_account(&conn, "a1");
    insert_txn(&conn, "t1", Some("万科物业"), Some("wkwy"), "2026-02-01", 0);

    let n1 = search_cache::with_shared_rows(&conn, |rows| rows.len()).unwrap();
    assert_eq!(n1, 1, "首读重建快照");

    // 另一连接（不同连接身份）：不得命中他连接快照，重建为自己的（空）视图。
    let conn2 = setup();
    let n2 = search_cache::with_shared_rows(&conn2, |rows| rows.len()).unwrap();
    assert_eq!(n2, 0, "异连接不串快照");

    // 同连接写后失效、再读重建：反映最新数据（写后脏标记 + 惰性重建闭环）。
    insert_txn(&conn, "t2", Some("招商银行"), Some("zsyh"), "2026-02-02", 0);
    search_cache::invalidate();
    let n3 = search_cache::with_shared_rows(&conn, |rows| rows.len()).unwrap();
    assert_eq!(n3, 2, "失效后重建应包含新写入行");

    // 空库连接：空快照合法（0 行），不报错。
    let empty = setup();
    insert_account(&empty, "a1");
    let n4 = search_cache::with_shared_rows(&empty, |rows| rows.len()).unwrap();
    assert_eq!(n4, 0, "空库快照为空数组");
}
