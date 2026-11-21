use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cucumber::{given, then, when};

use ledger_accounts::{AccountInput, AccountType, create_account};
use ledger_backup::{
    AUTO_BACKUP_PREFIX, AttemptOutcome, BackupKind, SkipReason, backup_db_to,
    expected_schema_version, get_state, list_managed_backups, prune_managed_backups,
    read_backup_kind, read_backup_meta, restore_db_from, set_state,
};
use ledger_categories::{
    CategoryInput, create_category, delete_category as delete_category_domain,
};
use ledger_currencies::ExchangeRateInput;
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::encryption::{DbFileKind, enable_encryption_for_file, probe_file_kind};
use ledger_infra::db::{
    DbState, new_uuid, now_iso, open_connection, open_connection_with_passphrase,
};
use ledger_infra::error::AppError;
use ledger_infra::settings::{self, SettingKey};
use ledger_investment::{
    InstrumentInput, InstrumentType, MarketPriceInput, create_exchange_rate, create_instrument,
    create_market_price,
};
use ledger_item::cost;
use ledger_item::domain::{create_item, delete_item, dispose_item, update_item};
use ledger_item::{ItemDisposeInput, ItemInput};
use ledger_transaction::TransactionBatch;
use ledger_transaction::TransactionInput;
use ledger_transaction::{delete_transaction_internal, update_transaction_internal};

use crate::common::{count_transactions, count_transactions_in_file, seed_account_with_expenses};
use crate::step_inputs::{expense_input, refund_input};
use crate::world::LedgerWorld;

fn temp_path(world: &mut LedgerWorld, name: &str) -> PathBuf {
    let dir = world.scratch_dir("e2e-backup-file");
    dir.join(format!("ledger-e2e-backup-{name}-{}.db", new_uuid()))
}

fn temp_safety_dir(world: &mut LedgerWorld) -> PathBuf {
    world.scratch_dir("e2e-safety")
}

fn extract_zip_db(world: &mut LedgerWorld, backup: &std::path::Path) -> PathBuf {
    let file = std::fs::File::open(backup).expect("打开备份包失败");
    let mut archive = zip::ZipArchive::new(file).expect("不是有效 zip 备份包");
    let out = temp_path(world, "probe-extract.db");
    let mut entry = archive.by_name("ledger.db").expect("备份包缺 ledger.db");
    let mut out_f = std::fs::File::create(&out).unwrap();
    std::io::copy(&mut entry, &mut out_f).unwrap();
    out
}

fn safety_backup_file(safety_dir: &std::path::Path) -> PathBuf {
    let entries: Vec<PathBuf> = std::fs::read_dir(safety_dir)
        .expect("读安全备份目录失败")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    let mut main: Vec<&PathBuf> = entries
        .iter()
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with("restore-safety-") && !n.starts_with("restore-safety-sync-")
            })
        })
        .collect();
    assert_eq!(
        main.len(),
        1,
        "安全备份目录应恰有一份主库安全备份（sync 副本另计）"
    );
    main.pop().unwrap().clone()
}

fn enc_backup_db_path(world: &LedgerWorld) -> PathBuf {
    world
        .boot
        .enc_dir
        .as_ref()
        .expect("当前场景未建立文件库")
        .join("ledger.db")
}

fn record_restore_outcome(
    world: &mut LedgerWorld,
    result: ledger_infra::error::Result<impl Sized>,
) {
    if let Err(e) = result {
        world.last_error = Some(e.to_string());
        world.last_app_error = Some(e);
    }
}

#[when(expr = "备份数据库到临时文件")]
fn backup_to_temp(world: &mut LedgerWorld) {
    let target = temp_path(world, "backup.zip");
    let result = backup_db_to(&world_conn!(world), &target, "0.2.0", BackupKind::Manual);
    assert!(result.is_ok(), "备份失败: {:?}", result.err());
    world.boot.last_backup_path = Some(target);
}

#[when(
    regex = r#"^自动备份数据库到临时目录(?:，来源设备标识 "([^"]+)")?(?:，账本指纹 "([^"]+)")?$"#
)]
fn auto_backup_to_temp(world: &mut LedgerWorld, device_id: String, fingerprint: String) {
    auto_backup_with_world_scope(world, !(device_id.is_empty() && fingerprint.is_empty()));
    if device_id.is_empty() && fingerprint.is_empty() {
        return;
    }
    let path = world
        .boot
        .last_auto_backup_path
        .clone()
        .expect("尚未自动备份");
    let mut extra = serde_json::Map::new();
    if !device_id.is_empty() {
        extra.insert("device_id".into(), serde_json::json!(device_id));
    }
    if !fingerprint.is_empty() {
        extra.insert("fingerprint".into(), serde_json::json!(fingerprint));
    }
    patch_backup_meta(&path, extra);
    let path = if fingerprint.is_empty() {
        path
    } else {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let (head, _) = name
            .split_once(".db.zip")
            .and_then(|(h, _)| h.rsplit_once('-'))
            .expect("受管命名应含标识段");
        let renamed = path.with_file_name(format!("{head}-{fingerprint}.db.zip"));
        std::fs::rename(&path, &renamed).expect("改写产物名失败");
        world.boot.last_auto_backup_path = Some(renamed.clone());
        renamed
    };
    world.boot.last_backup_path = Some(path);
}

