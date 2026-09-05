//! 启动失败恢复 BDD 步骤（issue #601 / ADR-0075 决策 5 修订）。
//!
//! 与 data_location / encryption feature 同一文件级接缝：真临时目录驱动真实
//! 文件系统，只断言外部可见行为（启动处置三态、失败重置闭环、恢复通道的
//! 引导目录落位）。「按启动处置流程尝试接管库文件」复刻 `lib.rs::init_database`
//! 消费 `classify_for_boot` 的判定 + 建连序列——与真实启动同一实现接缝
//! （先例：BDD 直调命令层内部函数）。

use cucumber::{given, then, when};

use tauri_app_lib::backup::{expected_schema_version, restore_db_from};
use tauri_app_lib::db::data_location::{self, DB_FILE_NAME, effective_db_dir};
use tauri_app_lib::db::encryption::SQLITE_HEADER_MAGIC;
use tauri_app_lib::db::{boot, new_uuid, open_db_in};

use crate::world::{LedgerWorld, StartupTakeover};

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

#[given(expr = "默认数据目录中存在一个头部完好但内容损坏的明文库")]
fn default_dir_with_corrupt_plaintext_db(world: &mut LedgerWorld) {
    let dir = std::env::temp_dir().join(format!("ledger-e2e-sf-plain-{}", new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    // 明文魔数完好、内容为垃圾：头探测按明文建连，建连即失败——启动失败的
    // 主场景（旧世界走原生「重置/退出」对话框，issue #601 起进失败恢复屏）。
    let mut bytes = SQLITE_HEADER_MAGIC.to_vec();
    bytes.extend_from_slice(b"corrupted body: not a real database page");
    std::fs::write(dir.join(DB_FILE_NAME), bytes).unwrap();
    world.dl_default_dir = Some(dir);
}

#[given(expr = "目标目录中存在一个损坏的库文件")]
fn target_dir_with_corrupt_db(world: &mut LedgerWorld) {
    let target = world.dl_target_dir.clone().unwrap();
    std::fs::create_dir_all(&target).unwrap();
    // 目标目录已有同名库（损坏残留）：引导直接接管目标位置，不发生搬迁。
    std::fs::write(target.join(DB_FILE_NAME), b"not a database at all").unwrap();
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

/// 启动处置接管（与 `lib.rs::init_database` 同序）：文件判定 → 按三态分派，
/// 明文路径建连失败同样归入启动失败。
fn takeover(world: &mut LedgerWorld) {
    let dir = world.dl_default_dir.clone().unwrap();
    let db_path = dir.join(DB_FILE_NAME);
    let outcome = match boot::classify_for_boot(&db_path) {
        Ok(boot::BootDisposition::AwaitUnlock) => StartupTakeover::AwaitUnlock,
        Ok(boot::BootDisposition::Unreadable) => StartupTakeover::Failed,
        Ok(boot::BootDisposition::OpenPlaintext) => match open_db_in(&dir) {
            Ok(state) => {
                world.dl_conn = Some(state);
                StartupTakeover::Opened
            }
            Err(_) => StartupTakeover::Failed,
        },
        Err(_) => StartupTakeover::Failed,
    };
    world.sf_last_takeover = Some(outcome);
}

#[when(expr = "按启动处置流程尝试接管库文件")]
fn takeover_step(world: &mut LedgerWorld) {
    takeover(world);
}

#[when(expr = "执行 DataLocation 引导（不建连）")]
fn boot_only(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    let boot = data_location::boot(&default_dir);
    world.last_boot = Some(boot);
}

#[when(expr = "以未登记引导解析生效库目录")]
fn resolve_without_boot(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.clone().unwrap();
    world.sf_resolved_dir = Some(effective_db_dir(None, &default_dir));
}

#[when(expr = "从备份恢复到生效目录的库位置（无已打开库连接参与）")]
fn restore_into_boot_dir(world: &mut LedgerWorld) {
    let backup = world.last_backup_path.clone().expect("尚未备份");
    let boot = world.last_boot.as_ref().expect("尚未执行引导");
    let default_dir = world.dl_default_dir.clone().unwrap();
    let db_path = boot.db_dir.join(DB_FILE_NAME);
    // 安全备份与真实命令壳同口径落默认数据目录（词汇表 RestoreSafetyBackup）。
    let safety_dir = default_dir.join("restore-safety");
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected, None);
    assert!(result.is_ok(), "恢复失败: {:?}", result.err());
    std::fs::remove_dir_all(&safety_dir).ok();
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "启动应进入失败状态")]
fn startup_failed(world: &mut LedgerWorld) {
    assert_eq!(
        world.sf_last_takeover,
        Some(StartupTakeover::Failed),
        "启动应进入失败状态（前端失败恢复屏接管）"
    );
}

#[then(expr = "启动应进入等待解锁")]
fn startup_awaits_unlock(world: &mut LedgerWorld) {
    assert_eq!(
        world.sf_last_takeover,
        Some(StartupTakeover::AwaitUnlock),
        "真密文库应保持锁定等待解锁（#570 既有路径）"
    );
}

#[then(expr = "启动应正常打开")]
fn startup_opened(world: &mut LedgerWorld) {
    assert_eq!(
        world.sf_last_takeover,
        Some(StartupTakeover::Opened),
        "完好明文库应正常打开（明文日常启动零改动）"
    );
}

#[then(expr = "引导登记的生效库目录应为目标目录")]
fn effective_dir_from_boot_is_target(world: &mut LedgerWorld) {
    let boot = world.last_boot.as_ref().expect("尚未执行引导");
    let default_dir = world.dl_default_dir.as_ref().unwrap();
    let target = world.dl_target_dir.as_ref().unwrap();
    assert_eq!(
        effective_db_dir(Some(boot), default_dir),
        *target,
        "引导登记的生效库目录应解析为目标目录"
    );
}

#[then(expr = "解析出的生效库目录应为默认数据目录")]
fn resolved_dir_is_default(world: &mut LedgerWorld) {
    let default_dir = world.dl_default_dir.as_ref().unwrap();
    assert_eq!(
        world.sf_resolved_dir.as_ref().unwrap(),
        default_dir,
        "未登记引导时生效库目录应回退默认数据目录"
    );
}

#[then(expr = "重置保留的 .bak 副本应与原库字节一致")]
fn bak_bytes_match_original(world: &mut LedgerWorld) {
    let dir = world.dl_default_dir.as_ref().unwrap();
    let bak = dir.join(DB_FILE_NAME).with_extension("db.bak");
    let bytes = std::fs::read(&bak).unwrap();
    assert_eq!(
        bytes,
        *world.dl_default_db_bytes.as_ref().expect("未记录字节快照"),
        ".bak 副本应原样保留旧库字节（重置命名语义）"
    );
}

#[then(expr = "默认数据目录中不存在库文件")]
fn default_dir_has_no_db(world: &mut LedgerWorld) {
    let dir = world.dl_default_dir.as_ref().unwrap();
    assert!(
        !dir.join(DB_FILE_NAME).exists(),
        "恢复应写入引导目录，默认数据目录不应被写入库文件（旧缺陷：写死默认目录）"
    );
}
