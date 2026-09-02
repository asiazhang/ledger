//! 导入去重身份（ImportDedup）测试：内容哈希（`compute_dedup_hash`）、判定
//! （`dedup_identity`，幂等键优先 / 内容哈希兜底，ADR-0010 冻结契约）与槽位联动
//! （软删 / 删除 / 编辑后同键同内容重跑的语义）。

use rusqlite::params;

use crate::db::{device_id, now_iso};
use crate::models::TransactionInput;
use crate::transaction::amount::TransactionKind;
use crate::transaction::{
    DedupIdentity, TransactionBatch, compute_dedup_hash, dedup_identity,
    delete_transaction_internal, update_transaction_internal,
};

use super::batch_common::{insert_account, make_input, setup};

// ---------------------------------------------------------------------------
// 内容哈希：字段稳定性、排除项、已知向量。
// ---------------------------------------------------------------------------

#[test]
fn dedup_hash_is_stable_for_same_fields() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");
    let a = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    let b = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    assert_eq!(compute_dedup_hash(&a), compute_dedup_hash(&b));
}

#[test]
fn dedup_hash_excludes_note_and_category() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");
    let base = make_input("acc-dedup", TransactionKind::Expense, 500, "2026-07-02");
    let with_note = TransactionInput {
        policy_id: None,
        note: Some("备注".into()),
        ..base.clone()
    };
    let with_category = TransactionInput {
        policy_id: None,
        category_id: Some("cat-1".into()),
        ..base.clone()
    };
    let h = compute_dedup_hash(&base);
    assert_eq!(compute_dedup_hash(&with_note), h);
    assert_eq!(compute_dedup_hash(&with_category), h);
}

#[test]
fn dedup_hash_changes_when_content_fields_change() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");
    let base = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    let h = compute_dedup_hash(&base);
    assert_ne!(
        compute_dedup_hash(&make_input(
            "acc-dedup",
            TransactionKind::Income,
            2000,
            "2026-07-01"
        )),
        h
    );
    assert_ne!(
        compute_dedup_hash(&make_input(
            "acc-dedup",
            TransactionKind::Expense,
            1000,
            "2026-07-01"
        )),
        h
    );
    assert_ne!(
        compute_dedup_hash(&make_input(
            "acc-other",
            TransactionKind::Income,
            1000,
            "2026-07-01"
        )),
        h
    );
    assert_ne!(
        compute_dedup_hash(&make_input(
            "acc-dedup",
            TransactionKind::Income,
            1000,
            "2026-07-02"
        )),
        h
    );
}

#[test]
fn dedup_hash_pins_empty_to_account_id_as_empty_string() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");
    let no_to = make_input("acc-dedup", TransactionKind::Transfer, 3000, "2026-07-03");
    let empty_to = TransactionInput {
        policy_id: None,
        to_account_id: Some("".into()),
        ..no_to.clone()
    };
    assert_eq!(
        compute_dedup_hash(&no_to),
        compute_dedup_hash(&empty_to),
        "缺省 to_account_id 应等同空串"
    );
    let with_to = TransactionInput {
        policy_id: None,
        to_account_id: Some("acc-to".into()),
        ..no_to.clone()
    };
    assert_ne!(
        compute_dedup_hash(&no_to),
        compute_dedup_hash(&with_to),
        "指定 to_account_id 应改变哈希"
    );
}

#[test]
fn dedup_hash_matches_known_sha256_vector() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    let input = make_input("acc-1", TransactionKind::Income, 1000, "2026-07-01");
    // sha256("2026-07-01|income|1000|CNY|acc-1|")
    assert_eq!(
        compute_dedup_hash(&input),
        "d5a4ee5fa04913672a319a06c454283d74d312f13506a27fc81c72b09602a558"
    );
}

// ---- dedup_identity 判定函数直接覆盖（issue #62）----