fn patch_backup_meta(path: &std::path::Path, extra: serde_json::Map<String, serde_json::Value>) {
    tauri_app_lib::test_support::merge_extra_backup_meta(path, &serde_json::Value::Object(extra));
}

#[when(expr = "把最近一份自动备份产物复制到设备子目录 {string} 并改写来源设备标识")]
fn clone_last_auto_backup_as_other_device(world: &mut LedgerWorld, device: String) {
    let dir = world
        .boot
        .auto_backup_dir
        .as_ref()
        .expect("尚未自动备份（无受管目录）")
        .clone();
    let path = world
        .boot
        .last_auto_backup_path
        .as_ref()
        .expect("尚未自动备份")
        .clone();
    let target_dir = dir.join(&device);
    std::fs::create_dir_all(&target_dir).expect("建设备子目录失败");
    let target = target_dir.join(path.file_name().expect("产物名应含文件名"));
    std::fs::copy(&path, &target).expect("复制产物失败");
    let mut extra = serde_json::Map::new();
    extra.insert("device_id".into(), serde_json::json!(device));
    patch_backup_meta(&target, extra);
}

#[when(expr = "以当前账本作用域自动备份数据库到临时目录")]
fn auto_backup_with_active_book_scope(world: &mut LedgerWorld) {
    auto_backup_with_world_scope(world, false);
}

#[then(expr = "备份产物文件名应携带当前账本指纹")]
fn backup_file_name_carries_book_fingerprint(world: &mut LedgerWorld) {
    let fingerprint = active_book_fingerprint(world);
    let path = world
        .boot
        .last_auto_backup_path
        .as_ref()
        .expect("尚未自动备份");
    let name = path.file_name().unwrap().to_string_lossy();
    assert!(
        name.ends_with(&format!("-{fingerprint}.db.zip")),
        "产物名应携带当前账本指纹（…-{fingerprint}.db.zip），实际 {name}"
    );
}

fn active_book_fingerprint(world: &mut LedgerWorld) -> String {
    active_book_scope(world)
        .expect("当前账本应有指纹（非空库）")
        .fingerprint
}

fn active_book_scope(world: &mut LedgerWorld) -> Option<ledger_backup::BackupScope> {
    if let Some(state) = world.boot.dl_conn.as_ref() {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        ledger_backup::BackupScope::of_conn(&conn).expect("作用域可构造")
    } else {
        ledger_backup::BackupScope::of_conn(&world_conn!(world)).expect("作用域可构造")
    }
}

fn auto_backup_with_world_scope(world: &mut LedgerWorld, next_day: bool) {
    let dir = match world.boot.auto_backup_dir.clone() {
        Some(dir) => dir,
        None => world.scratch_dir("e2e-auto-backup"),
    };
    world.boot.auto_backup_dir = Some(dir.clone());
    let now = if next_day {
        chrono::Utc::now() + chrono::Duration::days(1)
    } else {
        chrono::Utc::now()
    };
    let outcome = if let Some(state) = world.boot.dl_conn.as_ref() {
        run_due_on(state, &dir, now)
    } else {
        run_due_on_ref(&world_conn!(world), &dir, now)
    };
    assert!(
        matches!(outcome, AttemptOutcome::Performed { .. }),
        "自动备份应执行，实际 {outcome:?}"
    );
    if let AttemptOutcome::Performed { path } = outcome {
        world.boot.last_auto_backup_path = Some(PathBuf::from(path));
    }
}

fn run_due_on(
    state: &ledger_infra::db::DbState,
    dir: &std::path::Path,
    now: chrono::DateTime<chrono::Utc>,
) -> ledger_backup::AttemptOutcome {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    ledger_backup::run_due_backup(&conn, Some(dir.to_str().unwrap()), "0.2.0", now)
}

fn run_due_on_ref(
    conn: &rusqlite::Connection,
    dir: &std::path::Path,
    now: chrono::DateTime<chrono::Utc>,
) -> ledger_backup::AttemptOutcome {
    ledger_backup::run_due_backup(conn, Some(dir.to_str().unwrap()), "0.2.0", now)
}

#[when(expr = "删除全部交易")]
fn delete_all_txns(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    let ids: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT id FROM transactions WHERE is_deleted=0")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    for id in ids {
        delete_transaction_internal(&conn, &id).expect("删除交易失败");
    }
}

#[when(expr = "从备份恢复到临时数据库")]
fn restore_to_temp(world: &mut LedgerWorld) {
    let backup = world.boot.last_backup_path.clone().expect("尚未备份");
    restore_path_to_temp(world, backup);
}

#[when(expr = "从最近一份自动备份产物恢复到临时数据库")]
fn restore_last_auto_product_to_temp(world: &mut LedgerWorld) {
    let backup = world
        .boot
        .last_auto_backup_path
        .clone()
        .expect("尚未自动备份");
    restore_path_to_temp(world, backup);
}

fn restore_path_to_temp(world: &mut LedgerWorld, backup: PathBuf) {
    let db_path = temp_path(world, "restored.db");
    let safety_dir = temp_safety_dir(world);
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected, None);
    assert!(result.is_ok(), "恢复失败: {:?}", result.err());
    world.boot.restored_db_path = Some(db_path);
}

