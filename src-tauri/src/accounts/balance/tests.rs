//! [`affected_accounts`](super::affected_accounts)「受影响账户」并集口径的直测
//! （issue #533 / spec #519）。纯函数无数据库环境；形态仿 Writer 接缝按主题
//! 拆分单测先例：单端、双侧、旧新重叠、去重与首见顺序、空引用，
//! 并按创建 / 修改 / 删除三种写入形态覆盖调用面。

use super::affected_accounts;

// ---------------------------------------------------------------------------
// 单端：行上只有 account_id 一端（income/expense/refund/buy/sell 等）
// ---------------------------------------------------------------------------

/// 创建形态（旧行 None）：单端行推导只含转出账户一端。
#[test]
fn create_single_sided_row_yields_only_account_id() {
    let affected = affected_accounts(None, Some(("acc-a", None)));
    assert_eq!(affected, vec!["acc-a"]);
}

/// 删除形态（新行 None）：单端行推导只含原行账户一端。
#[test]
fn delete_single_sided_row_yields_only_account_id() {
    let affected = affected_accounts(Some(("acc-a", None)), None);
    assert_eq!(affected, vec!["acc-a"]);
}

// ---------------------------------------------------------------------------
// 双侧：transfer 行两端（account_id + to_account_id）
// ---------------------------------------------------------------------------

/// 创建形态：transfer 双侧都进集合，行内先 account 后 to——与既有创建推导
/// 顺序一致（接线无行为变化）。
#[test]
fn create_transfer_row_yields_both_sides_out_first() {
    let affected = affected_accounts(None, Some(("acc-out", Some("acc-in"))));
    assert_eq!(affected, vec!["acc-out", "acc-in"]);
}

/// 删除形态：原行 transfer 双侧都进集合。
#[test]
fn delete_transfer_row_yields_both_sides() {
    let affected = affected_accounts(Some(("acc-out", Some("acc-in"))), None);
    assert_eq!(affected, vec!["acc-out", "acc-in"]);
}

// ---------------------------------------------------------------------------
// 旧新重叠：修改形态的旧行 ∪ 新行
// ---------------------------------------------------------------------------

/// 修改形态：账户未移动时旧新两行完全重叠，去重后各账户只出现一次。
#[test]
fn update_without_move_unions_old_and_new_dedup() {
    let affected = affected_accounts(
        Some(("acc-a", Some("acc-b"))),
        Some(("acc-a", Some("acc-b"))),
    );
    assert_eq!(affected, vec!["acc-a", "acc-b"]);
}

/// 修改形态：跨账户移动（旧新无重叠）时两侧都进集合。
#[test]
fn update_with_account_move_unions_both() {
    let affected = affected_accounts(Some(("acc-old", None)), Some(("acc-new", None)));
    assert_eq!(affected, vec!["acc-old", "acc-new"]);
}

// ---------------------------------------------------------------------------
// 去重与首见顺序
// ---------------------------------------------------------------------------

/// 重复引用折叠到首见位置：旧新两端互相重叠（如 swap）不产生重复项。
#[test]
fn duplicates_collapse_to_first_occurrence() {
    let affected = affected_accounts(
        Some(("acc-a", Some("acc-b"))),
        Some(("acc-b", Some("acc-a"))),
    );
    assert_eq!(affected, vec!["acc-a", "acc-b"]);
}

/// 首见顺序保持：旧行引用先于新行引用，新行行内先 account 后 to。
#[test]
fn first_seen_order_preserved_across_old_and_new() {
    let affected = affected_accounts(Some(("acc-a", None)), Some(("acc-c", Some("acc-b"))));
    assert_eq!(affected, vec!["acc-a", "acc-c", "acc-b"]);
}

// ---------------------------------------------------------------------------
// 空引用
// ---------------------------------------------------------------------------

/// 两行皆缺（无账户引用可收集）得空集——调用方无需特判。
#[test]
fn no_rows_yield_empty_set() {
    assert!(affected_accounts(None, None).is_empty());
}
