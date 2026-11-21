use std::path::{Path, PathBuf};

use tauri_app_lib::test_support::ScratchDir;

use crate::boot::book_registry;
use crate::boot::book_registry::{BookRegistry, RegistryOrigin, RegistryRead, read_registry};
use crate::boot::data_location::*;
use crate::db::{check_integrity, open_connection};

fn temp_dir(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("dl-unit-{tag}"))
}

fn file_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

fn dir_listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn boot_missing_registry_uses_default_dir_as_factory_default_book() {
    let dir = temp_dir("boot-missing");
    let boot = boot(&dir);
    assert_eq!(boot.db_dir, dir.path());
    assert_eq!(boot.fallback_reason, None);
    assert_eq!(boot.deferred_relocation, None);
    let registry = boot.registry.expect("出厂登记信息应可用");
    assert_eq!(registry.origin, RegistryOrigin::LegacyPointer);
    assert_eq!(registry.books.len(), 1);
    assert_eq!(registry.active_dir(), Some(dir.path()));
}

#[test]
fn boot_legacy_pointer_opens_configured_dir_without_touching_files() {
    let dir = temp_dir("boot-legacy");
    let target = dir.join("custom");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(dir.join("ledger.db"), b"payload-old").unwrap();
    std::fs::write(target.join("ledger.db"), b"payload-new").unwrap();
    let pointer = serde_json::json!({ "data_dir": target.to_string_lossy() }).to_string();
    std::fs::write(dir.join(POINTER_FILE_NAME), &pointer).unwrap();

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, target);
    assert_eq!(boot.fallback_reason, None);
    assert_eq!(boot.deferred_relocation, None);
    let registry = boot.registry.expect("旧格式登记信息应可用");
    assert_eq!(registry.origin, RegistryOrigin::LegacyPointer);
    assert_eq!(registry.active_dir(), Some(target.as_path()));
    assert_eq!(file_bytes(&dir.join("ledger.db")), b"payload-old");
    assert_eq!(file_bytes(&target.join("ledger.db")), b"payload-new");
    assert_eq!(file_bytes(&dir.join(POINTER_FILE_NAME)), pointer.as_bytes());
}

#[test]
fn boot_corrupt_registry_falls_back_and_preserves_everything() {
    let dir = temp_dir("boot-corrupt");
    std::fs::write(dir.join("ledger.db"), b"payload").unwrap();
    std::fs::write(dir.join(POINTER_FILE_NAME), "{broken").unwrap();

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, dir.path());
    let reason = boot.fallback_reason.expect("损坏应携带回退原因");
    assert!(!reason.is_empty());
    assert!(
        boot.registry.is_none(),
        "损坏时登记信息不可用（写入被禁止）"
    );
    assert_eq!(file_bytes(&dir.join("ledger.db")), b"payload");
    assert_eq!(dir_listing(&dir), vec![POINTER_FILE_NAME, "ledger.db"]);
}

#[test]
fn boot_new_format_registry_activates_registered_dir() {
    let dir = temp_dir("boot-new");
    let book_a = dir.join("books").join("a");
    std::fs::create_dir_all(&book_a).unwrap();
    std::fs::write(book_a.join("ledger.db"), b"payload-a").unwrap();
    let book_b = dir.join("books").join("b");
    std::fs::create_dir_all(dir.join("books")).unwrap();
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({
            "version": 1,
            "books": [
                { "id": "a", "name": "默认账本", "dir": book_a.to_string_lossy() },
                { "id": "b", "name": "副业", "dir": book_b.to_string_lossy() }
            ],
            "active": "b"
        })
        .to_string(),
    )
    .unwrap();

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, book_b);
    assert_eq!(boot.fallback_reason, None);
    let registry = boot.registry.expect("新格式登记信息应可用");
    assert_eq!(registry.origin, RegistryOrigin::NewFormat);
    assert_eq!(registry.active_dir(), Some(book_b.as_path()));
    assert_eq!(file_bytes(&book_a.join("ledger.db")), b"payload-a");
    assert!(!book_b.join("ledger.db").exists());
    assert!(!dir.join("ledger.db").exists());
}