#[when(expr = "以主口令 {string} 从备份恢复到临时数据库")]
fn restore_to_temp_with_passphrase(world: &mut LedgerWorld, passphrase: String) {
    let backup = world.boot.last_backup_path.clone().expect("尚未备份");
    let db_path = temp_path(world, "restored-enc.db");
    let safety_dir = temp_safety_dir(world);
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected, Some(&passphrase));
    assert!(result.is_ok(), "恢复失败: {:?}", result.err());
    world.boot.restored_db_path = Some(db_path);
}

#[when(expr = "记录当前库文件字节")]
fn when_record_db_bytes(world: &mut LedgerWorld) {
    world.boot.enc_db_bytes = Some(std::fs::read(enc_backup_db_path(world)).unwrap());
}

#[when(expr = "尝试不带主口令从备份恢复到临时数据库")]
fn try_restore_without_passphrase(world: &mut LedgerWorld) {
    let backup = world.boot.last_backup_path.clone().expect("尚未备份");
    let db_path = temp_path(world, "restored-no-pass.db");
    let safety_dir = temp_safety_dir(world);
    let expected = expected_schema_version().unwrap();
    record_restore_outcome(
        world,
        restore_db_from(&backup, &db_path, &safety_dir, expected, None),
    );
}

#[when(expr = "尝试以主口令 {string} 从备份恢复到临时数据库")]
fn try_restore_with_wrong_passphrase(world: &mut LedgerWorld, passphrase: String) {
    let backup = world.boot.last_backup_path.clone().expect("尚未备份");
    let db_path = temp_path(world, "restored-wrong-pass.db");
    let safety_dir = temp_safety_dir(world);
    let expected = expected_schema_version().unwrap();
    record_restore_outcome(
        world,
        restore_db_from(&backup, &db_path, &safety_dir, expected, Some(&passphrase)),
    );
}

#[when(expr = "以主口令 {string} 从备份恢复到当前加密库")]
fn restore_onto_encrypted_current(world: &mut LedgerWorld, passphrase: String) {
    let backup = world.boot.last_backup_path.clone().expect("尚未备份");
    let db_path = enc_backup_db_path(world);
    let safety_dir = temp_safety_dir(world);
    world.boot.restore_safety_dir = Some(safety_dir.clone());
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected, Some(&passphrase));
    assert!(result.is_ok(), "恢复失败: {:?}", result.err());
}

#[when(expr = "尝试从更高 schema 版本恢复")]
fn try_newer_restore(world: &mut LedgerWorld) {
    let newer = temp_path(world, "newer.db");
    {
        let conn = open_connection(&newer).unwrap();
        conn.execute_batch("PRAGMA user_version = 999").unwrap();
    }
    let db_path = temp_path(world, "target.db");
    let safety_dir = temp_safety_dir(world);
    let expected = expected_schema_version().unwrap();
    world.last_error = match restore_db_from(&newer, &db_path, &safety_dir, expected, None) {
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

#[then(expr = "备份文件应存在")]
fn backup_exists(world: &mut LedgerWorld) {
    let p = world.boot.last_backup_path.as_ref().expect("尚未备份");
    assert!(p.exists(), "备份文件不存在: {}", p.display());
}

#[then(expr = "备份包应包含 {string} 与 {string}")]
fn backup_contains(world: &mut LedgerWorld, a: String, b: String) {
    let p = world.boot.last_backup_path.as_ref().expect("尚未备份");
    let file = std::fs::File::open(p).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().unwrap().to_string())
        .collect();
    assert!(names.contains(&a), "缺少条目 {a}: {names:?}");
    assert!(names.contains(&b), "缺少条目 {b}: {names:?}");
}

#[then(expr = "备份包内的数据库应包含 {int} 条交易")]
fn backup_db_has_txns(world: &mut LedgerWorld, expected: i64) {
    let p = world.boot.last_backup_path.clone().expect("尚未备份");
    let file = std::fs::File::open(&p).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut db_entry = archive.by_name("ledger.db").unwrap();
    let out = temp_path(world, "extract.db");
    let mut out_f = std::fs::File::create(&out).unwrap();
    std::io::copy(&mut db_entry, &mut out_f).unwrap();
    drop(out_f);
    let conn = open_connection(&out).unwrap();
    let count = count_transactions(&conn);
    assert_eq!(count, expected, "备份包内交易数量不匹配");
}

#[then(expr = "恢复的数据库应包含 {int} 条交易")]
fn restored_has_txns(world: &mut LedgerWorld, expected: i64) {
    let p = world.boot.restored_db_path.as_ref().expect("尚未恢复");
    let count = count_transactions_in_file(p, None);
    assert_eq!(count, expected, "恢复出的交易数量不匹配");
}

#[then(expr = "备份元数据来源应为 {string}")]
fn backup_meta_kind_manual(world: &mut LedgerWorld, expected: String) {
    let p = world.boot.last_backup_path.as_ref().expect("尚未手动备份");
    assert_eq!(
        read_backup_kind(p).unwrap().to_string(),
        expected,
        "手动备份元数据来源不匹配"
    );
}

#[then(expr = "自动备份元数据来源应为 {string}")]
fn backup_meta_kind_auto(world: &mut LedgerWorld, expected: String) {
    let p = world
        .boot
        .last_auto_backup_path
        .as_ref()
        .expect("尚未自动备份");
    assert_eq!(
        read_backup_kind(p).unwrap().to_string(),
        expected,
        "自动备份元数据来源不匹配"
    );
}

#[then(expr = "自动备份脏标记应为真")]
fn auto_backup_dirty(world: &mut LedgerWorld) {
    let state = get_state(&world_conn!(world)).unwrap();
    assert!(state.dirty, "业务写库成功后脏标记应为真");
}

#[then(expr = "自动备份脏标记应为假")]
fn auto_backup_clean(world: &mut LedgerWorld) {
    let state = get_state(&world_conn!(world)).unwrap();
    assert!(!state.dirty, "未发生业务写库时脏标记应为默认假");
}

#[when(expr = "删除最近创建的交易")]
fn delete_last_transaction(world: &mut LedgerWorld) {
    let id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有可删除的交易");
    world_write!(world, |conn| delete_transaction_internal(conn, &id)).unwrap();
}

#[when(expr = "距离上次自动备份已过一天")]
fn fast_forward_backup_due(world: &mut LedgerWorld) {
    use chrono::TimeZone;
    let conn = world_conn!(world);
    let mut state = get_state(&conn).unwrap();
    if state.last_backup_at.is_some() {
        let yesterday_noon = chrono::Local
            .from_local_datetime(
                &(chrono::Local::now().date_naive() - chrono::Days::new(1))
                    .and_hms_opt(12, 0, 0)
                    .expect("合法时刻"),
            )
            .earliest()
            .expect("本地昨天正午应可解析");
        state.last_backup_at = Some(ledger_infra::db::iso_at(
            yesterday_noon.with_timezone(&chrono::Utc),
        ));
        set_state(&conn, &state).expect("回拨备份锚点");
    }
}

#[when(expr = "再次到期触发自动备份因日界门静默跳过")]
fn due_trigger_skipped_by_day_gate(world: &mut LedgerWorld) {
    let anchor_before = get_state(&world_conn!(world)).unwrap().last_backup_at;
    let outcome = ledger_backup::run_due_backup(
        &world_conn!(world),
        Some(
            world
                .boot
                .auto_backup_dir
                .as_ref()
                .expect("尚未自动备份")
                .to_str()
                .unwrap(),
        ),
        "0.2.0",
        chrono::Utc::now(),
    );
    assert_eq!(
        outcome,
        AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday),
        "同日二次到期触发应静默跳过"
    );
    assert_eq!(
        get_state(&world_conn!(world)).unwrap().last_backup_at,
        anchor_before,
        "跳过不应前移锚点"
    );
}