#[test]
fn dedup_identity_key_hit_returns_existing_id() {
    let conn = setup();
    insert_account(&conn, "acc-ident", "现金", "cash", "CNY");

    // 落库一笔带幂等键的交易。
    let mut a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let created = TransactionBatch::run(&conn, vec![a.clone()], true)
        .unwrap()
        .results;
    let existing_id = created[0].id.clone().unwrap();

    // 同键但内容不同：内容无关，仍按幂等键命中并回传已有 id。
    let mut b = make_input("acc-ident", TransactionKind::Expense, 2000, "2026-02-01");
    b.idempotency_key = Some("file:1:1".into());
    match dedup_identity(&conn, &b).unwrap() {
        DedupIdentity::Existing { id } => {
            assert_eq!(
                id.as_deref(),
                Some(existing_id.as_str()),
                "幂等键命中应回传已有 id"
            );
        }
        other => panic!("同键应判定为命中已有，实际: {other:?}"),
    }
}

#[test]
fn dedup_identity_hash_hit_returns_none() {
    let conn = setup();
    insert_account(&conn, "acc-ident", "现金", "cash", "CNY");

    let a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    TransactionBatch::run(&conn, vec![a.clone()], true).unwrap();

    // 无键同内容：内容哈希兜底命中，冻结契约回传 id:None（不回归）。
    match dedup_identity(&conn, &a).unwrap() {
        DedupIdentity::Existing { id } => {
            assert_eq!(id, None, "内容哈希命中应回传 id:None（冻结契约，不回归）");
        }
        other => panic!("同内容应判定为命中已有，实际: {other:?}"),
    }
}

#[test]
fn dedup_identity_new_for_fresh_row() {
    let conn = setup();
    insert_account(&conn, "acc-ident", "现金", "cash", "CNY");

    let a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    TransactionBatch::run(&conn, vec![a], true).unwrap();

    // 内容不同（且无键）：应判定为新写，且携带与落库回写一致的内容哈希。
    let fresh = make_input("acc-ident", TransactionKind::Expense, 2000, "2026-02-01");
    match dedup_identity(&conn, &fresh).unwrap() {
        DedupIdentity::New { dedup_hash } => {
            assert_eq!(
                dedup_hash,
                compute_dedup_hash(&fresh),
                "New 应携带与落库回写一致的内容哈希"
            );
        }
        other => panic!("内容不同应判定为新写，实际: {other:?}"),
    }
}

#[test]
fn dedup_identity_key_takes_precedence_over_content_hash() {
    let conn = setup();
    insert_account(&conn, "acc-ident", "现金", "cash", "CNY");

    // 两笔内容完全相同但幂等键不同的交易（内容哈希相同）：不同键都应保留。
    let mut a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let mut b = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    b.idempotency_key = Some("file:2:1".into());
    let r = TransactionBatch::run(&conn, vec![a.clone(), b], true)
        .unwrap()
        .results;
    let id_a = r[0].id.clone().unwrap();

    // 无键、内容与二者相同：内容哈希命中（回传 id:None，冻结契约）。
    let keyless = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    match dedup_identity(&conn, &keyless).unwrap() {
        DedupIdentity::Existing { id } => {
            assert_eq!(id, None, "内容哈希命中应回传 id:None");
        }
        other => panic!("同内容无键应命中已有，实际: {other:?}"),
    }

    // 同键 file:1:1 但内容不同：幂等键命中，回传 a 的 id（内容无关）。
    let mut c = make_input("acc-ident", TransactionKind::Expense, 2000, "2026-02-01");
    c.idempotency_key = Some("file:1:1".into());
    match dedup_identity(&conn, &c).unwrap() {
        DedupIdentity::Existing { id } => {
            assert_eq!(
                id.as_deref(),
                Some(id_a.as_str()),
                "同键命中应回传 file:1:1 那笔的 id"
            );
        }
        other => panic!("同键应命中已有，实际: {other:?}"),
    }
}

