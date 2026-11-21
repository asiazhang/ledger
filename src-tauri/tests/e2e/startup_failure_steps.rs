use cucumber::{given, then, when};

use std::path::PathBuf;

use rusqlite::Connection;

use ledger_backup::{expected_schema_version, restore_db_from};
use ledger_infra::db::data_location::{self, DB_FILE_NAME, effective_db_dir};
use ledger_infra::db::encryption::{
    SQLITE_HEADER_MAGIC, enable_encryption_for_file, normalize_plaintext_db_file,
};
use ledger_infra::db::{boot, init_db, open_connection, open_db_in};

use crate::common::seed_account_with_expenses;
use crate::world::{LedgerWorld, StartupTakeover};

#[given(expr = "默认数据目录中存在一个头部完好但内容损坏的明文库")]
fn default_dir_with_corrupt_plaintext_db(world: &mut LedgerWorld) {
    let dir = world.scratch_dir("e2e-sf-plain");
    let mut bytes = SQLITE_HEADER_MAGIC.to_vec();
    bytes.extend_from_slice(b"corrupted body: not a real database page");
    std::fs::write(dir.join(DB_FILE_NAME), bytes).unwrap();
    world.boot.dl_default_dir = Some(dir);
}

#[given(expr = "目标目录中存在一个损坏的库文件")]
fn target_dir_with_corrupt_db(world: &mut LedgerWorld) {
    let target = world.boot.dl_target_dir.clone().unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join(DB_FILE_NAME), b"not a database at all").unwrap();
}

fn seed_vault_db(conn: &Connection, count: usize) {
    seed_account_with_expenses(
        conn,
        "现金",
        "解锁屏恢复种子交易",
        count,
        1000,
        "2026-03-01",
    );
}

#[given(expr = "默认数据目录中有一个凭主口令 {string} 加密且含 {int} 条交易的真密文库")]
fn default_dir_with_encrypted_vault(world: &mut LedgerWorld, passphrase: String, count: usize) {
    let dir = world.scratch_dir("e2e-sf-vault");
    let path = dir.join(DB_FILE_NAME);
    {
        let mut conn = open_connection(&path).unwrap();
        init_db(&mut conn).unwrap();
        seed_vault_db(&conn, count);
    }
    enable_encryption_for_file(&path, &passphrase).unwrap();
    std::fs::remove_file(path.with_extension("db.bak")).unwrap();
    world.boot.dl_default_dir = Some(dir);
}

#[given(expr = "默认数据目录中存在一个缺列漂移的明文库")]
fn default_dir_with_drifted_db(world: &mut LedgerWorld) {
    let dir = world.scratch_dir("e2e-sf-drift");
    let path = dir.join(DB_FILE_NAME);
    {
        let mut conn = open_connection(&path).unwrap();
        init_db(&mut conn).unwrap();
        conn.execute("ALTER TABLE sync_parked_ops DROP COLUMN park_params", [])
            .unwrap();
    }
    world.boot.dl_default_dir = Some(dir);
}

fn takeover(world: &mut LedgerWorld) {
    let dir = world.boot.dl_default_dir.clone().unwrap();
    let db_path = dir.join(DB_FILE_NAME);
    world.last_app_error = None;
    let outcome = match boot::classify_for_boot(&db_path) {
        Ok(boot::BootDisposition::AwaitUnlock) => StartupTakeover::AwaitUnlock,
        Ok(boot::BootDisposition::Unreadable) => StartupTakeover::Failed,
        Ok(disposition) => {
            let opened = if disposition == boot::BootDisposition::NormalizePlaintext {
                match normalize_plaintext_db_file(&db_path) {
                    Ok(()) => open_db_in(&dir),
                    Err(e) => Err(e),
                }
            } else {
                open_db_in(&dir)
            };
            match opened {
                Ok(state) => {
                    world.boot.dl_conn = Some(state);
                    StartupTakeover::Opened
                }
                Err(e) => {
                    world.last_app_error = Some(e);
                    StartupTakeover::Failed
                }
            }
        }
        Err(e) => {
            world.last_app_error = Some(e);
            StartupTakeover::Failed
        }
    };
    world.boot.sf_last_takeover = Some(outcome);
}

#[when(expr = "按启动处置流程尝试接管库文件")]
fn takeover_step(world: &mut LedgerWorld) {
    takeover(world);
}

#[when(expr = "执行 DataLocation 引导（不建连）")]
fn boot_only(world: &mut LedgerWorld) {
    let default_dir = world.boot.dl_default_dir.clone().unwrap();
    let boot = data_location::boot(&default_dir);
    world.boot.last_boot = Some(boot);
}