#[when(expr = "退出兜底因日界门静默跳过")]
fn exit_fallback_skipped_by_day_gate(world: &mut LedgerWorld) {
    let anchor_before = get_state(&world_conn!(world)).unwrap().last_backup_at;
    let outcome = ledger_backup::run_exit_backup(
        &world_conn!(world),
        Some(
            world
                .boot
                .auto_backup_dir
                .as_ref()
                .expect("尚未自动备份")
                .to_str()
                .unwrap(),
        ),
        "0.2.0",
        chrono::Utc::now(),
    );
    assert_eq!(
        outcome,
        AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday),
        "同日退出兜底应静默跳过"
    );
    assert_eq!(
        get_state(&world_conn!(world)).unwrap().last_backup_at,
        anchor_before,
        "跳过不应前移锚点"
    );
}

#[when(expr = "跨日后触发自动备份数据库到临时目录")]
fn auto_backup_next_day_to_temp(world: &mut LedgerWorld) {
    let dir = world.boot.auto_backup_dir.clone().expect("尚未自动备份");
    let outcome = ledger_backup::run_due_backup(
        &world_conn!(world),
        Some(dir.to_str().unwrap()),
        "0.2.0",
        chrono::Utc::now() + chrono::Duration::days(1),
    );
    assert!(
        matches!(outcome, AttemptOutcome::Performed { .. }),
        "跨本地日后有变动应恢复备份，实际 {outcome:?}"
    );
    if let AttemptOutcome::Performed { path } = outcome {
        world.boot.last_auto_backup_path = Some(PathBuf::from(path));
    }
}

#[when(expr = "首次兜底因日界门静默跳过")]
fn first_fallback_skipped_by_day_gate(world: &mut LedgerWorld) {
    let anchor_before = get_state(&world_conn!(world)).unwrap().last_backup_at;
    let outcome = ledger_backup::run_first_backup(
        &world_conn!(world),
        Some(
            world
                .boot
                .auto_backup_dir
                .as_ref()
                .expect("尚未自动备份")
                .to_str()
                .unwrap(),
        ),
        "0.2.0",
        chrono::Utc::now(),
    );
    assert_eq!(
        outcome,
        AttemptOutcome::Skipped(SkipReason::AlreadyBackedUpToday),
        "同日首次兜底应静默跳过"
    );
    assert_eq!(
        get_state(&world_conn!(world)).unwrap().last_backup_at,
        anchor_before,
        "跳过不应前移锚点"
    );
}

