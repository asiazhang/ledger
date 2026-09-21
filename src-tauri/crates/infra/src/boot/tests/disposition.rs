//! [`crate::boot::disposition`] 单元测试：库文件启动处置判定与启动失败门
//! （issue #601）。
//!
//! 判定语义的跨域组合行为（损坏 → 失败 → 重置闭环）由 BDD
//! `features/startup_failure.feature` 以真临时目录文件库钉住；此处只钉
//! 纯函数级别的三态分派与门状态翻转。

use tauri_app_lib::test_support::{ScratchDir, ScratchFile};

use crate::boot::disposition::{
    BOOT_DB_UNREADABLE, BootDisposition, BootFailureGate, classify_for_boot,
};

/// 文件夹具（issue #1645）：住各自的暂存目录，guard 随用例持有整棵删除。
fn temp_file(name: &str, bytes: &[u8]) -> ScratchFile {
    let file = ScratchFile::new(&format!("unit-boot-{name}"), "ledger.db");
    std::fs::write(&file, bytes).unwrap();
    file
}

/// 暂存目录（ScratchDir guard，issue #1645）：drop（含 panic unwind）整棵删除。
fn temp_dir(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("unit-boot-{tag}"))
}

#[test]
fn plaintext_header_opens_plaintext_even_with_garbage_body() {
    // 头部魔数完好、每页保留字节（偏移 20）为 0：一律按明文建连路径（建连
    // 是否失败由建连步骤判定，判定层不预判内容完整性）。偏移 20 非 0 的
    // 头部另有去向（外来形态，见 foreign_reserved_bytes… 用例）。
    let mut bytes = crate::db::encryption::SQLITE_HEADER_MAGIC.to_vec();
    bytes.extend_from_slice(&[0, 0, 0, 0, 0]);
    bytes.extend_from_slice(b"garbage body");
    let path = temp_file("plain-garbage", &bytes);
    assert_eq!(
        classify_for_boot(&path).unwrap(),
        BootDisposition::OpenPlaintext
    );
}

#[test]
fn short_and_missing_files_are_plaintext_fresh_install() {
    let path = temp_file("short", b"ab");
    assert_eq!(
        classify_for_boot(&path).unwrap(),
        BootDisposition::OpenPlaintext
    );
    let missing_dir = temp_dir("missing");
    let missing = missing_dir.join("nonexistent.db");
    assert_eq!(
        classify_for_boot(&missing).unwrap(),
        BootDisposition::OpenPlaintext
    );
}

#[test]
fn garbage_without_encrypted_layout_is_unreadable() {
    // 旧缺陷回归钉子：任意字节残留曾被头探测计为「密文库」卡在解锁屏，
    // 现按启动失败处理（issue #601）。
    let path = temp_file("garbage", b"definitely not a sqlite file");
    assert_eq!(
        classify_for_boot(&path).unwrap(),
        BootDisposition::Unreadable
    );
}

#[test]
fn page_aligned_non_magic_file_awaits_unlock() {
    // 具备密文库页对齐落盘形态（4096 整页、非明文魔数）→ 真密文库，等待解锁。
    let bytes = vec![0x7bu8; 4096];
    let path = temp_file("enc-shaped", &bytes);
    assert_eq!(
        classify_for_boot(&path).unwrap(),
        BootDisposition::AwaitUnlock
    );
}

#[test]
fn foreign_reserved_bytes_plaintext_classifies_as_normalize_plaintext() {
    // 外来形态明文库（每页保留字节 12，外部工具写的合法库，issue #1453）：
    // 建连前先归一化——分类若漏了这一态，启动会带着畸形形态进入日常路径，
    // 备份与同步检查点产出全灭（用户可观察回归）。
    let dir = temp_dir("foreign");
    let path = dir.join("ledger.db");
    crate::test_utils::write_foreign_form_plaintext_db(&path, 1);
    assert_eq!(
        classify_for_boot(&path).unwrap(),
        BootDisposition::NormalizePlaintext
    );
}

#[test]
fn app_owned_plaintext_db_classifies_as_open_plaintext() {
    // 应用自有形态的明文库（保留字节 0）→ 既有建连路径零改动，不进归一化态。
    let dir = temp_dir("owned");
    let path = dir.join("ledger.db");
    {
        let mut conn = crate::db::open_connection(&path).unwrap();
        crate::db::migrations().to_latest(&mut conn).unwrap();
    }
    assert_eq!(
        classify_for_boot(&path).unwrap(),
        BootDisposition::OpenPlaintext
    );
}

#[test]
fn non_aligned_truncated_encrypted_shape_is_unreadable() {
    // 非整页（截断）的密文形态：不构成可信密文库，按启动失败处理。
    let bytes = vec![0x7bu8; 4096 + 16];
    let path = temp_file("truncated-enc", &bytes);
    assert_eq!(
        classify_for_boot(&path).unwrap(),
        BootDisposition::Unreadable
    );
}

#[test]
fn gate_flips_and_is_idempotent() {
    let gate = BootFailureGate::new();
    assert!(!gate.is_failed());
    gate.set_failed(None);
    gate.set_failed(None);
    assert!(gate.is_failed());
    gate.clear();
    assert!(!gate.is_failed());
}

#[test]
fn clone_shares_the_same_flag() {
    let gate = BootFailureGate::new();
    let cloned = gate.clone();
    cloned.set_failed(None);
    assert!(gate.is_failed());
}

#[test]
fn gate_records_failure_code_and_falls_back_when_uncoded() {
    // 失败门记录引导失败错误的稳定码（issue #994 / ADR-0100）：漂移码原样
    // 上报，前端失败恢复屏按码区分「结构异常」与「库不可读」的文案与动作排序。
    let gate = BootFailureGate::new();
    gate.set_failed(Some(crate::db::schema_guard::BOOT_SCHEMA_DRIFT));
    assert!(gate.is_failed());
    assert_eq!(gate.failure_code(), "boot.schema-drift");
    gate.clear();
    assert!(!gate.is_failed());
    // 非码化失败（无码构造/极端时序）：回退既有单一码，#601 wire 行为不回退。
    gate.set_failed(None);
    assert_eq!(gate.failure_code(), BOOT_DB_UNREADABLE);
}

#[test]
fn plan_boot_classifies_the_resolved_dir_not_the_default_dir() {
    // 重引导计划的两步同序钉子（issue #644）：DataLocation 解析把生效目录
    // 指向目标后，处置判定消费的是目标目录里的库文件，不是默认目录的。
    use crate::db::boot::plan_boot;
    let base = temp_dir("planboot");
    let default_dir = base.join("default");
    let target = base.join("target");
    std::fs::create_dir_all(&default_dir).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    // 默认目录放损坏残留；目标目录放真密文库形态——计划必须判目标。
    std::fs::write(default_dir.join("ledger.db"), b"garbage residue").unwrap();
    std::fs::write(target.join("ledger.db"), vec![0x7bu8; 4096]).unwrap();
    // 指针文件指向目标（configured intent，JSON 形态与 PointerFile 同型）。
    std::fs::write(
        default_dir.join(crate::db::data_location::POINTER_FILE_NAME),
        serde_json::json!({ "data_dir": target.to_string_lossy() }).to_string(),
    )
    .unwrap();

    let plan = plan_boot(&default_dir);
    assert_eq!(plan.boot.db_dir, target);
    assert_eq!(plan.disposition.unwrap(), BootDisposition::AwaitUnlock);
}