#[test]
fn boot_new_format_never_relocates_default_dir_db_into_empty_active_book() {
    let dir = temp_dir("boot-no-relocate");
    std::fs::write(dir.join("ledger.db"), b"payload-default").unwrap();
    let book_b = dir.join("books").join("b");
    std::fs::create_dir_all(&book_b).unwrap();
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({
            "version": 1,
            "books": [
                { "id": "a", "name": "默认账本", "dir": dir.to_string_lossy() },
                { "id": "b", "name": "副业", "dir": book_b.to_string_lossy() }
            ],
            "active": "b"
        })
        .to_string(),
    )
    .unwrap();

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, book_b);
    assert_eq!(boot.fallback_reason, None);
    assert!(
        !book_b.join("ledger.db").exists(),
        "空活动账本不得被默认目录的库填充"
    );
    assert_eq!(file_bytes(&dir.join("ledger.db")), b"payload-default");
}

#[test]
fn boot_unavailable_active_dir_falls_back_with_reason_and_preserves_files() {
    let dir = temp_dir("boot-unavailable");
    std::fs::write(dir.join("ledger.db"), b"payload").unwrap();
    let occupied = dir.join("occupied");
    std::fs::write(&occupied, b"not a dir").unwrap();
    let book_dir = occupied.join("sub");
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({
            "version": 1,
            "books": [
                { "id": "a", "name": "默认账本", "dir": dir.to_string_lossy() },
                { "id": "b", "name": "副业", "dir": book_dir.to_string_lossy() }
            ],
            "active": "b"
        })
        .to_string(),
    )
    .unwrap();

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, dir.path());
    let reason = boot.fallback_reason.expect("目录不可用应携带回退原因");
    assert!(reason.contains("目标目录不可用"), "实际 {reason}");
    let registry = boot.registry.expect("可读注册表的登记信息应保留");
    assert_eq!(registry.origin, RegistryOrigin::NewFormat);
    assert_eq!(boot.deferred_relocation, None);
    assert_eq!(file_bytes(&dir.join("ledger.db")), b"payload");
    assert_eq!(file_bytes(&occupied), b"not a dir");
    assert_eq!(
        dir_listing(&dir),
        vec![POINTER_FILE_NAME, "ledger.db", "occupied"]
    );
}

#[test]
fn boot_deferred_legacy_relocation_reports_source_dir_in_registry() {
    let dir = temp_dir("boot-deferred");
    std::fs::write(dir.join("ledger.db"), vec![0x7bu8; 4096]).unwrap();
    let target = dir.join("custom");
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({ "data_dir": target.to_string_lossy() }).to_string(),
    )
    .unwrap();

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, dir.path());
    assert_eq!(boot.deferred_relocation, Some(target));
    assert_eq!(boot.fallback_reason, None, "推迟搬迁不是回退，无警示");
    let registry = boot.registry.expect("推迟窗口登记信息应可用");
    assert_eq!(registry.active_dir(), Some(dir.path()));
    assert_eq!(file_bytes(&dir.join("ledger.db")).len(), 4096);
}

#[test]
fn pointer_roundtrip_and_missing_means_unconfigured() {
    let dir = temp_dir("roundtrip");
    assert!(matches!(read_registry(&dir), RegistryRead::Unconfigured));
    let target = dir.join("somewhere");
    write_pointer(&dir, &target).unwrap();
    match read_registry(&dir) {
        RegistryRead::Resolved(resolved) => {
            assert_eq!(resolved.origin, RegistryOrigin::LegacyPointer);
            assert_eq!(resolved.active_dir(), Some(target.as_path()));
        }
        other => panic!("应读回已配置意图，实际 {other:?}"),
    }
}