#[when(expr = "以未登记引导解析生效库目录")]
fn resolve_without_boot(world: &mut LedgerWorld) {
    let default_dir = world.boot.dl_default_dir.clone().unwrap();
    world.boot.sf_resolved_dir = Some(effective_db_dir(None, &default_dir));
}

#[when(expr = "从备份恢复到生效目录的库位置（无已打开库连接参与）")]
fn restore_into_boot_dir(world: &mut LedgerWorld) {
    let backup = world.boot.last_backup_path.clone().expect("尚未备份");
    let boot = world.boot.last_boot.as_ref().expect("尚未执行引导");
    let default_dir = world.boot.dl_default_dir.clone().unwrap();
    let db_path = boot.db_dir.join(DB_FILE_NAME);
    let safety_dir = default_dir.join("restore-safety");
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected, None);
    assert!(result.is_ok(), "恢复失败: {:?}", result.err());
    std::fs::remove_dir_all(&safety_dir).ok();
}

fn takeover_db_path(world: &LedgerWorld) -> PathBuf {
    let dir = world
        .boot
        .dl_default_dir
        .as_ref()
        .expect("未登记默认数据目录");
    dir.join(DB_FILE_NAME)
}

fn restore_into_failed_location(world: &mut LedgerWorld, passphrase: Option<&str>) {
    let backup = world.boot.last_backup_path.clone().expect("尚未备份");
    let db_path = takeover_db_path(world);
    let default_dir = world.boot.dl_default_dir.clone().unwrap();
    let safety_dir = default_dir.join("restore-safety");
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected, passphrase);
    world.boot.restore_safety_dir = Some(safety_dir);
    world.boot.restored_db_path = Some(db_path);
    assert!(result.is_ok(), "恢复失败: {:?}", result.err());
}

#[when(expr = "从备份恢复到启动失败的库位置（无已打开库连接参与）")]
fn restore_into_failed_location_plaintext(world: &mut LedgerWorld) {
    restore_into_failed_location(world, None);
}

#[when(expr = "以主口令 {string} 从备份恢复到启动失败的库位置")]
fn restore_into_failed_location_with_passphrase(world: &mut LedgerWorld, passphrase: String) {
    restore_into_failed_location(world, Some(&passphrase));
}

#[when(expr = "尝试以主口令 {string} 从备份恢复到启动失败的库位置")]
fn try_restore_into_failed_location(world: &mut LedgerWorld, passphrase: String) {
    let backup = world.boot.last_backup_path.clone().expect("尚未备份");
    let db_path = takeover_db_path(world);
    let default_dir = world.boot.dl_default_dir.clone().unwrap();
    let safety_dir = default_dir.join("restore-safety-attempt");
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected, Some(&passphrase));
    world.boot.restore_safety_dir = Some(safety_dir);
    assert!(result.is_err(), "错误口令恢复应被拒绝");
    if let Err(e) = result {
        world.last_app_error = Some(e);
    }
}

#[when(expr = "从备份恢复到等待解锁的库位置（解锁屏恢复入口）")]
fn restore_into_awaiting_unlock_location(world: &mut LedgerWorld) {
    restore_into_failed_location(world, None);
}

#[when(expr = "以主口令 {string} 从备份恢复到等待解锁的库位置")]
fn restore_into_awaiting_unlock_location_with_passphrase(
    world: &mut LedgerWorld,
    passphrase: String,
) {
    restore_into_failed_location(world, Some(&passphrase));
}

#[when(expr = "尝试以主口令 {string} 从备份恢复到等待解锁的库位置")]
fn try_restore_into_awaiting_unlock_location(world: &mut LedgerWorld, passphrase: String) {
    let backup = world.boot.last_backup_path.clone().expect("尚未备份");
    let db_path = takeover_db_path(world);
    let default_dir = world.boot.dl_default_dir.clone().unwrap();
    let safety_dir = default_dir.join("restore-safety-attempt");
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected, Some(&passphrase));
    world.boot.restore_safety_dir = Some(safety_dir);
    assert!(result.is_err(), "错误口令恢复应被拒绝");
    if let Err(e) = result {
        world.last_app_error = Some(e);
    }
}

#[then(expr = "启动应进入失败状态")]
fn startup_failed(world: &mut LedgerWorld) {
    assert_eq!(
        world.boot.sf_last_takeover,
        Some(StartupTakeover::Failed),
        "启动应进入失败状态（前端失败恢复屏接管）"
    );
}

#[then(expr = "启动失败错误码应为 {string}")]
fn startup_failed_with_code(world: &mut LedgerWorld, code: String) {
    let error = world.last_app_error.as_ref().expect("预期启动失败错误");
    assert_eq!(
        error.code(),
        Some(code.as_str()),
        "启动失败错误码不匹配，实际: {error}"
    );
}

