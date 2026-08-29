use std::path::PathBuf;

use cucumber::{then, when};

use tauri_app_lib::auto_backup::AttemptOutcome;
use tauri_app_lib::commands::backup::{
    BackupKind, backup_db_to, expected_schema_version, read_backup_kind, restore_db_from,
};
use tauri_app_lib::commands::transactions::{
    delete_transaction_internal, update_transaction_internal,
};
use tauri_app_lib::db::{new_uuid, open_connection};
use tauri_app_lib::models::TransactionInput;
use tauri_app_lib::settings::{self, SettingKey};
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::world::LedgerWorld;

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ledger-e2e-backup-{name}-{}.db", new_uuid()))
}

fn temp_safety_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ledger-e2e-safety-{}", new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "备份数据库到临时文件")]
fn backup_to_temp(world: &mut LedgerWorld) {
    let target = temp_path("backup.zip");
    let result = backup_db_to(
        &world.db.conn.lock().unwrap_or_else(|e| e.into_inner()),
        &target,
        "0.2.0",
        BackupKind::Manual,
    );
    assert!(result.is_ok(), "备份失败: {:?}", result.err());
    world.last_backup_path = Some(target);
}

/// 真实走自动备份触发入口（前置业务写已置脏、开关默认开启），产物落到独立临时目录。
#[when(expr = "自动备份数据库到临时目录")]
fn auto_backup_to_temp(world: &mut LedgerWorld) {
    let dir = std::env::temp_dir().join(format!("ledger-e2e-auto-backup-{}", new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let outcome = tauri_app_lib::auto_backup::run_due_backup(
        &world.db.conn.lock().unwrap_or_else(|e| e.into_inner()),
        Some(dir.to_str().unwrap()),
        "0.2.0",
        chrono::Utc::now(),
    );
    assert!(
        matches!(outcome, AttemptOutcome::Performed { .. }),
        "自动备份应执行，实际 {outcome:?}"
    );
    if let AttemptOutcome::Performed { path } = outcome {
        world.last_auto_backup_path = Some(PathBuf::from(path));
    }
}

#[when(expr = "删除全部交易")]
fn delete_all_txns(world: &mut LedgerWorld) {
    world
        .conn()
        .execute_batch("UPDATE transactions SET is_deleted=1")
        .unwrap();
}

#[when(expr = "从备份恢复到临时数据库")]
fn restore_to_temp(world: &mut LedgerWorld) {
    let backup = world.last_backup_path.clone().expect("尚未备份");
    let db_path = temp_path("restored.db");
    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected);
    assert!(result.is_ok(), "恢复失败: {:?}", result.err());
    world.restored_db_path = Some(db_path);
    std::fs::remove_dir_all(&safety_dir).ok();
}

#[when(expr = "尝试从更高 schema 版本恢复")]
fn try_newer_restore(world: &mut LedgerWorld) {
    // 构造一个 schema 版本更高的库文件作为"备份"。
    let newer = temp_path("newer.db");
    {
        let conn = open_connection(&newer).unwrap();
        conn.execute_batch("PRAGMA user_version = 999").unwrap();
    }
    let db_path = temp_path("target.db");
    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    world.last_error = match restore_db_from(&newer, &db_path, &safety_dir, expected) {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
    std::fs::remove_dir_all(&safety_dir).ok();
    std::fs::remove_file(&newer).ok();
    std::fs::remove_file(&db_path).ok();
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "备份文件应存在")]
fn backup_exists(world: &mut LedgerWorld) {
    let p = world.last_backup_path.as_ref().expect("尚未备份");
    assert!(p.exists(), "备份文件不存在: {}", p.display());
}

#[then(expr = "备份包应包含 {string} 与 {string}")]
fn backup_contains(world: &mut LedgerWorld, a: String, b: String) {
    let p = world.last_backup_path.as_ref().expect("尚未备份");
    let file = std::fs::File::open(p).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(names.contains(&a), "缺少条目 {a}: {names:?}");
    assert!(names.contains(&b), "缺少条目 {b}: {names:?}");
}

#[then(expr = "备份包内的数据库应包含 {int} 条交易")]
fn backup_db_has_txns(world: &mut LedgerWorld, expected: i64) {
    let p = world.last_backup_path.as_ref().expect("尚未备份");
    let file = std::fs::File::open(p).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut db_entry = archive.by_name("ledger.db").unwrap();
    let out = temp_path("extract.db");
    let mut out_f = std::fs::File::create(&out).unwrap();
    std::io::copy(&mut db_entry, &mut out_f).unwrap();
    drop(out_f);
    let conn = open_connection(&out).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, expected, "备份包内交易数量不匹配");
    std::fs::remove_file(&out).ok();
}