#[test]
fn corrupt_pointer_is_normal_input() {
    let dir = temp_dir("corrupt");
    std::fs::write(dir.join(POINTER_FILE_NAME), "{not json").unwrap();
    assert!(matches!(read_registry(&dir), RegistryRead::Corrupt(_)));
    std::fs::write(dir.join(POINTER_FILE_NAME), "{}").unwrap();
    assert!(matches!(read_registry(&dir), RegistryRead::Corrupt(_)));
    std::fs::write(dir.join(POINTER_FILE_NAME), r#"{"data_dir": "  "}"#).unwrap();
    assert!(matches!(read_registry(&dir), RegistryRead::Corrupt(_)));
}

#[test]
fn pointer_write_is_atomic_replace_and_stays_legacy() {
    let dir = temp_dir("atomic");
    let a = dir.join("a");
    let b = dir.join("b");
    write_pointer(&dir, &a).unwrap();
    write_pointer(&dir, &b).unwrap();
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".data_location.json."))
        .collect();
    assert!(leftovers.is_empty(), "不应残留指针临时文件: {leftovers:?}");
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(POINTER_FILE_NAME)).unwrap())
            .unwrap();
    assert_eq!(raw["data_dir"], b.to_string_lossy().to_string());
    assert!(raw.get("books").is_none());
}

#[test]
fn configured_intent_maps_all_pointer_states() {
    let dir = temp_dir("intent");
    assert!(configured_intent(&dir).is_none());
    std::fs::write(dir.join(POINTER_FILE_NAME), "{broken").unwrap();
    assert!(configured_intent(&dir).is_none());
    let target = dir.join("elsewhere");
    write_pointer(&dir, &target).unwrap();
    assert_eq!(configured_intent(&dir), Some(target.clone()));
    let book_dir = dir.join("books").join("main");
    book_registry::write_registry(
        &dir,
        &book_registry::BookRegistry {
            active_id: "m".into(),
            books: vec![book_registry::Book {
                id: "m".into(),
                name: "默认账本".into(),
                dir: book_dir.clone(),
            }],
            pending_relocation: None,
            origin: book_registry::RegistryOrigin::NewFormat,
        },
    )
    .unwrap();
    assert_eq!(configured_intent(&dir), Some(book_dir));
}

#[test]
fn mutable_registry_follows_write_timing_contract() {
    let err = mutable_registry(None).unwrap_err();
    assert!(err.is_code("book.registry-unavailable"), "实际 {err:?}");
    let corrupt = Boot {
        db_dir: PathBuf::from("/data/default"),
        fallback_reason: Some("损坏".into()),
        deferred_relocation: None,
        registry: None,
    };
    let err = mutable_registry(Some(&corrupt)).unwrap_err();
    assert!(err.is_code("book.registry-corrupt"), "实际 {err:?}");
    let mut deferred = corrupt.clone();
    deferred.registry = Some(BookRegistry::single_default(Path::new("/data/default")));
    deferred.db_dir = PathBuf::from("/data/default");
    deferred.deferred_relocation = Some(PathBuf::from("/data/target"));
    let err = mutable_registry(Some(&deferred)).unwrap_err();
    assert!(err.is_code("book.registry-busy"), "实际 {err:?}");
    let mut ready = deferred;
    ready.deferred_relocation = None;
    assert!(mutable_registry(Some(&ready)).is_ok());
}

