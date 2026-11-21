use rusqlite::params;

use ledger_infra::db::now_iso;
use ledger_sync_protocol::device::device_id;
use tauri_app_lib::ledger_transaction::TransactionInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::{
    DedupIdentity, TransactionBatch, compute_dedup_hash, dedup_identity,
    delete_transaction_internal, update_transaction_internal,
};

use crate::tests::common::make_buy_input;
use crate::tests::common::make_input;
use tauri_app_lib::test_support;

#[test]
fn dedup_hash_is_stable_for_same_fields() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-dedup", "现金", "cash", "CNY", 0);
    let a = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    let b = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    assert_eq!(compute_dedup_hash(&a), compute_dedup_hash(&b));
}

#[test]
fn dedup_hash_excludes_note_and_category() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-dedup", "现金", "cash", "CNY", 0);
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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-dedup", "现金", "cash", "CNY", 0);
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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-dedup", "现金", "cash", "CNY", 0);
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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    let input = make_input("acc-1", TransactionKind::Income, 1000, "2026-07-01");
    assert_eq!(
        compute_dedup_hash(&input),
        "d5a4ee5fa04913672a319a06c454283d74d312f13506a27fc81c72b09602a558"
    );
}

#[test]
fn dedup_hash_includes_funding_account_when_present() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-dedup", "现金", "cash", "CNY", 0);
    let base = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    let with_funding = TransactionInput {
        policy_id: None,
        funding_account_id: Some("acc-fund".into()),
        ..base.clone()
    };
    assert_ne!(
        compute_dedup_hash(&with_funding),
        compute_dedup_hash(&base),
        "仅出资账户不同应得到不同哈希"
    );
}

fn convert_input(to_instrument_id: &str, out_amount_cents: i64) -> TransactionInput {
    let base = make_input("acc-cv", TransactionKind::Income, 0, "2026-07-01");
    TransactionInput {
        kind: TransactionKind::Convert,
        to_instrument_id: Some(to_instrument_id.into()),
        to_quantity: Some(10.0),
        out_amount_cents: Some(out_amount_cents),
        in_amount_cents: Some(out_amount_cents),
        ..base
    }
}

#[test]
fn dedup_hash_includes_convert_leg_fields() {
    let first = convert_input("inst-b", 1000);
    let second = convert_input("inst-c", 1000);
    assert_ne!(
        compute_dedup_hash(&first),
        compute_dedup_hash(&second),
        "多腿转换仅转入标的不同的两腿不应互相去重"
    );
    assert_eq!(
        compute_dedup_hash(&first),
        compute_dedup_hash(&convert_input("inst-b", 1000))
    );
    assert_ne!(
        compute_dedup_hash(&first),
        compute_dedup_hash(&convert_input("inst-b", 2000))
    );
}

fn dividend_input(instrument_id: &str, amount_cents: i64) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Dividend,
        instrument_id: Some(instrument_id.into()),
        ..make_input(
            "acc-dv",
            TransactionKind::Dividend,
            amount_cents,
            "2026-07-01",
        )
    }
}

#[test]
fn dedup_hash_includes_dividend_instrument() {
    let first = dividend_input("inst-a", 3000);
    let second = dividend_input("inst-b", 3000);
    assert_ne!(
        compute_dedup_hash(&first),
        compute_dedup_hash(&second),
        "同日同额、仅归属标的不同的两笔分红不应互相去重"
    );
    assert_eq!(
        compute_dedup_hash(&first),
        compute_dedup_hash(&dividend_input("inst-a", 3000))
    );
}

#[test]
fn dedup_hash_unchanged_for_non_convert_inputs() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    let input = make_input("acc-1", TransactionKind::Income, 1000, "2026-07-01");
    assert_eq!(
        compute_dedup_hash(&input),
        "d5a4ee5fa04913672a319a06c454283d74d312f13506a27fc81c72b09602a558"
    );
}

#[test]
fn dedup_hash_matches_known_vector_with_funding_account() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    let mut input = make_input("acc-1", TransactionKind::Income, 1000, "2026-07-01");
    input.funding_account_id = Some("acc-fund".into());
    assert_eq!(
        compute_dedup_hash(&input),
        "fb83dd1889fe6f5f181f9437104f874d4a07ff99d81f73cecfa8bbb91775f9fe"
    );
}

