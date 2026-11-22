use super::affected_accounts;
use ledger_transaction::seams::balance::RowAccounts;

fn row<'a>(
    account_id: &'a str,
    to_account_id: Option<&'a str>,
    funding_account_id: Option<&'a str>,
    funding_items: &'a [&'a str],
) -> RowAccounts<'a> {
    RowAccounts {
        account_id: Some(account_id),
        to_account_id,
        funding_account_id,
        funding_items,
    }
}

#[test]
fn create_single_sided_row_yields_only_account_id() {
    let affected = affected_accounts(None, Some(row("acc-a", None, None, &[])));
    assert_eq!(affected, vec!["acc-a"]);
}

#[test]
fn delete_single_sided_row_yields_only_account_id() {
    let affected = affected_accounts(Some(row("acc-a", None, None, &[])), None);
    assert_eq!(affected, vec!["acc-a"]);
}

#[test]
fn create_transfer_row_yields_both_sides_out_first() {
    let affected = affected_accounts(None, Some(row("acc-out", Some("acc-in"), None, &[])));
    assert_eq!(affected, vec!["acc-out", "acc-in"]);
}

#[test]
fn delete_transfer_row_yields_both_sides() {
    let affected = affected_accounts(Some(row("acc-out", Some("acc-in"), None, &[])), None);
    assert_eq!(affected, vec!["acc-out", "acc-in"]);
}

#[test]
fn create_buy_row_yields_account_and_funding() {
    let affected = affected_accounts(None, Some(row("acc-inv", None, Some("acc-cash"), &[])));
    assert_eq!(affected, vec!["acc-inv", "acc-cash"]);
}

#[test]
fn delete_buy_row_yields_account_and_funding() {
    let affected = affected_accounts(Some(row("acc-inv", None, Some("acc-cash"), &[])), None);
    assert_eq!(affected, vec!["acc-inv", "acc-cash"]);
}

#[test]
fn update_buy_row_funding_move_unions_old_and_new_funding() {
    let affected = affected_accounts(
        Some(row("acc-inv", None, Some("acc-cash-old"), &[])),
        Some(row("acc-inv", None, Some("acc-cash-new"), &[])),
    );
    assert_eq!(affected, vec!["acc-inv", "acc-cash-old", "acc-cash-new"]);
}

#[test]
fn create_split_funding_row_yields_all_item_accounts() {
    let new = RowAccounts {
        account_id: None,
        to_account_id: None,
        funding_account_id: None,
        funding_items: &["acc-wallet", "acc-balance"],
    };
    let affected = affected_accounts(None, Some(new));
    assert_eq!(affected, vec!["acc-wallet", "acc-balance"]);
}

#[test]
fn delete_split_funding_row_yields_all_item_accounts() {
    let old = RowAccounts {
        account_id: None,
        to_account_id: None,
        funding_account_id: None,
        funding_items: &["acc-wallet", "acc-balance"],
    };
    let affected = affected_accounts(Some(old), None);
    assert_eq!(affected, vec!["acc-wallet", "acc-balance"]);
}

#[test]
fn update_single_to_multi_conversion_unions_old_and_new_items() {
    let affected = affected_accounts(
        Some(row("acc-old", None, None, &[])),
        Some(RowAccounts {
            account_id: None,
            to_account_id: None,
            funding_account_id: None,
            funding_items: &["acc-wallet", "acc-balance"],
        }),
    );
    assert_eq!(affected, vec!["acc-old", "acc-wallet", "acc-balance"]);
}

#[test]
fn duplicate_item_accounts_collapse() {
    let new = RowAccounts {
        account_id: None,
        to_account_id: None,
        funding_account_id: None,
        funding_items: &["acc-wallet", "acc-wallet"],
    };
    let affected = affected_accounts(None, Some(new));
    assert_eq!(affected, vec!["acc-wallet"]);
}

#[test]
fn item_accounts_come_after_main_refs() {
    let new = RowAccounts {
        account_id: Some("acc-main"),
        to_account_id: None,
        funding_account_id: Some("acc-cash"),
        funding_items: &["acc-wallet"],
    };
    let affected = affected_accounts(None, Some(new));
    assert_eq!(affected, vec!["acc-main", "acc-cash", "acc-wallet"]);
}

#[test]
fn update_without_move_unions_old_and_new_dedup() {
    let affected = affected_accounts(
        Some(row("acc-a", Some("acc-b"), None, &[])),
        Some(row("acc-a", Some("acc-b"), None, &[])),
    );
    assert_eq!(affected, vec!["acc-a", "acc-b"]);
}

#[test]
fn update_with_account_move_unions_both() {
    let affected = affected_accounts(
        Some(row("acc-old", None, None, &[])),
        Some(row("acc-new", None, None, &[])),
    );
    assert_eq!(affected, vec!["acc-old", "acc-new"]);
}

#[test]
fn duplicates_collapse_to_first_occurrence() {
    let affected = affected_accounts(
        Some(row("acc-a", Some("acc-b"), None, &[])),
        Some(row("acc-b", Some("acc-a"), None, &[])),
    );
    assert_eq!(affected, vec!["acc-a", "acc-b"]);
}

#[test]
fn first_seen_order_preserved_across_old_and_new() {
    let affected = affected_accounts(
        Some(row("acc-a", None, None, &[])),
        Some(row("acc-c", Some("acc-b"), None, &[])),
    );
    assert_eq!(affected, vec!["acc-a", "acc-c", "acc-b"]);
}

#[test]
fn no_rows_yield_empty_set() {
    assert!(affected_accounts(None, None).is_empty());
}