#[test]
fn dedup_identity_ignores_soft_deleted_rows() {
    let conn = setup();
    insert_account(&conn, "acc-ident", "现金", "cash", "CNY");

    // 带键落库后软删除：键路径与哈希路径都不应命中。
    let mut a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let created = TransactionBatch::run(&conn, vec![a.clone()], true)
        .unwrap()
        .results;
    let id = created[0].id.clone().unwrap();
    delete_transaction_internal(&conn, &id).unwrap();

    match dedup_identity(&conn, &a).unwrap() {
        DedupIdentity::New { .. } => {}
        other => panic!("软删除后同键应判定为新写，实际: {other:?}"),
    }
    // 无键路径同样忽略软删除行。
    let keyless = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    match dedup_identity(&conn, &keyless).unwrap() {
        DedupIdentity::New { .. } => {}
        other => panic!("软删除后同内容应判定为新写，实际: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 槽位联动：软删 / 删除 / 编辑后，键与内容哈希的「占位」释放与保持。
// ---------------------------------------------------------------------------

#[test]
fn dedup_ignores_soft_deleted_transactions() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");

    let input = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    let first = TransactionBatch::run(&conn, vec![input.clone()], true)
        .unwrap()
        .results;
    let id = first[0].id.clone().unwrap();

    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        params![id, now_iso(), device_id()],
    ).unwrap();

    let second = TransactionBatch::run(&conn, vec![input], true)
        .unwrap()
        .results;
    assert!(second[0].success && !second[0].duplicate && second[0].id.is_some());

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn batch_create_idempotency_key_soft_deleted_frees_slot() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");

    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let first = TransactionBatch::run(&conn, vec![a.clone()], true)
        .unwrap()
        .results;
    let id = first[0].id.clone().unwrap();
    delete_transaction_internal(&conn, &id).unwrap();

    // 软删除后同键重跑：部分唯一索引只约束未删除交易，应重新写入。
    let second = TransactionBatch::run(&conn, vec![a], true).unwrap().results;
    assert!(
        second[0].success && !second[0].duplicate && second[0].id.is_some(),
        "软删除后同键应重新写入"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn delete_transaction_internal_frees_dedup_slot_for_reimport() {
    let conn = setup();
    insert_account(&conn, "acc-reimport", "现金", "cash", "CNY");

    let input = make_input("acc-reimport", TransactionKind::Income, 1000, "2026-07-01");
    let first = TransactionBatch::run(&conn, vec![input.clone()], true)
        .unwrap()
        .results;
    let id = first[0].id.clone().unwrap();

    delete_transaction_internal(&conn, &id).unwrap();

    let second = TransactionBatch::run(&conn, vec![input], true)
        .unwrap()
        .results;
    assert!(
        second[0].success && !second[0].duplicate && second[0].id.is_some(),
        "删除后重跑应重新写入（duplicate=false）"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn update_transaction_internal_preserves_key_and_rerun_dedup() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");
    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let first = TransactionBatch::run(&conn, vec![a.clone()], true)
        .unwrap()
        .results;
    let id = first[0].id.clone().unwrap();

    // 编辑内容（金额/备注/日期），幂等键应保持不变。
    let mut edited = make_input("acc-key", TransactionKind::Income, 2000, "2026-01-03");
    edited.note = Some("改".into());
    update_transaction_internal(&conn, &id, edited).unwrap();

    let key: Option<String> = conn
        .query_row(
            "SELECT idempotency_key FROM transactions WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(key.as_deref(), Some("file:1:1"), "编辑不应改变幂等键");

    // 编辑后重跑同批导入（带同键）：仍按同键去重、返回已有 id → 不产生重复。
    let mut rerun = make_input("acc-key", TransactionKind::Income, 3000, "2026-02-01");
    rerun.idempotency_key = Some("file:1:1".into());
    let second = TransactionBatch::run(&conn, vec![rerun], true)
        .unwrap()
        .results;
    assert!(
        second[0].success && second[0].duplicate,
        "同键重跑应去重跳过"
    );
    assert_eq!(second[0].id.as_deref(), Some(id.as_str()));

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "编辑后重跑不应新增交易");
}