#[test]
fn dedup_identity_distinguishes_funding_only_difference() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-inv-f", "inst-f");
    test_support::seed_account(&conn, "acc-fund-a", "卡A", "bank", "USD", 0);
    test_support::seed_account(&conn, "acc-fund-b", "卡B", "bank", "USD", 0);

    let mut a = make_buy_input("acc-inv-f", "inst-f", 10.0, 10000, 500);
    a.funding_account_id = Some("acc-fund-a".into());
    let mut b = make_buy_input("acc-inv-f", "inst-f", 10.0, 10000, 500);
    b.funding_account_id = Some("acc-fund-b".into());
    TransactionBatch::run(&conn, vec![a], true).unwrap();

    match dedup_identity(&conn, &b).unwrap() {
        DedupIdentity::New { dedup_hash } => {
            assert_eq!(
                dedup_hash,
                compute_dedup_hash(&b),
                "仅出资账户不同应判定为新写，且携带与落库回写一致的内容哈希"
            );
        }
        other => panic!("仅出资账户不同不应互相去重，实际: {other:?}"),
    }

    let mut c = make_buy_input("acc-inv-f", "inst-f", 10.0, 10000, 500);
    c.funding_account_id = Some("acc-fund-b".into());
    let r = TransactionBatch::run(&conn, vec![c], true).unwrap().results;
    assert!(r[0].success && !r[0].duplicate, "仅出资账户不同应写入新行");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "两笔各落一行");
}

#[test]
fn historical_row_without_funding_still_dedups_by_content_hash() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-hist", "现金", "cash", "CNY", 0);

    let input = make_input("acc-hist", TransactionKind::Income, 1000, "2026-07-01");
    let first = TransactionBatch::run(&conn, vec![input.clone()], true)
        .unwrap()
        .results;
    let id = first[0].id.clone().unwrap();
    conn.execute(
        "UPDATE transactions SET dedup_hash=?2 WHERE id=?1",
        params![
            id,
            "cbe601c2ff074b00086e739a0a85ae761c22aa7078bb2488508ae7472a510b06"
        ],
    )
    .unwrap();

    match dedup_identity(&conn, &input).unwrap() {
        DedupIdentity::Existing { id } => {
            assert_eq!(id, None, "内容哈希命中回传 id:None（冻结契约，不回归）");
        }
        other => panic!("历史行同内容重导应命中去重，实际: {other:?}"),
    }
}

#[test]
fn dedup_identity_key_hit_returns_existing_id() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ident", "现金", "cash", "CNY", 0);

    let mut a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let created = TransactionBatch::run(&conn, vec![a.clone()], true)
        .unwrap()
        .results;
    let existing_id = created[0].id.clone().unwrap();

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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ident", "现金", "cash", "CNY", 0);

    let a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    TransactionBatch::run(&conn, vec![a.clone()], true).unwrap();

    match dedup_identity(&conn, &a).unwrap() {
        DedupIdentity::Existing { id } => {
            assert_eq!(id, None, "内容哈希命中应回传 id:None（冻结契约，不回归）");
        }
        other => panic!("同内容应判定为命中已有，实际: {other:?}"),
    }
}

#[test]
fn dedup_identity_new_for_fresh_row() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ident", "现金", "cash", "CNY", 0);

    let a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    TransactionBatch::run(&conn, vec![a], true).unwrap();

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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ident", "现金", "cash", "CNY", 0);

    let mut a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let mut b = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    b.idempotency_key = Some("file:2:1".into());
    let r = TransactionBatch::run(&conn, vec![a.clone(), b], true)
        .unwrap()
        .results;
    let id_a = r[0].id.clone().unwrap();

    let keyless = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    match dedup_identity(&conn, &keyless).unwrap() {
        DedupIdentity::Existing { id } => {
            assert_eq!(id, None, "内容哈希命中应回传 id:None");
        }
        other => panic!("同内容无键应命中已有，实际: {other:?}"),
    }

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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ident", "现金", "cash", "CNY", 0);

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
    let keyless = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    match dedup_identity(&conn, &keyless).unwrap() {
        DedupIdentity::New { .. } => {}
        other => panic!("软删除后同内容应判定为新写，实际: {other:?}"),
    }
}

#[test]
fn dedup_ignores_soft_deleted_transactions() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-dedup", "现金", "cash", "CNY", 0);

    let input = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    let first = TransactionBatch::run(&conn, vec![input.clone()], true)
        .unwrap()
        .results;
    let id = first[0].id.clone().unwrap();

    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        params![id, now_iso(), device_id(&conn).unwrap()],
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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-key", "现金", "cash", "CNY", 0);

    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let first = TransactionBatch::run(&conn, vec![a.clone()], true)
        .unwrap()
        .results;
    let id = first[0].id.clone().unwrap();
    delete_transaction_internal(&conn, &id).unwrap();

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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-reimport", "现金", "cash", "CNY", 0);

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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-key", "现金", "cash", "CNY", 0);
    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let first = TransactionBatch::run(&conn, vec![a.clone()], true)
        .unwrap()
        .results;
    let id = first[0].id.clone().unwrap();

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
