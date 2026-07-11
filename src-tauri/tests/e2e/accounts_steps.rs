use cucumber::{when, then};
use rusqlite::params;

use tauri_app_lib::db::{balance::compute_balance, device_id, new_uuid, now_iso};

use crate::common::query_accounts_by_name;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "创建账户 {string} 类型 {string} 币种 {string} 初始余额 {int}")]
fn create_account(
    world: &mut LedgerWorld,
    name: String,
    kind: String,
    currency: String,
    initial_balance: i64,
) {
    let id = new_uuid();
    let now = now_iso();
    world
        .conn
        .execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
            params![id, name, kind, currency, initial_balance, now, now, 1, device_id()],
        )
        .unwrap();
    world.account_name_to_id.insert(name, id);
}

#[when(expr = "删除账户 {string}")]
fn delete_account(world: &mut LedgerWorld, name: String) {
    let id = world.account_id(&name);
    world
        .conn
        .execute(
            "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
            params![id, now_iso(), device_id()],
        )
        .unwrap();
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "账户列表应包含 {int} 条记录")]
fn check_account_count(world: &mut LedgerWorld, expected: i64) {
    let count: i64 = world
        .conn
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, expected, "账户数量不匹配");
}

#[then(expr = "{string} 账户余额应为 {int}")]
fn check_balance(world: &mut LedgerWorld, name: String, expected: i64) {
    let id = world.account_id(&name);
    let balance = compute_balance(&world.conn, &id).unwrap();
    assert_eq!(balance, expected, "账户 '{}' 余额不匹配", name);
}

#[then(expr = "账户列表应包含 {string}")]
fn check_account_exists(world: &mut LedgerWorld, name: String) {
    let accounts = query_accounts_by_name(&world.conn);
    assert!(
        accounts.contains(&name),
        "账户列表应包含 '{}'，但实际为 {:?}",
        name,
        accounts
    );
}

#[then(expr = "账户列表不应包含 {string}")]
fn check_account_not_exists(world: &mut LedgerWorld, name: String) {
    let accounts = query_accounts_by_name(&world.conn);
    assert!(
        !accounts.contains(&name),
        "账户列表不应包含 '{}'，但实际为 {:?}",
        name,
        accounts
    );
}
