//! 出资项读回与读闭包（issue #1860 / ADR-0138 决策 5/6/7）：列表/详情 attach、
//! refund 缺省比例派生（尾差归顺序位最小项）、涉及账户过滤含出资子行端、
//! 详情读快照探针。
//!
//! 全部经根包实例驱动（行为路径测试纪律，见 crate 根 lib.rs 头注）。

use tauri_app_lib::ledger_transaction::TransactionFundingInput;
use tauri_app_lib::ledger_transaction::TransactionInput;
use tauri_app_lib::ledger_transaction::TransactionListFilter;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::test_support;
use tauri_app_lib::test_support::ScratchDir;
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};

/// 分解条目构造器。
fn item(account_id: &str, amount_cents: i64, label: Option<&str>) -> TransactionFundingInput {
    TransactionFundingInput {
        account_id: account_id.into(),
        amount_cents,
        label: label.map(String::from),
    }
}

/// 组合支付样例（ADR-0138 背景：订单 ¥281.60 = 小金库 ¥28.53 + 余额 ¥253.07）。
fn seed_combined_payment(conn: &rusqlite::Connection) -> String {
    test_support::seed_account(conn, "acc-wallet", "小金库", "cash", "CNY", 0);
    test_support::seed_account(conn, "acc-balance", "余额", "cash", "CNY", 0);
    create_transaction_internal(
        conn,
        TransactionInput {
            account_id: None,
            funding: vec![
                item("acc-wallet", 2853, Some("定金")),
                item("acc-balance", 25307, None),
            ],
            ..crate::tests::common::make_input("", TransactionKind::Expense, 28160, "2026-01-01")
        },
    )
    .unwrap()
    .id
}

// ---------------------------------------------------------------------------
// 列表与详情 attach（决策 6：读回非空分解 ⇔ 存在分解行）
// ---------------------------------------------------------------------------

#[test]
fn list_and_detail_attach_fundings_in_sort_order() {
    let conn = test_support::open();
    let id = seed_combined_payment(&conn);

    // 列表：分解行 account_id 读回 null + 出资项按顺序位稳定返回（含标签）。
    let list = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(list.total, 1);
    let row = &list.items[0];
    assert_eq!(row.id, id);
    assert_eq!(row.account_id, None, "分解行主表账户列读回 null");
    assert_eq!(row.fundings.len(), 2);
    assert_eq!(row.fundings[0].account_id, "acc-wallet");
    assert_eq!(row.fundings[0].amount_cents, 2853);
    assert_eq!(row.fundings[0].label.as_deref(), Some("定金"));
    assert_eq!(row.fundings[1].account_id, "acc-balance");
    assert_eq!(row.fundings[1].amount_cents, 25307);
    assert!(!row.fundings[0].derived, "落库条目 derived 恒 false");

    // 详情：同一 attach 契约。
    let detail = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(detail.account_id, None);
    assert_eq!(detail.fundings.len(), 2);
    assert_eq!(detail.fundings[0].account_id, "acc-wallet");
}

#[test]
fn single_funding_rows_read_back_with_empty_fundings() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "现金", "cash", "CNY", 0);
    create_transaction_internal(
        &conn,
        crate::tests::common::make_input("acc", TransactionKind::Expense, 1000, "2026-01-01"),
    )
    .unwrap();

    let list = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(
        list.items[0].account_id.as_deref(),
        Some("acc"),
        "单出资行读回形态与现状兼容"
    );
    assert!(
        list.items[0].fundings.is_empty(),
        "无分解行的 fundings 恒空"
    );
}

// ---------------------------------------------------------------------------
// refund 缺省派生（决策 5：比例换算、尾差归顺序位最小项）
// ---------------------------------------------------------------------------

