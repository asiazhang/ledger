//! [`crate::db::boot`] 单元测试：库文件启动处置判定与启动失败门（issue #601）。
//!
//! 判定语义的跨域组合行为（损坏 → 失败 → 重置闭环）由 BDD
//! `features/startup_failure.feature` 以真临时目录文件库钉住；此处只钉
//! 纯函数级别的三态分派与门状态翻转。

use super::{BootDisposition, BootFailureGate, classify_for_boot};

fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("ledger-unit-boot-{}-{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ledger.db");
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn plaintext_header_opens_plaintext_even_with_garbage_body() {
    // 头部魔数完好：一律按明文建连路径（建连是否失败由建连步骤判定，
    // 判定层不预判内容完整性）。
    let mut bytes = crate::db::encryption::SQLITE_HEADER_MAGIC.to_vec();
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
    let missing = std::env::temp_dir().join("ledger-unit-boot-missing-nonexistent.db");
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
    gate.set_failed();
    gate.set_failed();
    assert!(gate.is_failed());
    gate.clear();
    assert!(!gate.is_failed());
}

#[test]
fn clone_shares_the_same_flag() {
    let gate = BootFailureGate::new();
    let cloned = gate.clone();
    cloned.set_failed();
    assert!(gate.is_failed());
}

#[test]
fn plan_boot_classifies_the_resolved_dir_not_the_default_dir() {
    // 重引导计划的两步同序钉子（issue #644）：DataLocation 解析把生效目录
    // 指向目标后，处置判定消费的是目标目录里的库文件，不是默认目录的。
    use crate::db::boot::plan_boot;
    let base = std::env::temp_dir().join(format!("ledger-unit-planboot-{}", std::process::id()));
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
    assert_eq!(
        plan.disposition.unwrap(),
        super::BootDisposition::AwaitUnlock
    );
    std::fs::remove_dir_all(base).ok();
}
