use tauri_app_lib::test_support::{ScratchDir, ScratchFile};

use crate::boot::disposition::{
    BOOT_DB_UNREADABLE, BootDisposition, BootFailureGate, classify_for_boot,
};

fn temp_file(name: &str, bytes: &[u8]) -> ScratchFile {
    let file = ScratchFile::new(&format!("unit-boot-{name}"), "ledger.db");
    std::fs::write(&file, bytes).unwrap();
    file
}

fn temp_dir(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("unit-boot-{tag}"))
}

#[test]
fn plaintext_header_opens_plaintext_even_with_garbage_body() {
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
    let path = temp_file("garbage", b"definitely not a sqlite file");
    assert_eq!(
        classify_for_boot(&path).unwrap(),
        BootDisposition::Unreadable
    );
}

#[test]
fn page_aligned_non_magic_file_awaits_unlock() {
    let bytes = vec![0x7bu8; 4096];
    let path = temp_file("enc-shaped", &bytes);
    assert_eq!(
        classify_for_boot(&path).unwrap(),
        BootDisposition::AwaitUnlock
    );
}

#[test]
fn foreign_reserved_bytes_plaintext_classifies_as_normalize_plaintext() {
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
    let gate = BootFailureGate::new();
    gate.set_failed(Some(crate::db::schema_guard::BOOT_SCHEMA_DRIFT));
    assert!(gate.is_failed());
    assert_eq!(gate.failure_code(), "boot.schema-drift");
    gate.clear();
    assert!(!gate.is_failed());
    gate.set_failed(None);
    assert_eq!(gate.failure_code(), BOOT_DB_UNREADABLE);
}

#[test]
fn plan_boot_classifies_the_resolved_dir_not_the_default_dir() {
    use crate::db::boot::plan_boot;
    let base = temp_dir("planboot");
    let default_dir = base.join("default");
    let target = base.join("target");
    std::fs::create_dir_all(&default_dir).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(default_dir.join("ledger.db"), b"garbage residue").unwrap();
    std::fs::write(target.join("ledger.db"), vec![0x7bu8; 4096]).unwrap();
    std::fs::write(
        default_dir.join(crate::db::data_location::POINTER_FILE_NAME),
        serde_json::json!({ "data_dir": target.to_string_lossy() }).to_string(),
    )
    .unwrap();

    let plan = plan_boot(&default_dir);
    assert_eq!(plan.boot.db_dir, target);
    assert_eq!(plan.disposition.unwrap(), BootDisposition::AwaitUnlock);
}