#[when(expr = "清空自动备份目录")]
fn clear_auto_backup_dir(world: &mut LedgerWorld) {
    let dir = world.boot.auto_backup_dir.as_ref().expect("尚未自动备份");
    for entry in std::fs::read_dir(dir).expect("列目录") {
        let path = entry.expect("读目录项").path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path).expect("删除设备子目录");
        } else {
            std::fs::remove_file(&path).expect("删除产物");
        }
    }
}

#[then(expr = "备份目录内自动备份产物数量应为 {int}")]
fn auto_backup_product_count(world: &mut LedgerWorld, expected: i64) {
    let dir = world.boot.auto_backup_dir.as_ref().expect("尚未自动备份");
    let mut count = 0;
    let mut stack = vec![dir.clone()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).expect("列目录") {
            let entry = entry.expect("读目录项");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if entry
                .file_name()
                .to_string_lossy()
                .starts_with(AUTO_BACKUP_PREFIX)
            {
                count += 1;
            }
        }
    }
    assert_eq!(count, expected, "自动备份产物数量不匹配");
}

#[when(expr = "写入一项设置")]
fn write_a_setting(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    settings::set(&conn, SettingKey::AutoBackupEnabled, &false).expect("写入设置");
}

#[when(expr = "创建账户 {string} 类型 {string} 币种 {string}")]
fn create_account_via_entry(world: &mut LedgerWorld, name: String, kind: String, currency: String) {
    let input = AccountInput {
        name,
        kind: kind.parse::<AccountType>().expect("非法账户类型"),
        currency_code: currency,
        initial_balance_cents: None,
        credit_limit_cents: None,
        statement_day: None,
        due_day: None,
    };
    world_write!(world, |conn| create_account(conn, input)).expect("创建账户失败");
}

#[when(expr = "创建分类 {string} 类型 {string}")]
fn create_category_via_entry(world: &mut LedgerWorld, name: String, kind: String) {
    let input = CategoryInput {
        name,
        kind,
        parent_id: None,
        icon: None,
    };
    world_write!(world, |conn| create_category(conn, input)).expect("创建分类失败");
}

#[when(expr = "删除分类 {string}")]
fn delete_category_via_entry(world: &mut LedgerWorld, name: String) {
    let id: String = {
        let conn = world_conn!(world);
        conn.query_row(
            "SELECT id FROM categories WHERE name=?1 AND is_deleted=0 LIMIT 1",
            rusqlite::params![name],
            |r| r.get(0),
        )
        .expect("分类不存在")
    };
    world_write!(world, |conn| delete_category_domain(conn, &id)).expect("删除分类失败");
}

#[when(expr = "尝试把最近创建的交易修改为非法金额")]
fn update_last_transaction_invalid_amount(world: &mut LedgerWorld) {
    let id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有可修改的交易");
    let input = expense_input(0, "acc-any", "2026-02-01");
    world.last_error =
        match world_write!(world, |conn| update_transaction_internal(conn, &id, input)) {
            Ok(_) => Some(String::from("预期失败但成功了")),
            Err(e) => Some(e.to_string()),
        };
}

#[when(expr = "批量导入 {int} 笔支出各 {int} 分到账户 {string}")]
fn batch_import_expenses_via_entry(
    world: &mut LedgerWorld,
    count: usize,
    cents: i64,
    account: String,
) {
    let account_id = world.account_id(&account);
    let inputs: Vec<TransactionInput> = (0..count)
        .map(|i| expense_input(cents, &account_id, &format!("2026-02-{:02}", i + 1)))
        .collect();
    let results = world_write!(world, |conn| TransactionBatch::run(conn, inputs, true))
        .expect("批量导入失败")
        .results;
    assert!(
        results.iter().all(|r| r.success),
        "批量导入应整批成功: {results:?}"
    );
}

#[when(expr = "批量导入两笔交易但退款行引用不存在的原支出交易")]
fn batch_import_rollback_via_entry(world: &mut LedgerWorld) {
    let account_id = world.account_id("现金");
    let expense = expense_input(1500, &account_id, "2026-02-01");
    let refund = refund_input(500, &account_id, "tx-no-such", "2026-02-01");
    world.last_error = match world_write!(world, |conn| TransactionBatch::run(
        conn,
        vec![expense, refund],
        true
    )) {
        Ok(_) => Some(String::from("预期失败但成功了")),
        Err(e) => Some(e.to_string()),
    };
}

#[when(expr = "创建物品 {string} 关联最近创建的购买交易")]
fn create_item_via_entry(world: &mut LedgerWorld, name: String) {
    let tx_id = world
        .txn
        .last_transaction_id
        .clone()
        .expect("没有可关联的购买交易");
    let input = ItemInput {
        name,
        purchase_date: "1970-01-01".into(),
        total_cost_cents: 1,
        currency_code: "CNY".into(),
        note: None,
        purchase_transaction_id: Some(tx_id),
    };
    let id =
        world_write!(world, |conn| create_item(conn, input, &mut || {})).expect("创建物品失败");
    world.item.last_item_id = Some(id);
}

