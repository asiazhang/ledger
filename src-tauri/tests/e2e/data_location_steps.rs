//! DataLocation 启动引导 BDD 步骤（issue #132）。
//!
//! 与 backup.feature 同一的文件级接缝：真临时目录驱动真实文件系统，
//! 每个 scenario 干净的目录现场，只断言外部可见行为（哪个目录的 `ledger.db`
//! 被创建/保留、内容是否完整、回退信号）。

use cucumber::{given, then, when};
use rusqlite::Connection;

use tauri_app_lib::commands::data_location::{gather_info, validate_and_commit};
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
    let account_id: String = conn
        .query_row(
            "SELECT id FROM accounts WHERE name = ?1 AND is_deleted = 0",
            [account],
            |r| r.get(0),
        )
        .unwrap();
    for i in 0..count {
        let input = TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents: 1000 + i as i64,
            currency_code: "CNY".into(),
            account_id: account_id.clone(),
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

/// 抽取关键表的内容快照（排序后逐行拼接），供搬迁前后一致性比对。
fn key_table_fingerprint(db_path: &std::path::Path) -> String {
    let conn = open_connection(db_path).unwrap();
    let mut fingerprint = String::new();
    for sql in [
        "SELECT id, name, type, currency_code FROM accounts WHERE is_deleted = 0 ORDER BY id",
        "SELECT id, name, kind FROM categories WHERE is_deleted = 0 ORDER BY id",
        "SELECT id, kind, amount_cents, currency_code, account_id, date, note, is_deleted \
         FROM transactions ORDER BY id",
    ] {
        let mut stmt = conn.prepare(sql).unwrap();
        let column_count = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let cols: Vec<String> = (0..column_count)
                .map(|i| match row.get_ref(i).unwrap() {
                    rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                    rusqlite::types::ValueRef::Integer(n) => n.to_string(),
                    rusqlite::types::ValueRef::Null => "∅".into(),
                    other => format!("{other:?}"),
                })
                .collect();
            fingerprint.push_str(&cols.join("\u{1}"));
            fingerprint.push('\u{2}');
        }
        fingerprint.push('\u{3}');
    }
    fingerprint
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

#[given(expr = "指针文件指向已含 {int} 条交易库的目标目录")]
fn pointer_to_target_with_db(world: &mut LedgerWorld, count: usize) {
    pointer_to_target(world);
    let target = world.dl_target_dir.clone().unwrap();
    // 真实流程中目标目录在命令层校验时已创建（#133）；场景现场直接准备就绪。
    std::fs::create_dir_all(&target).unwrap();
    let mut conn = open_connection(target.join(data_location::DB_FILE_NAME)).unwrap();
    init_db(&mut conn).unwrap();
    seed_db(&conn, "目标现金", count);
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

#[given(expr = "默认数据目录中存在一个损坏的库文件")]
fn default_dir_with_corrupt_db(world: &mut LedgerWorld) {
    let dir = std::env::temp_dir().join(format!("ledger-e2e-dl-reset-{}", new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(data_location::DB_FILE_NAME),
        b"definitely not a sqlite file",
    )
    .unwrap();
    world.dl_default_dir = Some(dir);
}

#[given(expr = "记录默认数据目录库文件的字节")]
fn record_default_db_bytes(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    let bytes = std::fs::read(default_dir.join(data_location::DB_FILE_NAME)).unwrap();
    world.dl_default_db_bytes = Some(bytes);
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

#[when(expr = "尝试执行 DataLocation 引导并打开数据库（预期打开失败）")]
fn boot_and_open_expect_failure(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    let boot = data_location::boot(&default_dir);
    let opened = open_db_in(&boot.db_dir);
    assert!(
        opened.is_err(),
        "预期打开失败，实际成功（生效目录 {:?}）",
        boot.db_dir
    );
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

#[then(expr = "目标目录的库的关键表内容应与默认目录的库一致")]
fn relocated_db_matches_source(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.as_ref().unwrap();
    let target = world.dl_target_dir.as_ref().unwrap();
    let source = key_table_fingerprint(&default_dir.join(data_location::DB_FILE_NAME));
    let relocated = key_table_fingerprint(&target.join(data_location::DB_FILE_NAME));
    assert_eq!(
        source, relocated,
        "搬迁后的库关键表（accounts/categories/transactions）应与原库内容一致"
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

#[then(expr = "默认数据目录库文件的字节应保持不变")]
fn default_db_bytes_unchanged(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.as_ref().unwrap();
    let bytes = std::fs::read(default_dir.join(data_location::DB_FILE_NAME)).unwrap();
    assert_eq!(
        bytes,
        *world.dl_default_db_bytes.as_ref().expect("未记录字节快照"),
        "回退场景中默认目录的既有库文件不应被改动"
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

// ---------------------------------------------------------------------------
// 领域命令层（issue #133）：直接调用命令层内部函数（与真实 IPC 命令同一实现）
// ---------------------------------------------------------------------------

/// 提交更改意图并记录结果（成功 → outcome，失败 → last_error）。
fn submit(world: &mut LedgerWorld, target: &std::path::Path, adopt_existing: bool) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    match validate_and_commit(&default_dir, target, adopt_existing) {
        Ok(outcome) => world.dl_last_outcome = Some(outcome),
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[when(expr = "向一个未占用的新目录提交更改意图")]
fn submit_to_fresh_dir(world: &mut LedgerWorld) {
    ensure_default_dir(world);
    let target = std::env::temp_dir().join(format!("ledger-e2e-dl-new-{}", new_uuid()));
    world.dl_target_dir = Some(target.clone());
    submit(world, &target, false);
}

#[when(expr = "向一个无法创建的目录提交更改意图")]
fn submit_to_uncreatable(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    // 目标父级是一个普通文件 → create_dir_all 必然失败（跨平台）。
    let blocker = default_dir.join("blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let target = blocker.join("child");
    world.dl_target_dir = Some(target.clone());
    submit(world, &target, false);
}

#[when(expr = "向该目标目录提交更改意图（不接管既有库）")]
fn submit_no_adopt(world: &mut LedgerWorld) {
    let target = world.dl_target_dir.clone().unwrap();
    submit(world, &target, false);
}

#[when(expr = "选择接管既有库并再次提交")]
fn submit_adopt(world: &mut LedgerWorld) {
    let target = world.dl_target_dir.clone().unwrap();
    submit(world, &target, true);
}

#[when(expr = "提交恢复默认位置（不接管既有库）")]
fn restore_default_no_adopt(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    submit(world, &default_dir, false);
}

#[when(expr = "选择接管既有库并再次提交恢复默认")]
fn restore_default_adopt(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    submit(world, &default_dir, true);
}

#[when(expr = "查询 DataLocation 信息")]
fn query_info(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    // 与真实命令一致：active/fallback 来自启动期已登记的引导结果。
    let (active_dir, fallback_reason) = match world.last_boot.as_ref() {
        Some(boot) => (boot.db_dir.clone(), boot.fallback_reason.clone()),
        None => (default_dir.clone(), None),
    };
    world.dl_last_info = Some(gather_info(
        &default_dir,
        &active_dir,
        fallback_reason.as_deref(),
    ));
}

// ---------------------------------------------------------------------------
// Then：命令层结果断言
// ---------------------------------------------------------------------------

#[then(expr = "提交结果应为意图已落盘")]
fn outcome_committed(world: &mut LedgerWorld) {
    let outcome = world.dl_last_outcome.as_ref().expect("无提交结果");
    assert!(
        outcome.committed && !outcome.requires_choice,
        "提交结果应为意图已落盘，实际 {outcome:?}"
    );
    // 意图指向是否正确由「指针文件应指向目标/默认数据目录」步骤断言。
}

#[then(expr = "提交结果应为需要二选一")]
fn outcome_requires_choice(world: &mut LedgerWorld) {
    let outcome = world.dl_last_outcome.as_ref().expect("无提交结果");
    assert!(
        outcome.requires_choice && !outcome.committed,
        "提交结果应为需要二选一，实际 {outcome:?}"
    );
}

#[then(expr = "提交应被拒绝且错误信息包含 {string}")]
fn submit_rejected_with(world: &mut LedgerWorld, needle: String) {
    assert!(world.dl_last_outcome.is_none(), "被拒绝时不应产生提交结果");
    let error = world.last_error.as_ref().expect("应被拒绝但无错误信息");
    assert!(
        error.contains(&needle),
        "错误信息不匹配: 期望包含 '{needle}', 实际 '{error}'"
    );
}

#[then(expr = "指针文件应指向目标目录")]
fn pointer_points_to_target(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.as_ref().unwrap();
    let target = world.dl_target_dir.as_ref().unwrap();
    let configured = data_location::configured_intent(default_dir).expect("指针文件应已配置");
    assert_eq!(configured, *target, "指针文件应指向目标目录");
}

#[then(expr = "指针文件应指向默认数据目录")]
fn pointer_points_to_default(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.as_ref().unwrap();
    let configured = data_location::configured_intent(default_dir).expect("指针文件应已配置");
    assert_eq!(configured, *default_dir, "指针文件应指向默认数据目录");
}

#[then(expr = "指针文件应保持未配置")]
fn pointer_remains_unconfigured(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.as_ref().unwrap();
    assert!(
        data_location::configured_intent(default_dir).is_none(),
        "意图不应落盘（指针文件应保持未配置）"
    );
}

#[then(expr = "信息中的生效目录应为默认数据目录")]
fn info_active_is_default(world: &mut LedgerWorld) {
    let info = world.dl_last_info.as_ref().expect("无信息查询结果");
    let default_dir = world.dl_default_dir.as_ref().unwrap();
    assert_eq!(
        info.active_dir,
        default_dir.to_string_lossy(),
        "信息中的生效目录应为默认数据目录"
    );
}

#[then(expr = "信息应无待重启生效状态且无回退警示")]
fn info_quiescent(world: &mut LedgerWorld) {
    let info = world.dl_last_info.as_ref().expect("无信息查询结果");
    assert!(!info.pending_restart, "不应处于待重启生效状态");
    assert!(
        info.fallback_reason.is_none(),
        "不应有回退警示，实际: {:?}",
        info.fallback_reason
    );
}

#[then(expr = "信息应显示已更改待重启生效，意图目录为目标目录")]
fn info_pending(world: &mut LedgerWorld) {
    let info = world.dl_last_info.as_ref().expect("无信息查询结果");
    let target = world.dl_target_dir.as_ref().unwrap();
    assert!(info.pending_restart, "应处于已更改待重启生效状态");
    assert_eq!(
        info.configured_dir.as_deref(),
        Some(target.to_string_lossy().as_ref()),
        "意图目录应为目标目录"
    );
}

#[then(expr = "信息应携带回退警示包含 {string}")]
fn info_fallback_contains(world: &mut LedgerWorld, needle: String) {
    let info = world.dl_last_info.as_ref().expect("无信息查询结果");
    let reason = info.fallback_reason.as_ref().expect("应有回退警示但为空");
    assert!(
        reason.contains(&needle),
        "回退警示不匹配: 期望包含 '{needle}', 实际 '{reason}'"
    );
}

#[then(expr = "信息应无待重启生效状态")]
fn info_not_pending(world: &mut LedgerWorld) {
    let info = world.dl_last_info.as_ref().expect("无信息查询结果");
    assert!(!info.pending_restart, "不应处于待重启生效状态");
}

#[given(expr = "一个已含 {int} 条交易库的目标目录")]
fn target_dir_with_db_no_pointer(world: &mut LedgerWorld, count: usize) {
    ensure_default_dir(world);
    // 不写指针：仅准备目标现场，供「二选一」场景提交时使用。
    let target = std::env::temp_dir().join(format!("ledger-e2e-dl-adopt-{}", new_uuid()));
    std::fs::create_dir_all(&target).unwrap();
    let mut conn = open_connection(target.join(data_location::DB_FILE_NAME)).unwrap();
    init_db(&mut conn).unwrap();
    seed_db(&conn, "目标现金", count);
    world.dl_target_dir = Some(target);
}
