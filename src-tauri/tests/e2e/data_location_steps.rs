//! DataLocation 启动引导 BDD 步骤（issue #132）。
//!
//! 与 backup.feature 同一的文件级接缝：真临时目录驱动真实文件系统，
//! 每个 scenario 干净的目录现场，只断言外部可见行为（哪个目录的 `ledger.db`
//! 被创建/保留、内容是否完整、回退信号）。

use cucumber::{given, then, when};
use rusqlite::Connection;

use tauri_app_lib::commands::transactions::insert_transaction;
use tauri_app_lib::db::{
    data_location, init_db, new_uuid, open_connection, open_db_in, reset_db_in,
};
use tauri_app_lib::models::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::common::{insert_account, new_account_id};
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// 场景现场
// ---------------------------------------------------------------------------

fn ensure_default_dir(world: &mut LedgerWorld) {
    if world.dl_default_dir.is_none() {
        let dir = std::env::temp_dir().join(format!("ledger-e2e-dl-default-{}", new_uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        world.dl_default_dir = Some(dir);
    }
}

/// 在指定文件库中建账户与交易（经 Writer/行为层接缝，与真实写路径一致）。
fn seed_db(conn: &Connection, account: &str, count: usize) {
    insert_account(conn, &new_account_id(), account, "cash", "CNY");
    for i in 0..count {
        let input = TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents: 1000 + i as i64,
            currency_code: "CNY".into(),
            account_id: conn
                .query_row(
                    "SELECT id FROM accounts WHERE name = ?1 AND is_deleted = 0",
                    [account],
                    |r| r.get(0),
                )
                .unwrap(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: Some(format!("种子交易 {i}")),
            date: "2026-03-01".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        };
        insert_transaction(conn, input).unwrap();
    }
}

fn count_transactions(db_path: &std::path::Path) -> usize {
    let conn = open_connection(db_path).unwrap();
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
        [],
        |r| r.get::<_, i64>(0),
    )
    .unwrap() as usize
}

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

#[given(expr = "默认数据目录中已有一个含 {int} 条交易的库")]
fn default_dir_with_db(world: &mut LedgerWorld, count: usize) {
    ensure_default_dir(world);
    let dir = world.dl_default_dir.clone().unwrap();
    let mut conn = open_connection(dir.join(data_location::DB_FILE_NAME)).unwrap();
    init_db(&mut conn).unwrap();
    seed_db(&conn, "现金", count);
}

#[given(expr = "空的默认数据目录")]
fn empty_default_dir(world: &mut LedgerWorld) {
    ensure_default_dir(world);
}

#[given(expr = "指针文件指向目标目录")]
fn pointer_to_target(world: &mut LedgerWorld) {
    ensure_default_dir(world);
    let default_dir = world.dl_default_dir.clone().unwrap();
    let target = std::env::temp_dir().join(format!("ledger-e2e-dl-target-{}", new_uuid()));
    data_location::write_pointer(&default_dir, &target).unwrap();
    world.dl_target_dir = Some(target);
}

#[given(expr = "指针文件内容为损坏文本 {string}")]
fn pointer_corrupted(world: &mut LedgerWorld, raw: String) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    std::fs::write(default_dir.join(data_location::POINTER_FILE_NAME), raw).unwrap();
}