#[when(expr = "修改最近创建的物品备注为 {string}")]
fn update_last_item_note_via_entry(world: &mut LedgerWorld, note: String) {
    let id = world.item.last_item_id.clone().expect("没有已创建的物品");
    let (name, purchase_date, total_cost_cents, currency_code): (String, String, i64, String) = {
        let conn = world_conn!(world);
        conn.query_row(
            "SELECT name, purchase_date, total_cost_cents, currency_code FROM items \
             WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("读物品现值失败")
    };
    let input = ItemInput {
        name,
        purchase_date,
        total_cost_cents,
        currency_code,
        note: if note.is_empty() { None } else { Some(note) },
        purchase_transaction_id: None,
    };
    world_write!(world, |conn| update_item(conn, &id, input, &mut || {})).expect("修改物品失败");
}

#[when(expr = "今天处置最近创建的物品")]
fn dispose_last_item_today_via_entry(world: &mut LedgerWorld) {
    let id = world.item.last_item_id.clone().expect("没有已创建的物品");
    let input = ItemDisposeInput {
        disposal_date: cost::today().format("%Y-%m-%d").to_string(),
        residual_value_cents: None,
    };
    world_write!(world, |conn| dispose_item(conn, &id, input, &mut || {})).expect("处置物品失败");
}

#[when(expr = "软删除最近创建的物品")]
fn delete_last_item_via_entry(world: &mut LedgerWorld) {
    let id = world.item.last_item_id.clone().expect("没有已创建的物品");
    world_write!(world, |conn| delete_item(conn, &id, &mut || {})).expect("软删除物品失败");
}

#[when(expr = "写入汇率 {string} 兑 {string} 为 {float}")]
fn write_exchange_rate_via_entry(world: &mut LedgerWorld, base: String, quote: String, rate: f64) {
    let input = ExchangeRateInput {
        base_code: base,
        quote_code: quote,
        rate,
        priced_at: now_iso(),
        source: None,
    };
    world_write!(world, |conn| create_exchange_rate(conn, input)).expect("写入汇率失败");
}

fn create_instrument_entry(
    world: &mut LedgerWorld,
    symbol: String,
    name: String,
    currency: String,
) {
    let input = InstrumentInput {
        symbol,
        kind: InstrumentType::Stock,
        name: Some(name),
        currency_code: currency,
        market: None,
    };
    world_write!(world, |conn| create_instrument(conn, input)).expect("新建标的失败");
}

#[when(expr = "新建标的 {string} 名称 {string} 币种 {string}")]
fn create_instrument_via_entry(
    world: &mut LedgerWorld,
    symbol: String,
    name: String,
    currency: String,
) {
    create_instrument_entry(world, symbol, name, currency);
}

#[when(expr = "再次新建标的 {string} 名称 {string} 币种 {string}")]
fn recreate_instrument_via_entry(
    world: &mut LedgerWorld,
    symbol: String,
    name: String,
    currency: String,
) {
    create_instrument_entry(world, symbol, name, currency);
}

#[when(expr = "写入标的 {string} 现价 {int} 币种 {string}")]
fn write_market_price_via_entry(
    world: &mut LedgerWorld,
    symbol: String,
    price: i64,
    currency: String,
) {
    let instrument_id: String = {
        let conn = world_conn!(world);
        conn.query_row(
            "SELECT id FROM instruments WHERE symbol=?1 LIMIT 1",
            rusqlite::params![symbol],
            |r| r.get(0),
        )
        .expect("标的不存在")
    };
    let input = MarketPriceInput {
        instrument_id,
        price_cents: price,
        currency_code: currency,
        priced_at: now_iso(),
        source: None,
    };
    world_write!(world, |conn| create_market_price(conn, input)).expect("写入行情失败");
}

#[then(expr = "恢复的数据库自动备份状态应为「未脏且已重新计时」")]
fn restored_auto_backup_state_reset(world: &mut LedgerWorld) {
    let p = world.boot.restored_db_path.as_ref().expect("尚未恢复");
    let conn = open_connection(p).unwrap();
    let state = get_state(&conn).unwrap();
    assert!(!state.dirty, "恢复后脏标记应被重置为假");
    assert!(
        state.last_backup_at.is_some(),
        "恢复后上次备份锚点应重新计时"
    );
}

fn seed_account_and_transactions(
    conn: &rusqlite::Connection,
    account: &str,
    note_prefix: &str,
    count: usize,
) -> String {
    seed_account_with_expenses(conn, account, note_prefix, count, 1500, "2026-02-01")
}

#[given(expr = "以主口令 {string} 加密的文件库中已有账户 {string} 与 {int} 条交易")]
fn given_encrypted_file_lib(
    world: &mut LedgerWorld,
    passphrase: String,
    account: String,
    count: usize,
) {
    let dir = world.scratch_dir("e2e-bak-enc");
    let db_path = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db_path).unwrap();
        ledger_infra::db::init_db(&mut conn).unwrap();
        let id = seed_account_and_transactions(&conn, &account, "加密库种子交易", count);
        world.account_name_to_id.insert(account, id);
    }
    enable_encryption_for_file(&db_path, &passphrase).unwrap();
    std::fs::remove_file(db_path.with_extension("db.bak")).unwrap();
    world.boot.enc_dir = Some(dir);
    let conn = open_connection_with_passphrase(&db_path, &passphrase).unwrap();
    let read_conn =
        ledger_infra::db::open_connection_readonly_with_passphrase(&db_path, &passphrase).unwrap();
    world.db = DbState {
        conn: Arc::new(Mutex::new(conn)),
        read_conn: Arc::new(Mutex::new(read_conn)),
    };
}