#[test]
fn gather_book_list_follows_boot_states() {
    use book_registry::RegistryRead;

    let dir = temp_dir("list-ok");
    let book_dir = dir.join("books").join("m");
    std::fs::create_dir_all(&book_dir).unwrap();
    book_registry::write_registry(
        &dir,
        &book_registry::BookRegistry {
            active_id: "m".into(),
            books: vec![book_registry::Book {
                id: "m".into(),
                name: "默认账本".into(),
                dir: book_dir,
            }],
            pending_relocation: None,
            origin: book_registry::RegistryOrigin::NewFormat,
        },
    )
    .unwrap();
    let boot = crate::boot::data_location::boot(&dir);
    let info = gather_book_list(&dir, Some(&boot));
    assert_eq!(info.books.len(), 1);
    assert_eq!(info.active_id.as_deref(), Some("m"));
    assert!(info.mutable);
    assert!(info.fallback_reason.is_none());

    let second_dir = dir.join("books").join("s");
    std::fs::create_dir_all(&second_dir).unwrap();
    let mut registry = match book_registry::read_registry(&dir) {
        RegistryRead::Resolved(registry) => registry,
        other => panic!("现场应可解析，实际 {other:?}"),
    };
    registry.books.push(book_registry::Book {
        id: "s".into(),
        name: "二本".into(),
        dir: second_dir,
    });
    book_registry::write_registry(&dir, &registry).unwrap();
    let info = gather_book_list(&dir, Some(&boot));
    assert_eq!(info.books.len(), 2, "清单应取最新落盘态");

    let broken = temp_dir("list-broken");
    std::fs::write(broken.join(POINTER_FILE_NAME), "{broken").unwrap();
    let boot = crate::boot::data_location::boot(&broken);
    let info = gather_book_list(&broken, Some(&boot));
    assert!(info.books.is_empty());
    assert_eq!(info.active_id, None);
    assert!(!info.mutable);
    assert!(info.fallback_reason.is_some());

    let boot = Boot {
        db_dir: PathBuf::from("/data/source"),
        fallback_reason: None,
        deferred_relocation: Some(PathBuf::from("/data/target")),
        registry: Some(BookRegistry::single_default(Path::new("/data/source"))),
    };
    let info = gather_book_list(Path::new("/data/source"), Some(&boot));
    assert_eq!(info.books.len(), 1);
    assert!(!info.mutable);
    assert!(info.fallback_reason.is_none());

    let info = gather_book_list(&dir, None);
    assert_eq!(info.books.len(), 2);
    assert!(!info.mutable);
}

fn write_registry_with_books(dir: &Path, books: &[(&str, &str, &Path)], active: &str) {
    let entries: Vec<serde_json::Value> = books
        .iter()
        .map(|(id, name, path)| {
            serde_json::json!({ "id": id, "name": name, "dir": path.to_string_lossy() })
        })
        .collect();
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({ "version": 1, "books": entries, "active": active }).to_string(),
    )
    .unwrap();
}

#[test]
fn boot_consumes_pending_relocation_target_has_db() {
    let dir = temp_dir("pending-adopt");
    let source = dir.join("source");
    let target = dir.join("target");
    let other = dir.join("other");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(source.join("ledger.db"), b"old").unwrap();
    std::fs::write(target.join("ledger.db"), b"new").unwrap();
    std::fs::write(other.join("ledger.db"), b"other").unwrap();
    write_registry_with_books(
        &dir,
        &[("a", "默认账本", &source), ("b", "副业", &other)],
        "a",
    );
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({
            "version": 1,
            "books": [
                { "id": "a", "name": "默认账本", "dir": target.to_string_lossy() },
                { "id": "b", "name": "副业", "dir": other.to_string_lossy() }
            ],
            "active": "a",
            "relocation": { "book_id": "a", "from_dir": source.to_string_lossy() }
        })
        .to_string(),
    )
    .unwrap();

    let result = boot(&dir);
    assert_eq!(result.db_dir, target);
    assert_eq!(result.fallback_reason, None);
    assert_eq!(result.deferred_relocation, None);
    let registry = result.registry.expect("登记信息应可用");
    assert_eq!(
        registry.pending_relocation, None,
        "内存态意图已消费（文件态由登记变更自愈）"
    );
    assert_eq!(file_bytes(&target.join("ledger.db")), b"new", "接管目标库");
    assert_eq!(
        file_bytes(&source.join("ledger.db")),
        b"old",
        "来源库原样保留"
    );
    assert_eq!(
        file_bytes(&other.join("ledger.db")),
        b"other",
        "其他账本原地不动"
    );
    let again = boot(&dir);
    assert_eq!(again.db_dir, target);
}