#[given(expr = "指针文件指向一个无法创建的目标目录")]
fn pointer_to_unusable_target(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    // 目标父级是一个普通文件 → create_dir_all 必然失败（跨平台）。
    let blocker = default_dir.join("blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let target = blocker.join("child");
    data_location::write_pointer(&default_dir, &target).unwrap();
    world.dl_target_dir = Some(target);
}

#[given(expr = "生效目录中存在一个损坏的库文件")]
fn active_dir_with_corrupt_db(world: &mut LedgerWorld) {
    let dir = std::env::temp_dir().join(format!("ledger-e2e-dl-reset-{}", new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(data_location::DB_FILE_NAME),
        b"definitely not a sqlite file",
    )
    .unwrap();
    world.dl_default_dir = Some(dir);
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "执行 DataLocation 引导并打开数据库")]
fn boot_and_open(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    let boot = data_location::boot(&default_dir);
    let opened = open_db_in(&boot.db_dir);
    assert!(opened.is_ok(), "引导后打开数据库失败: {:?}", opened.err());
    world.dl_conn = Some(opened.unwrap());
    world.last_boot = Some(boot);
}

#[when(expr = "在生效位置的库中记入 {int} 条标记交易")]
fn append_marker_transactions(world: &mut LedgerWorld, count: usize) {
    let state = world.dl_conn.as_ref().unwrap();
    let conn = state.conn.lock().unwrap();
    seed_db(&conn, "现金", count);
}

#[when(expr = "执行启动失败重置")]
fn run_reset(world: &mut LedgerWorld) {
    let dir = world.dl_default_dir.clone().unwrap();
    let conn = reset_db_in(&dir);
    assert!(conn.is_ok(), "启动失败重置失败: {:?}", conn.err());
    world.dl_conn = Some(conn.unwrap());
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "生效目录应为默认数据目录")]
fn active_dir_is_default(world: &mut LedgerWorld) {
    let boot = world.last_boot.as_ref().expect("尚未执行引导");
    let default_dir = world.dl_default_dir.as_ref().unwrap();
    assert_eq!(boot.db_dir, *default_dir, "生效目录应为默认数据目录");
}

#[then(expr = "生效目录应为目标目录")]
fn active_dir_is_target(world: &mut LedgerWorld) {
    let boot = world.last_boot.as_ref().expect("尚未执行引导");
    let target = world.dl_target_dir.as_ref().unwrap();
    assert_eq!(boot.db_dir, *target, "生效目录应为目标目录");
}

#[then(expr = "不应发生回退")]
fn no_fallback(world: &mut LedgerWorld) {
    let boot = world.last_boot.as_ref().expect("尚未执行引导");
    assert!(
        boot.fallback_reason.is_none(),
        "不应发生回退，实际: {:?}",
        boot.fallback_reason
    );
}

#[then(expr = "回退信号应包含 {string}")]
fn fallback_reason_contains(world: &mut LedgerWorld, needle: String) {
    let boot = world.last_boot.as_ref().expect("尚未执行引导");
    let reason = boot
        .fallback_reason
        .as_ref()
        .expect("应发生回退但回退信号为空");
    assert!(
        reason.contains(&needle),
        "回退信号不匹配: 期望包含 '{needle}', 实际 '{reason}'"
    );
}

#[then(expr = "打开的库应包含 {int} 条交易")]
fn opened_db_contains(world: &mut LedgerWorld, count: usize) {
    let state = world.dl_conn.as_ref().unwrap();
    let conn = state.conn.lock().unwrap();
    let actual: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(actual as usize, count, "打开的库交易数不符");
}

#[then(expr = "打开的库应为空库（0 条交易）")]
fn opened_db_is_empty(world: &mut LedgerWorld) {
    // 复用同一断言（显式 0），保持 feature 文案可读。
    opened_db_contains(world, 0);
}

#[then(expr = "目标目录的库应包含 {int} 条交易")]
fn target_db_contains(world: &mut LedgerWorld, count: usize) {
    let target = world.dl_target_dir.as_ref().unwrap();
    assert_eq!(
        count_transactions(&target.join(data_location::DB_FILE_NAME)),
        count,
        "目标目录的库交易数不符"
    );
}

#[then(expr = "默认数据目录的库应原样保留且仍包含 {int} 条交易")]
fn default_db_preserved(world: &mut LedgerWorld, count: usize) {
    let default_dir = world.dl_default_dir.as_ref().unwrap();
    assert_eq!(
        count_transactions(&default_dir.join(data_location::DB_FILE_NAME)),
        count,
        "默认目录的旧库应原样保留"
    );
}

#[then(expr = "重置后的库应为空库（0 条交易）")]
fn reset_db_is_empty(world: &mut LedgerWorld) {
    opened_db_contains(world, 0);
}

#[then(expr = "原库文件应被重命名为 .bak 保留")]
fn bak_file_preserved(world: &mut LedgerWorld) {
    let dir = world.dl_default_dir.as_ref().unwrap();
    let bak = dir
        .join(data_location::DB_FILE_NAME)
        .with_extension("db.bak");
    assert!(bak.exists(), ".bak 文件应保留: {}", bak.display());
}