#[test]
fn refund_default_derivation_splits_by_ratio_with_remainder_to_first() {
    let conn = test_support::open();
    let expense_id = seed_combined_payment(&conn);

    // 退款 ¥140.80 = 28160 / 2：比例换算 2853/28160 = 14.265 → floor 1426，
    // 25307/28160 = 126.535 → floor 12653；合计 14079，尾差 1 分归顺序位最小项。
    let refund_id = create_transaction_internal(
        &conn,
        TransactionInput {
            account_id: None,
            refund_of_transaction_id: Some(expense_id.clone()),
            ..crate::tests::common::make_input("acc", TransactionKind::Refund, 14080, "2026-01-02")
        },
    )
    .unwrap()
    .id;

    let refund = get_transaction_internal(&conn, &refund_id).unwrap();
    assert_eq!(
        refund.account_id, None,
        "原支出为分解行：退款无单一账户可继承"
    );
    assert_eq!(refund.fundings.len(), 2, "缺省按出资比例派生");
    assert!(
        refund.fundings.iter().all(|f| f.derived),
        "派生条目带 derived 标记"
    );
    assert_eq!(refund.fundings[0].account_id, "acc-wallet");
    assert_eq!(refund.fundings[0].amount_cents, 1427, "floor 1426 + 尾差 1");
    assert_eq!(refund.fundings[1].account_id, "acc-balance");
    assert_eq!(refund.fundings[1].amount_cents, 12653);
    // Σ 派生 = 退款金额。
    let sum: i64 = refund.fundings.iter().map(|f| f.amount_cents).sum();
    assert_eq!(sum, 14080);
}

#[test]
fn refund_explicit_override_reads_back_persisted_breakdown() {
    let conn = test_support::open();
    let expense_id = seed_combined_payment(&conn);
    test_support::seed_account(&conn, "acc-x", "精准回退", "cash", "CNY", 0);

    let refund_id = create_transaction_internal(
        &conn,
        TransactionInput {
            account_id: None,
            refund_of_transaction_id: Some(expense_id),
            funding: vec![item("acc-x", 5000, None)],
            ..crate::tests::common::make_input("acc", TransactionKind::Refund, 5000, "2026-01-02")
        },
    )
    .unwrap()
    .id;

    let refund = get_transaction_internal(&conn, &refund_id).unwrap();
    assert_eq!(refund.account_id, None);
    assert_eq!(refund.fundings.len(), 1, "显式覆盖落库、不派生");
    assert!(!refund.fundings[0].derived);
    assert_eq!(refund.fundings[0].account_id, "acc-x");
}

// ---------------------------------------------------------------------------
// 涉及账户过滤含出资子行端（决策 7：InvolvingAccount 覆盖出资端）
// ---------------------------------------------------------------------------

#[test]
fn involving_account_filter_hits_funding_item_accounts() {
    let conn = test_support::open();
    seed_combined_payment(&conn);

    for acc in ["acc-wallet", "acc-balance"] {
        let filter = TransactionListFilter {
            involving_account_id: Some(acc.into()),
            ..TransactionListFilter::default()
        };
        let hit = list_transactions_internal(&conn, &filter).unwrap();
        assert_eq!(hit.total, 1, "按出资方 {acc} 过滤应命中组合支付行");
        assert_eq!(hit.items[0].fundings.len(), 2);
    }
}

#[test]
fn delete_funding_row_removes_it_from_involving_filter() {
    let conn = test_support::open();
    let id = seed_combined_payment(&conn);
    delete_transaction_internal(&conn, &id).unwrap();

    let filter = TransactionListFilter {
        involving_account_id: Some("acc-wallet".into()),
        ..TransactionListFilter::default()
    };
    let hit = list_transactions_internal(&conn, &filter).unwrap();
    assert_eq!(hit.total, 0, "软删行不参与涉及账户过滤（子行随主行失效）");
}

// ---------------------------------------------------------------------------
// 搜索页同一读回契约（接线：删除 search 闭包内 attach_fundings 即红）
// ---------------------------------------------------------------------------

/// 搜索页与列表页同一读回契约（issue #1860 / ADR-0138）：分解行经搜索读回
/// `fundings` 非空且按顺序位（`account_id` 同为 null）——搜索是 Transaction 行
/// 的第三个读入口（列表/详情/搜索），漏接即行呈现「无账户且无分解」假象。
#[test]
fn search_results_carry_funding_breakdown() {
    let conn = test_support::open();
    let id = seed_combined_payment(&conn);

    // 空关键字 + 金额筛选命中组合支付行（仅筛选路径同样回表取展示列）。
    let hit = search_transactions_internal(&conn, "", 1, 50, Some(28160), Some(28160), None, None)
        .unwrap();
    assert_eq!(hit.total, 1, "金额筛选应命中组合支付行");
    let row = &hit.items[0];
    assert_eq!(row.id, id);
    assert_eq!(row.account_id, None, "分解行主表账户列读回 null");
    assert_eq!(row.fundings.len(), 2, "搜索页分解行同样携带出资项");
    assert_eq!(row.fundings[0].account_id, "acc-wallet");
    assert_eq!(row.fundings[1].amount_cents, 25307);
}