#[then(expr = "启动应进入等待解锁")]
fn startup_awaits_unlock(world: &mut LedgerWorld) {
    assert_eq!(
        world.boot.sf_last_takeover,
        Some(StartupTakeover::AwaitUnlock),
        "真密文库应保持锁定等待解锁（#570 既有路径）"
    );
}

#[then(expr = "启动应正常打开")]
fn startup_opened(world: &mut LedgerWorld) {
    assert_eq!(
        world.boot.sf_last_takeover,
        Some(StartupTakeover::Opened),
        "完好明文库应正常打开（明文日常启动零改动）"
    );
}

#[then(expr = "引导登记的生效库目录应为目标目录")]
fn effective_dir_from_boot_is_target(world: &mut LedgerWorld) {
    let boot = world.boot.last_boot.as_ref().expect("尚未执行引导");
    let default_dir = world.boot.dl_default_dir.as_ref().unwrap();
    let target = world.boot.dl_target_dir.as_ref().unwrap();
    assert_eq!(
        effective_db_dir(Some(boot), default_dir),
        *target,
        "引导登记的生效库目录应解析为目标目录"
    );
}

#[then(expr = "解析出的生效库目录应为默认数据目录")]
fn resolved_dir_is_default(world: &mut LedgerWorld) {
    let default_dir = world.boot.dl_default_dir.as_ref().unwrap();
    assert_eq!(
        world.boot.sf_resolved_dir.as_ref().unwrap(),
        default_dir,
        "未登记引导时生效库目录应回退默认数据目录"
    );
}

#[then(expr = "重置保留的 .bak 副本应与原库字节一致")]
fn bak_bytes_match_original(world: &mut LedgerWorld) {
    let dir = world.boot.dl_default_dir.as_ref().unwrap();
    let bak = dir.join(DB_FILE_NAME).with_extension("db.bak");
    let bytes = std::fs::read(&bak).unwrap();
    assert_eq!(
        bytes,
        *world
            .boot
            .dl_default_db_bytes
            .as_ref()
            .expect("未记录字节快照"),
        ".bak 副本应原样保留旧库字节（重置命名语义）"
    );
}

#[then(expr = "默认数据目录中不存在库文件")]
fn default_dir_has_no_db(world: &mut LedgerWorld) {
    let dir = world.boot.dl_default_dir.as_ref().unwrap();
    assert!(
        !dir.join(DB_FILE_NAME).exists(),
        "恢复应写入引导目录，默认数据目录不应被写入库文件（旧缺陷：写死默认目录）"
    );
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

#[then(expr = "恢复安全备份应与失败前库字节一致")]
fn safety_backup_bytes_match_original(world: &mut LedgerWorld) {
    let safety_dir = world
        .boot
        .restore_safety_dir
        .as_ref()
        .expect("尚未触发安全备份");
    let safety = safety_backup_file(safety_dir);
    let bytes = std::fs::read(&safety).unwrap();
    assert_eq!(
        bytes,
        *world
            .boot
            .dl_default_db_bytes
            .as_ref()
            .expect("未记录字节快照"),
        "失败场景的恢复安全备份应是失败前库的字节副本（fs::copy 语义）"
    );
}

#[then(expr = "启动失败库位置的文件字节应保持不变")]
fn failed_db_bytes_unchanged(world: &mut LedgerWorld) {
    let bytes = std::fs::read(takeover_db_path(world)).unwrap();
    assert_eq!(
        bytes,
        *world
            .boot
            .dl_default_db_bytes
            .as_ref()
            .expect("未记录字节快照"),
        "被拒绝的恢复不应改动失败库位置任何字节（可回滚语义）"
    );
}

#[then(expr = "等待解锁库位置的文件字节应保持不变")]
fn awaiting_unlock_db_bytes_unchanged(world: &mut LedgerWorld) {
    let bytes = std::fs::read(takeover_db_path(world)).unwrap();
    assert_eq!(
        bytes,
        *world
            .boot
            .dl_default_db_bytes
            .as_ref()
            .expect("未记录字节快照"),
        "被拒绝的恢复不应改动等待解锁的库文件任何字节（解锁无限重试前提）"
    );
}

#[then(expr = "拒绝场景不应生成恢复安全备份")]
fn rejected_restore_creates_no_safety_backup(world: &mut LedgerWorld) {
    let safety_dir = world
        .boot
        .restore_safety_dir
        .as_ref()
        .expect("未登记尝试用安全目录");
    assert!(
        !safety_dir.exists(),
        "口令校验拒绝发生在安全备份之前，不应生成安全备份目录"
    );
}