#[given(expr = "一份含 {int} 条交易的明文库备份")]
fn given_plaintext_backup(world: &mut LedgerWorld, count: usize) {
    let dir = world.scratch_dir("e2e-bak-plain");
    let db_path = dir.join(DB_FILE_NAME);
    {
        let mut conn = open_connection(&db_path).unwrap();
        ledger_infra::db::init_db(&mut conn).unwrap();
        seed_account_and_transactions(&conn, "现金", "明文库种子交易", count);
    }
    let target = dir.join("plaintext-backup.db.zip");
    backup_db_to(
        &open_connection(&db_path).unwrap(),
        &target,
        "0.2.0",
        BackupKind::Manual,
    )
    .expect("明文库备份失败");
    world.boot.last_backup_path = Some(target);
}

#[when(expr = "备份目录中写入一份缺加密标记的旧版明文备份")]
fn write_legacy_plaintext_backup(world: &mut LedgerWorld) {
    let dir = world
        .boot
        .auto_backup_dir
        .clone()
        .expect("尚未自动备份（无受管目录）");
    let plain_db = world.scratch_dir("e2e-backup-legacy").join(DB_FILE_NAME);
    {
        let mut conn = open_connection(&plain_db).unwrap();
        ledger_infra::db::init_db(&mut conn).unwrap();
    }
    let target = dir.join("ledger-backup-20250101-010101.db.zip");
    let file = std::fs::File::create(&target).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("ledger.db", options).unwrap();
    zip.write_all(std::fs::read(&plain_db).unwrap().as_slice())
        .unwrap();
    zip.start_file("backup.json", options).unwrap();
    zip.write_all(
        format!(
            "{{\n  \"created_at\": \"2025-01-01T01:01:01Z\",\n  \"app_version\": \"0.2.0\",\n  \"schema_version\": {},\n  \"kind\": \"manual\"\n}}",
            expected_schema_version().unwrap()
        )
        .as_bytes(),
    )
    .unwrap();
    zip.finish().unwrap();
    world.boot.last_backup_path = Some(target);
}

#[then(expr = "备份包内的数据库应探测为密文")]
fn backup_zip_db_is_encrypted(world: &mut LedgerWorld) {
    let p = world.boot.last_backup_path.clone().expect("尚未备份");
    let extracted = extract_zip_db(world, &p);
    assert_eq!(
        probe_file_kind(&extracted).unwrap(),
        DbFileKind::Encrypted,
        "备份包内的数据库应为密文库（VACUUM INTO 继承源库加密与密钥）"
    );
}

#[then(expr = "自动备份产物应探测为密文")]
fn auto_backup_product_is_encrypted(world: &mut LedgerWorld) {
    let p = world
        .boot
        .last_auto_backup_path
        .clone()
        .expect("尚未自动备份");
    let extracted = extract_zip_db(world, &p);
    assert_eq!(
        probe_file_kind(&extracted).unwrap(),
        DbFileKind::Encrypted,
        "自动备份产物应为密文库"
    );
}

#[then(expr = "备份元数据应标记为已加密")]
fn backup_meta_encrypted(world: &mut LedgerWorld) {
    let p = world.boot.last_backup_path.as_ref().expect("尚未备份");
    assert!(
        read_backup_meta(p).unwrap().encrypted,
        "备份元数据应含 encrypted 标记"
    );
}

#[then(expr = "自动备份元数据应标记为已加密")]
fn auto_backup_meta_encrypted(world: &mut LedgerWorld) {
    let p = world
        .boot
        .last_auto_backup_path
        .as_ref()
        .expect("尚未自动备份");
    assert!(
        read_backup_meta(p).unwrap().encrypted,
        "自动备份元数据应含 encrypted 标记"
    );
}

#[then(expr = "受管备份列表应显示 {int} 份密文备份与 {int} 份明文备份")]
fn managed_list_encrypted_flags(world: &mut LedgerWorld, encrypted: usize, plaintext: usize) {
    let dir = world.boot.auto_backup_dir.as_ref().expect("无受管备份目录");
    let files = list_managed_backups(dir, None).expect("列受管备份失败");
    assert_eq!(
        files.iter().filter(|f| f.encrypted).count(),
        encrypted,
        "密文备份数不匹配: {files:?}"
    );
    assert_eq!(
        files.iter().filter(|f| !f.encrypted).count(),
        plaintext,
        "明文备份数不匹配: {files:?}"
    );
}

fn scoped_managed_list(world: &mut LedgerWorld) -> Vec<ledger_backup::BackupFileInfo> {
    let dir = world
        .boot
        .auto_backup_dir
        .as_ref()
        .expect("无受管备份目录")
        .clone();
    let scope = active_book_scope(world).expect("当前账本应有指纹");
    list_managed_backups(&dir, Some(&scope)).expect("列受管备份失败")
}