/// 存在软删账户时分解行不被滤出（NULL NOT IN 语义回归，ADR-0138）：账户引用
/// 为 NULL 的行在关键字路径（NullableNotIn 子句）与仅筛选路径（行级字典过滤
/// 放行 NULL）都必须照常命中——历史上两路径都假定 account_id 恒非空。
#[test]
fn search_does_not_drop_split_rows_when_soft_deleted_accounts_exist() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-gone", "已删账户", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-wallet", "小金库", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-balance", "余额", "cash", "CNY", 0);
    let created = create_transaction_internal(
        &conn,
        TransactionInput {
            account_id: None,
            funding: vec![
                item("acc-wallet", 2853, None),
                item("acc-balance", 25307, None),
            ],
            note: Some("组合支付订单".into()),
            ..crate::tests::common::make_input("", TransactionKind::Expense, 28160, "2026-01-01")
        },
    )
    .unwrap();

    // 软删一个在场账户：触发关键字路径 NullableNotIn 子句与仅筛选路径字典过滤。
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-gone'", [])
        .unwrap();

    // 关键字路径：备注命中，分解行不被 NULL NOT IN 语义滤出。
    let kw =
        search_transactions_internal(&conn, "组合支付", 1, 50, None, None, None, None).unwrap();
    assert_eq!(kw.total, 1, "关键字路径：软删账户在场不滤出分解行");
    assert_eq!(kw.items[0].id, created.id);

    // 仅筛选路径：行级字典过滤放行 NULL 账户。
    let filtered =
        search_transactions_internal(&conn, "", 1, 50, Some(28160), Some(28160), None, None)
            .unwrap();
    assert_eq!(filtered.total, 1, "仅筛选路径：软删账户在场不滤出分解行");
}

// ---------------------------------------------------------------------------
// 详情读快照探针（读闭包纪律：删除接线即红）
// ---------------------------------------------------------------------------

/// 详情读回是多语句读闭包（主行 SELECT ↔ 出资子行 SELECT）：写提交落在两语句
/// 之间会读到「主行旧、子行新」的混搭行。探针在子行查询开始前于另一连接改写
/// 出资项金额——
/// - 读闭包无快照保护（红）：主行读旧子行读新，fundings 与基线漂移；
/// - 读闭包收进读事务（绿）：注入写被挡，详情与基线同快照。
#[test]
fn detail_main_row_and_fundings_share_one_snapshot() {
    let dir = ScratchDir::new("tx-detail-read-snapshot");
    let conn = test_support::open_file(dir.path());
    let id = seed_combined_payment(&conn);

    let baseline = get_transaction_internal(&conn, &id).unwrap();

    // 探针：marker = 子行 SELECT（详情闭包的第二个语句）——注入落在主行读后、
    // 子行读前，另一连接把首条出资项金额 +7。
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM transaction_fundings WHERE transaction_id IN",
        &[&format!(
            "UPDATE transaction_fundings SET amount_cents = amount_cents + 7 \
             WHERE transaction_id = '{id}' AND sort = 0"
        )],
    );
    let after = get_transaction_internal(&conn, &id).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中详情读闭包（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );
    assert_eq!(
        baseline.fundings, after.fundings,
        "详情主行与出资子行必须同快照——注入写要么整体进快照、要么整体不进"
    );
}

/// 列表读闭包（页行 SELECT ↔ 出资子行 SELECT）同快照——与详情探针同款（ADR-0138
/// 影响节把「列表（页行集 ↔ 出资子行）」一并列入探针清单；删除列表闭包内
/// attach_fundings 接线本测试变红）。
#[test]
fn list_page_rows_and_fundings_share_one_snapshot() {
    let dir = ScratchDir::new("tx-list-read-snapshot");
    let conn = test_support::open_file(dir.path());
    let id = seed_combined_payment(&conn);

    let baseline = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();

    // 探针：marker = 子行 SELECT——注入落在页行读后、子行读前，另一连接把首条
    // 出资项金额 +7。
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM transaction_fundings WHERE transaction_id IN",
        &[&format!(
            "UPDATE transaction_fundings SET amount_cents = amount_cents + 7 \
             WHERE transaction_id = '{id}' AND sort = 0"
        )],
    );
    let after = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中列表读闭包（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );
    assert_eq!(
        baseline.items[0].fundings, after.items[0].fundings,
        "列表页行与出资子行必须同快照——注入写要么整体进快照、要么整体不进"
    );
}