#[then(expr = "恢复的数据库应包含 {int} 条交易")]
fn restored_has_txns(world: &mut LedgerWorld, expected: i64) {
    let p = world.restored_db_path.as_ref().expect("尚未恢复");
    let conn = open_connection(p).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, expected, "恢复出的交易数量不匹配");
}

// ---------------------------------------------------------------------------
// 备份产物来源标记（issue #127）
// ---------------------------------------------------------------------------

#[then(expr = "备份元数据来源应为 {string}")]
fn backup_meta_kind_manual(world: &mut LedgerWorld, expected: String) {
    let p = world.last_backup_path.as_ref().expect("尚未手动备份");
    assert_eq!(
        read_backup_kind(p).unwrap().to_string(),
        expected,
        "手动备份元数据来源不匹配"
    );
}

#[then(expr = "自动备份元数据来源应为 {string}")]
fn backup_meta_kind_auto(world: &mut LedgerWorld, expected: String) {
    let p = world.last_auto_backup_path.as_ref().expect("尚未自动备份");
    assert_eq!(
        read_backup_kind(p).unwrap().to_string(),
        expected,
        "自动备份元数据来源不匹配"
    );
}

// ---------------------------------------------------------------------------
// 脏标记挂钩（issue #126）
// ---------------------------------------------------------------------------

use tauri_app_lib::auto_backup::get_state;

#[then(expr = "自动备份脏标记应为真")]
fn auto_backup_dirty(world: &mut LedgerWorld) {
    let state = get_state(&world.db.conn.lock().unwrap_or_else(|e| e.into_inner())).unwrap();
    assert!(state.dirty, "业务写库成功后脏标记应为真");
}

#[then(expr = "自动备份脏标记应为假")]
fn auto_backup_clean(world: &mut LedgerWorld) {
    let state = get_state(&world.db.conn.lock().unwrap_or_else(|e| e.into_inner())).unwrap();
    assert!(!state.dirty, "未发生业务写库时脏标记应为默认假");
}

#[when(expr = "删除最近创建的交易")]
fn delete_last_transaction(world: &mut LedgerWorld) {
    let id = world.last_transaction_id.clone().expect("没有可删除的交易");
    // 与 IPC 命令同形态：经连接层统一写入口（ADR-0032）删除，成功即置脏。
    world
        .db
        .write(|conn| delete_transaction_internal(conn, &id))
        .unwrap();
}

/// 设置写入（`app_settings`，经 settings 模块单点收口，普通锁不走出入口）——
/// ADR-0032 的豁免路径：不置脏。
#[when(expr = "写入一项设置")]
fn write_a_setting(world: &mut LedgerWorld) {
    let conn = world.db.conn.lock().unwrap_or_else(|e| e.into_inner());
    settings::set(&conn, SettingKey::AutoBackupEnabled, &false).expect("写入设置");
}

/// 尝试把最近创建的交易改为非法金额（金额必须大于 0）：修改事务内失败回滚，
/// 写入口闭包失败不置脏（ADR-0032）。错误记入 last_error 供「应返回错误」断言。
#[when(expr = "尝试把最近创建的交易修改为非法金额")]
fn update_last_transaction_invalid_amount(world: &mut LedgerWorld) {
    let id = world.last_transaction_id.clone().expect("没有可修改的交易");
    let input = TransactionInput {
        kind: TransactionKind::Expense,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: "acc-any".into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        merchant_name: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-02-01".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    world.last_error = match world
        .db
        .write(|conn| update_transaction_internal(conn, &id, input))
    {
        Ok(()) => Some(String::from("预期失败但成功了")),
        Err(e) => Some(e.to_string()),
    };
}

#[then(expr = "恢复的数据库自动备份状态应为「未脏且已重新计时」")]
fn restored_auto_backup_state_reset(world: &mut LedgerWorld) {
    let p = world.restored_db_path.as_ref().expect("尚未恢复");
    let conn = open_connection(p).unwrap();
    let state = get_state(&conn).unwrap();
    assert!(!state.dirty, "恢复后脏标记应被重置为假");
    assert!(
        state.last_backup_at.is_some(),
        "恢复后上次备份锚点应重新计时"
    );
}