#[when(expr = "按当前账本作用域清理受管备份到 {int} 份")]
fn prune_scoped_to_limit(world: &mut LedgerWorld, keep: usize) {
    let dir = world
        .boot
        .auto_backup_dir
        .as_ref()
        .expect("无受管备份目录")
        .clone();
    let scope = active_book_scope(world).expect("当前账本应有指纹");
    let r = prune_managed_backups(&dir, keep, Some(&scope)).expect("清理失败");
    world.boot.last_prune = Some(r);
}

#[then(expr = "清理应已删除 {int} 份产物")]
fn prune_deleted_count(world: &mut LedgerWorld, expected: usize) {
    let r = world.boot.last_prune.as_ref().expect("尚未清理");
    assert_eq!(r.deleted.len(), expected, "清理删除数量不匹配: {:?}", r);
}

#[then(expr = "受管备份列表应显示 {int} 份备份")]
fn managed_list_scoped_count(world: &mut LedgerWorld, expected: usize) {
    let files = scoped_managed_list(world);
    let names: Vec<String> = files.iter().map(|f| f.file_name.clone()).collect();
    assert_eq!(names.len(), expected, "产物清单: {names:?}");
}

#[then(expr = "备份列表应含来源设备 {string} 的产物")]
fn managed_list_contains_device(world: &mut LedgerWorld, device: String) {
    let files = scoped_managed_list(world);
    let names: Vec<String> = files.iter().map(|f| f.file_name.clone()).collect();
    assert!(
        files
            .iter()
            .any(|f| f.source_device_id.as_deref() == Some(device.as_str())),
        "列表应含来源设备 {device} 的产物: {names:?}"
    );
}

#[then(expr = "备份列表中来源设备 {string} 的产物应有 {int} 份")]
fn managed_list_device_count(world: &mut LedgerWorld, device: String, expected: usize) {
    let files = scoped_managed_list(world);
    let names: Vec<String> = files.iter().map(|f| f.file_name.clone()).collect();
    let count = files
        .iter()
        .filter(|f| f.source_device_id.as_deref() == Some(device.as_str()))
        .count();
    assert_eq!(
        count, expected,
        "来源设备 {device} 的产物数量不匹配: {names:?}"
    );
}

#[then(expr = "备份列表不应有未知来源产物")]
fn managed_list_has_no_unknown_source(world: &mut LedgerWorld) {
    let files = scoped_managed_list(world);
    let names: Vec<String> = files.iter().map(|f| f.file_name.clone()).collect();
    assert!(
        files.iter().all(|f| !f.source_unknown),
        "列表不应有未知来源产物: {names:?}"
    );
}

#[then(expr = "恢复的数据库应探测为密文库")]
fn restored_db_is_encrypted(world: &mut LedgerWorld) {
    let p = world.boot.restored_db_path.as_ref().expect("尚未恢复");
    assert_eq!(
        probe_file_kind(p).unwrap(),
        DbFileKind::Encrypted,
        "恢复出的库应为密文库"
    );
}

#[then(expr = "恢复的数据库应探测为明文库")]
fn restored_db_is_plaintext(world: &mut LedgerWorld) {
    let p = world.boot.restored_db_path.as_ref().expect("尚未恢复");
    assert_eq!(
        probe_file_kind(p).unwrap(),
        DbFileKind::Plaintext,
        "恢复出的库应为明文库"
    );
}

#[then(expr = "凭主口令 {string} 打开恢复的数据库应包含 {int} 条交易")]
fn restored_db_count_with_passphrase(world: &mut LedgerWorld, passphrase: String, count: i64) {
    let p = world.boot.restored_db_path.as_ref().expect("尚未恢复");
    let n = count_transactions_in_file(p, Some(&passphrase));
    assert_eq!(n, count, "凭主口令打开的恢复库交易数不匹配");
}

#[then(expr = "恢复应失败且错误码为 {string}")]
fn restore_failed_with_code(world: &mut LedgerWorld, code: String) {
    let error = world.last_app_error.as_ref().expect("预期恢复失败");
    match error {
        AppError::Coded { code: actual, .. } => {
            assert_eq!(actual, &code, "错误码不匹配，实际: {error}")
        }
        other => panic!("预期码化错误 {code}，实际: {other}"),
    }
}

#[then(expr = "恢复安全备份应探测为密文")]
fn safety_backup_is_encrypted(world: &mut LedgerWorld) {
    let dir = world
        .boot
        .restore_safety_dir
        .as_ref()
        .expect("尚未触发安全备份");
    let safety = safety_backup_file(dir);
    assert_eq!(
        probe_file_kind(&safety).unwrap(),
        DbFileKind::Encrypted,
        "加密库上的恢复安全备份应为密文（文件拷贝继承密文）"
    );
}

#[then(expr = "用恢复安全备份回滚后凭主口令 {string} 打开应包含 {int} 条交易")]
fn rollback_from_safety_backup(world: &mut LedgerWorld, passphrase: String, count: i64) {
    let safety_dir = world
        .boot
        .restore_safety_dir
        .as_ref()
        .expect("尚未触发安全备份");
    let safety = safety_backup_file(safety_dir);
    let db_path = enc_backup_db_path(world);
    std::fs::copy(&safety, &db_path).expect("回滚拷贝失败");
    let n = count_transactions_in_file(&db_path, Some(&passphrase));
    assert_eq!(n, count, "回滚后的库交易数不匹配");
}