#[test]
fn boot_pending_relocation_moves_db_via_vacuum() {
    let dir = temp_dir("pending-move");
    let source = dir.join("source");
    let target = dir.join("target");
    let other = dir.join("other");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    drop(crate::db::open_db_in(&source).unwrap());
    write_registry_with_books(
        &dir,
        &[("a", "默认账本", &target), ("b", "副业", &other)],
        "a",
    );
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({
            "version": 1,
            "books": [
                { "id": "a", "name": "默认账本", "dir": target.to_string_lossy() },
                { "id": "b", "name": "副业", "dir": other.to_string_lossy() }
            ],
            "active": "a",
            "relocation": { "book_id": "a", "from_dir": source.to_string_lossy() }
        })
        .to_string(),
    )
    .unwrap();

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, target);
    assert_eq!(boot.fallback_reason, None);
    assert_eq!(
        boot.registry.expect("登记信息应可用").pending_relocation,
        None,
        "搬迁成功即消费意图"
    );
    assert!(target.join("ledger.db").is_file(), "库已就位目标目录");
    check_integrity(&open_connection(target.join(DB_FILE_NAME)).unwrap()).unwrap();
    assert!(source.join("ledger.db").is_file(), "来源库原样保留");
    assert!(!other.join("ledger.db").exists(), "其他账本不动");
}

#[test]
fn boot_pending_relocation_encrypted_source_defers() {
    let dir = temp_dir("pending-encrypted");
    let source = dir.join("source");
    let target = dir.join("target");
    std::fs::create_dir_all(&source).unwrap();
    {
        drop(crate::db::open_db_in(&source).unwrap());
        crate::boot::encryption::enable_encryption_for_file(&source.join(DB_FILE_NAME), "pw")
            .unwrap();
    }
    write_registry_with_books(&dir, &[("a", "默认账本", &target)], "a");
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({
            "version": 1,
            "books": [
                { "id": "a", "name": "默认账本", "dir": target.to_string_lossy() }
            ],
            "active": "a",
            "relocation": { "book_id": "a", "from_dir": source.to_string_lossy() }
        })
        .to_string(),
    )
    .unwrap();

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, source, "推迟窗口以来源位置生效");
    assert_eq!(boot.deferred_relocation, Some(target.clone()));
    assert_eq!(boot.fallback_reason, None);
    let registry = boot.registry.expect("登记信息应可用");
    assert_eq!(registry.active_dir(), Some(source.as_path()));
    assert_eq!(
        registry.pending_relocation,
        Some(book_registry::PendingRelocation {
            book_id: "a".into(),
            from_dir: source.clone()
        })
    );
}

#[test]
fn boot_pending_relocation_failure_falls_back_to_source_with_reason() {
    let dir = temp_dir("pending-fail");
    let source = dir.join("source");
    let target = dir.join("target");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("ledger.db"), b"garbage-not-a-db").unwrap();
    write_registry_with_books(&dir, &[("a", "默认账本", &target)], "a");
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({
            "version": 1,
            "books": [
                { "id": "a", "name": "默认账本", "dir": target.to_string_lossy() }
            ],
            "active": "a",
            "relocation": { "book_id": "a", "from_dir": source.to_string_lossy() }
        })
        .to_string(),
    )
    .unwrap();

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, source, "失败以来源位置生效");
    assert_eq!(boot.deferred_relocation, None);
    let reason = boot.fallback_reason.expect("失败应携带回退警示");
    assert!(
        reason.contains("搬迁") || reason.contains("原库"),
        "{reason}"
    );
    assert!(
        boot.registry
            .expect("登记信息应可用")
            .pending_relocation
            .is_some(),
        "意图保留待重试"
    );
    assert!(!target.join("ledger.db").exists());
    assert_eq!(file_bytes(&source.join("ledger.db")), b"garbage-not-a-db");
}

#[test]
fn boot_ignores_pending_relocation_of_non_active_book() {
    let dir = temp_dir("pending-inactive");
    let a = dir.join("a");
    let b = dir.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    write_registry_with_books(&dir, &[("a", "默认账本", &a), ("b", "副业", &b)], "b");
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({
            "version": 1,
            "books": [
                { "id": "a", "name": "默认账本", "dir": a.to_string_lossy() },
                { "id": "b", "name": "副业", "dir": b.to_string_lossy() }
            ],
            "active": "b",
            "relocation": { "book_id": "a", "from_dir": dir.to_string_lossy() }
        })
        .to_string(),
    )
    .unwrap();

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, b, "按活动账本进入，意图不干扰");
    assert_eq!(boot.fallback_reason, None);
    assert!(!a.join("ledger.db").exists());
}

#[test]
fn validate_and_commit_writes_pending_intent_for_active_book() {
    let dir = temp_dir("commit-narrow");
    let legacy_dir = dir.join("legacy-custom");
    let target = dir.join("new-place");
    let other = dir.join("other-book");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    drop(crate::db::open_db_in(&legacy_dir).unwrap());
    std::fs::write(other.join("ledger.db"), b"other").unwrap();
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({ "data_dir": legacy_dir.to_string_lossy() }).to_string(),
    )
    .unwrap();

    let outcome = validate_and_commit(&dir, &target, false).unwrap();
    assert!(outcome.committed && !outcome.requires_choice);
    assert_eq!(
        outcome.target_dir.as_deref(),
        Some(target.to_str().unwrap())
    );

    let RegistryRead::Resolved(registry) = book_registry::read_registry(&dir) else {
        panic!("提交后应为新格式注册表");
    };
    assert_eq!(registry.books.len(), 1);
    assert_eq!(registry.books[0].dir, target);
    assert_eq!(
        registry.pending_relocation,
        Some(book_registry::PendingRelocation {
            book_id: registry.active_id.clone(),
            from_dir: legacy_dir.clone(),
        })
    );

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, target);
    assert_eq!(boot.fallback_reason, None);
    check_integrity(&open_connection(target.join("ledger.db")).unwrap()).unwrap();
    assert!(legacy_dir.join("ledger.db").is_file(), "原位置库保留");
    assert_eq!(file_bytes(&other.join("ledger.db")), b"other");
}

#[test]
fn validate_and_commit_is_noop_when_target_is_current_dir() {
    let dir = temp_dir("commit-noop");
    let target = dir.join("books").join("m");
    std::fs::create_dir_all(&target).unwrap();
    write_registry_with_books(&dir, &[("m", "默认账本", &target)], "m");

    let outcome = validate_and_commit(&dir, &target, false).unwrap();
    assert!(outcome.committed);
    assert_eq!(configured_intent(&dir), Some(target.clone()));
    let registry = match read_registry(&dir) {
        RegistryRead::Resolved(registry) => registry,
        other => panic!("注册表应可读，实际 {other:?}"),
    };
    assert!(
        registry.pending_relocation.is_none(),
        "幂等提交不产生搬迁意图"
    );
}

#[test]
fn validate_and_commit_rejects_target_of_other_book() {
    let dir = temp_dir("commit-dup");
    let a = dir.join("a");
    let b = dir.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    write_registry_with_books(&dir, &[("a", "默认账本", &a), ("b", "副业", &b)], "a");

    let err = validate_and_commit(&dir, &b, false).unwrap_err();
    assert!(err.is_code("book.dir-exists"), "实际 {err:?}");
    assert!(
        err.to_string().contains("副业"),
        "错误应指认占用账本：{err}"
    );
}

#[test]
fn validate_and_commit_overwrites_corrupt_registry_as_escape() {
    let dir = temp_dir("commit-corrupt");
    std::fs::write(dir.join(POINTER_FILE_NAME), "{not valid json").unwrap();
    let target = dir.join("fresh");

    let outcome = validate_and_commit(&dir, &target, false).unwrap();
    assert!(outcome.committed);
    let RegistryRead::Resolved(registry) = book_registry::read_registry(&dir) else {
        panic!("逃生舱提交后应可解析");
    };
    assert_eq!(registry.books[0].dir, target);
    assert_eq!(
        registry.pending_relocation,
        Some(book_registry::PendingRelocation {
            book_id: registry.active_id.clone(),
            from_dir: dir.path().to_path_buf(),
        })
    );
}

#[test]
fn validate_and_commit_requires_choice_before_writes() {
    let dir = temp_dir("commit-choice");
    let target = dir.join("occupied");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join(DB_FILE_NAME), b"existing").unwrap();

    let outcome = validate_and_commit(&dir, &target, false).unwrap();
    assert!(outcome.requires_choice && !outcome.committed);
    assert!(!dir.join(POINTER_FILE_NAME).exists(), "未选择接管前不落盘");
}
