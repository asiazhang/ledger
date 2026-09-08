//! [`crate::db::book_registry`] 单元测试：注册表解析（双格式）、损坏判定与
//! 升级写入（issue #832）。引导回退与文件保全的引导级组合行为由
//! `data_location` 测试与 BDD e2e 覆盖，此处钉内核纯函数语义。

use super::*;
use crate::db::new_uuid;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ledger-registry-unit-{tag}-{}", new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_raw(dir: &Path, content: &str) {
    std::fs::write(dir.join(REGISTRY_FILE_NAME), content).unwrap();
}

#[test]
fn missing_file_is_unconfigured() {
    let dir = temp_dir("missing");
    assert_eq!(read_registry(&dir), RegistryRead::Unconfigured);
}

#[test]
fn legacy_pointer_reads_as_single_default_book() {
    let dir = temp_dir("legacy");
    let configured = dir.join("somewhere");
    write_raw(
        &dir,
        &serde_json::json!({ "data_dir": configured.to_string_lossy() }).to_string(),
    );
    let RegistryRead::Resolved(registry) = read_registry(&dir) else {
        panic!("旧格式应解析为注册表");
    };
    assert_eq!(registry.origin, RegistryOrigin::LegacyPointer);
    assert_eq!(registry.books.len(), 1);
    let book = &registry.books[0];
    assert_eq!(book.dir, configured);
    assert_eq!(book.name, DEFAULT_BOOK_NAME);
    assert!(!book.id.is_empty());
    assert_eq!(registry.active_id, book.id);
    assert_eq!(registry.active_dir(), Some(configured.as_path()));
    // 唯一账本即默认账本兼活动账本：id 现铸但只存内存，文件保持原样（读取绝不回写）。
    let after = std::fs::read_to_string(dir.join(REGISTRY_FILE_NAME)).unwrap();
    assert_eq!(
        after,
        serde_json::json!({ "data_dir": configured.to_string_lossy() }).to_string()
    );
}

#[test]
fn single_default_shapes_factory_registry() {
    let dir = temp_dir("factory");
    let registry = BookRegistry::single_default(&dir);
    assert_eq!(registry.origin, RegistryOrigin::LegacyPointer);
    assert_eq!(registry.books.len(), 1);
    assert_eq!(registry.books[0].dir, dir);
    assert_eq!(registry.books[0].name, DEFAULT_BOOK_NAME);
    assert_eq!(registry.active_dir(), Some(dir.as_path()));
}

#[test]
fn new_format_registry_roundtrips_books_and_active() {
    let dir = temp_dir("roundtrip");
    let book_a = Book {
        id: new_uuid(),
        name: "默认账本".into(),
        dir: dir.join("books").join("a"),
    };
    let book_b = Book {
        id: new_uuid(),
        name: "副业".into(),
        dir: dir.join("books").join("b"),
    };
    let registry = BookRegistry {
        active_id: book_b.id.clone(),
        books: vec![book_a.clone(), book_b.clone()],
        origin: RegistryOrigin::NewFormat,
    };
    write_registry(&dir, &registry).unwrap();

    let RegistryRead::Resolved(resolved) = read_registry(&dir) else {
        panic!("新格式应解析为注册表");
    };
    assert_eq!(resolved, registry);
    assert_eq!(resolved.active(), Some(&book_b));
    assert_eq!(resolved.active_dir(), Some(book_b.dir.as_path()));
}

#[test]
fn registry_write_lands_new_format_shape_without_legacy_field() {
    let dir = temp_dir("shape");
    let book = Book {
        id: "book-1".into(),
        name: "默认账本".into(),
        dir: dir.join("books").join("main"),
    };
    let registry = BookRegistry {
        active_id: book.id.clone(),
        books: vec![book],
        origin: RegistryOrigin::NewFormat,
    };
    write_registry(&dir, &registry).unwrap();

    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(REGISTRY_FILE_NAME)).unwrap())
            .unwrap();
    assert_eq!(raw["version"], 1);
    assert_eq!(raw["active"], "book-1");
    assert_eq!(raw["books"].as_array().unwrap().len(), 1);
    assert_eq!(raw["books"][0]["id"], "book-1");
    assert_eq!(raw["books"][0]["name"], "默认账本");
    // 新格式不再携带旧式 data_dir 字段（目录信息由账本清单承载）。
    assert!(raw.get("data_dir").is_none());
}

#[test]
fn registry_write_replaces_atomically_without_temp_leftovers() {
    let dir = temp_dir("atomic");
    let make_book = |name: &str| Book {
        id: new_uuid(),
        name: name.into(),
        dir: dir.join(name),
    };
    let book_a = make_book("a");
    let first = BookRegistry {
        active_id: book_a.id.clone(),
        books: vec![book_a],
        origin: RegistryOrigin::NewFormat,
    };
    write_registry(&dir, &first).unwrap();
    let book_b = make_book("b");
    let second = BookRegistry {
        active_id: book_b.id.clone(),
        books: vec![book_b],
        origin: RegistryOrigin::NewFormat,
    };
    write_registry(&dir, &second).unwrap();

    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(&format!(".{REGISTRY_FILE_NAME}.")))
        .collect();
    assert!(
        leftovers.is_empty(),
        "不应残留注册表临时文件: {leftovers:?}"
    );
    match read_registry(&dir) {
        RegistryRead::Resolved(registry) => {
            assert_eq!(registry.books[0].name, "b");
        }
        other => panic!("应读回最新注册表，实际 {other:?}"),
    }
}

#[test]
fn upgrade_write_roundtrip_from_legacy_pointer() {
    let dir = temp_dir("upgrade");
    let configured = dir.join("existing-data");
    write_raw(
        &dir,
        &serde_json::json!({ "data_dir": configured.to_string_lossy() }).to_string(),
    );

    // 旧格式读取 → 登记信息可用 → 首次写入落新格式。
    let RegistryRead::Resolved(resolved) = read_registry(&dir) else {
        panic!("旧格式应解析为注册表");
    };
    write_registry(&dir, &resolved).unwrap();

    let RegistryRead::Resolved(upgraded) = read_registry(&dir) else {
        panic!("升级写入后应解析为新格式注册表");
    };
    assert_eq!(upgraded.origin, RegistryOrigin::NewFormat);
    assert_eq!(upgraded.books.len(), 1);
    assert_eq!(upgraded.books[0].dir, configured);
    // 目录与活动指针跨升级保持一致（零数据移动）。
    assert_eq!(upgraded.active_dir(), resolved.active_dir());
}

#[test]
fn corrupt_registry_inputs_fall_back_with_reason() {
    let book = serde_json::json!({ "id": "a", "name": "A", "dir": "/data/a" });
    // 逐例断言：输入 → 期望 Corrupt（原因非空）。
    let corrupt_inputs: Vec<serde_json::Value> = vec![
        serde_json::json!({}),                   // 既无注册表也无旧字段
        serde_json::json!({ "data_dir": "  " }), // 旧字段空白
        serde_json::json!({ "version": 2, "books": [book], "active": "a" }), // 未知版本
        serde_json::json!({ "books": [book] }),  // 缺活动指针
        serde_json::json!({ "active": "a" }),    // 缺清单
        serde_json::json!({ "books": [], "active": "a" }), // 空清单
        serde_json::json!({ "books": [book], "active": "b" }), // 活动指针悬空
        serde_json::json!({ "books": [
            { "id": "a", "name": "A" }
        ], "active": "a" }), // 条目缺目录
        serde_json::json!({ "books": [
            { "id": "a", "dir": "/data/a" }
        ], "active": "a" }), // 条目缺展示名
        serde_json::json!({ "books": [
            { "id": "", "name": "A", "dir": "/data/a" }
        ], "active": "a" }), // 条目缺 id
        serde_json::json!({ "books": [
            { "id": "a", "name": "A", "dir": "/data/a" },
            { "id": "a", "name": "B", "dir": "/data/b" }
        ], "active": "a" }), // id 重复
        serde_json::json!({ "books": [
            { "id": "a", "name": "A", "dir": "/data/same" },
            { "id": "b", "name": "B", "dir": "/data/same" }
        ], "active": "a" }), // 目录重复登记
    ];
    for input in &corrupt_inputs {
        let probe = temp_dir("corrupt-case");
        write_raw(&probe, &input.to_string());
        match read_registry(&probe) {
            RegistryRead::Corrupt(reason) => {
                assert!(!reason.is_empty(), "{input} 应回退原因");
            }
            other => panic!("{input} 应按损坏回退，实际 {other:?}"),
        }
    }
    // 非 JSON 文本同样按损坏。
    let probe = temp_dir("corrupt-raw");
    write_raw(&probe, "{not json");
    assert!(matches!(read_registry(&probe), RegistryRead::Corrupt(_)));
}

#[test]
fn unknown_extra_fields_are_tolerated() {
    // 前向兼容：未来新增字段（注册表层或条目层）不破坏旧读取。
    let dir = temp_dir("forward");
    write_raw(
        &dir,
        r#"{
            "version": 1,
            "books": [ { "id": "a", "name": "A", "dir": "/data/a", "icon": "star" } ],
            "active": "a",
            "future": true
        }"#,
    );
    let RegistryRead::Resolved(registry) = read_registry(&dir) else {
        panic!("未知字段应被容忍");
    };
    assert_eq!(registry.origin, RegistryOrigin::NewFormat);
    assert_eq!(registry.books[0].id, "a");
}

#[test]
fn write_registry_rejects_corrupt_registry_state() {
    let dir = temp_dir("reject");
    let book = Book {
        id: "a".into(),
        name: "A".into(),
        dir: PathBuf::from("/data/a"),
    };
    let dangling = BookRegistry {
        active_id: "zzz".into(),
        books: vec![book.clone()],
        origin: RegistryOrigin::NewFormat,
    };
    let empty = BookRegistry {
        active_id: "a".into(),
        books: vec![],
        origin: RegistryOrigin::NewFormat,
    };
    let duplicate_dir = BookRegistry {
        active_id: "a".into(),
        books: vec![
            book.clone(),
            Book {
                id: "b".into(),
                name: "B".into(),
                dir: book.dir.clone(),
            },
        ],
        origin: RegistryOrigin::NewFormat,
    };
    let dangling_entry = BookRegistry {
        active_id: "a".into(),
        books: vec![Book {
            id: "a".into(),
            name: String::new(),
            dir: PathBuf::from("/data/a"),
        }],
        origin: RegistryOrigin::NewFormat,
    };
    for registry in [dangling, empty, duplicate_dir, dangling_entry] {
        let err = write_registry(&dir, &registry).unwrap_err();
        assert!(err.is_code("book-registry.invalid"), "实际 {err:?}");
    }
    // 被拒绝的写入不得产生文件或残留。
    assert!(!dir.join(REGISTRY_FILE_NAME).exists());
}

// -------------------------------------------------------------------------
// 登记变更命令内核（issue #833）：新建 / 切换 / 改名 / 移除。
// 写入时机契约（注册表损坏、推迟搬迁窗口禁止变更）的引导级组合行为
// 由 data_location 测试与命令集成测试覆盖，此处钉内核纯函数语义。
// -------------------------------------------------------------------------

/// 现场工具：经变更内核造一本已登记账本，返回其 id。
fn create_entry(dir: &Path, name: &str) -> String {
    create_book_entry(dir, name).unwrap().id
}

#[test]
fn create_appends_entry_and_lands_new_format() {
    let dir = temp_dir("create-append");
    let created = create_book_entry(&dir, "家庭账本").unwrap();
    assert_eq!(created.name, "家庭账本");
    // 新账本目录：应用数据目录下自动创建的子目录，且物理存在。
    assert_eq!(
        created.dir.parent(),
        Some(dir.join(BOOKS_DIR_NAME).as_path())
    );
    assert!(created.dir.is_dir());

    // 注册表落新格式：默认账本（出厂折叠）+ 新账本，活动指针仍在默认账本。
    let RegistryRead::Resolved(registry) = read_registry(&dir) else {
        panic!("登记后应解析为新格式注册表");
    };
    assert_eq!(registry.origin, RegistryOrigin::NewFormat);
    assert_eq!(registry.books.len(), 2);
    assert_eq!(registry.books[0].name, DEFAULT_BOOK_NAME);
    assert_eq!(registry.books[0].dir, dir);
    assert_eq!(registry.books[1].id, created.id);
    assert_eq!(registry.active_id, registry.books[0].id);
}

#[test]
fn create_on_legacy_pointer_upgrades_in_place() {
    let dir = temp_dir("create-legacy");
    let configured = dir.join("existing-data");
    std::fs::create_dir_all(&configured).unwrap();
    write_raw(
        &dir,
        &serde_json::json!({ "data_dir": configured.to_string_lossy() }).to_string(),
    );

    create_book_entry(&dir, "副业").unwrap();

    // 首次登记把旧指针确定性升级为新格式：默认账本目录 = 旧 data_dir，零数据移动。
    let RegistryRead::Resolved(registry) = read_registry(&dir) else {
        panic!("登记后应解析为新格式注册表");
    };
    assert_eq!(registry.books.len(), 2);
    assert_eq!(registry.books[0].dir, configured);
    assert_eq!(registry.books[0].name, DEFAULT_BOOK_NAME);
}

#[test]
fn create_rejects_blank_name_without_writes() {
    for name in ["", "   "] {
        let dir = temp_dir("create-blank");
        let err = create_book_entry(&dir, name).unwrap_err();
        assert!(err.is_code("book.name-required"), "({name}) 实际 {err:?}");
        assert!(!dir.join(REGISTRY_FILE_NAME).exists(), "空白名不得落盘");
    }
}

#[test]
#[cfg(unix)]
fn dir_registration_check_folds_symlink_aliases() {
    // 同一目录不得重复登记（路径身份按物理位置折叠）：待登记目录经符号链接
    // 别名指向既有账本目录时按重复拒绝；非链接的全新路径照常放行。create 的
    // 目录名是现铸 uuid、天然全新，此查重是登记接缝的纵深防御（行为级无法
    // 自然触达，直测内核接缝）。
    let dir = temp_dir("create-dup");
    let first = create_book_entry(&dir, "一本").unwrap();
    let link = dir.join(BOOKS_DIR_NAME).join("link-to-first");
    std::os::unix::fs::symlink(&first.dir, &link).unwrap();
    let RegistryRead::Resolved(registry) = read_registry(&dir) else {
        panic!("现场应可解析");
    };

    let err = ensure_dir_not_registered(&registry, &link).unwrap_err();
    assert!(err.is_code("book.dir-exists"), "实际 {err:?}");
    let fresh = dir.join(BOOKS_DIR_NAME).join("fresh");
    std::fs::create_dir_all(&fresh).unwrap();
    ensure_dir_not_registered(&registry, &fresh).unwrap();
}

#[test]
fn create_rejects_corrupt_registry_without_writes() {
    let dir = temp_dir("create-corrupt");
    write_raw(&dir, "{not json");
    let err = create_book_entry(&dir, "家庭账本").unwrap_err();
    assert!(err.is_code("book.registry-corrupt"), "实际 {err:?}");
    // 损坏文件原样保留（覆盖前须走既有恢复通道）。
    assert_eq!(
        std::fs::read_to_string(dir.join(REGISTRY_FILE_NAME)).unwrap(),
        "{not json"
    );
}

#[test]
fn switch_updates_active_pointer_only() {
    let dir = temp_dir("switch");
    let first = create_entry(&dir, "一本");
    let second = create_entry(&dir, "二本");

    let switched = switch_active_book(&dir, &second).unwrap();
    assert_eq!(switched.id, second);
    let RegistryRead::Resolved(registry) = read_registry(&dir) else {
        panic!("切换后应可解析");
    };
    assert_eq!(registry.active_id, second);
    assert_eq!(registry.books.len(), 3); // 默认 + 两本，清单不变
    let _ = first;
}

#[test]
fn switch_rejects_unknown_id() {
    let dir = temp_dir("switch-unknown");
    create_entry(&dir, "一本");
    let err = switch_active_book(&dir, "no-such-id").unwrap_err();
    assert!(err.is_code("book.not-found"), "实际 {err:?}");
}

#[test]
fn switch_rejects_already_active() {
    let dir = temp_dir("switch-active");
    create_entry(&dir, "一本");
    let second = create_entry(&dir, "二本");
    switch_active_book(&dir, &second).unwrap();
    let err = switch_active_book(&dir, &second).unwrap_err();
    assert!(err.is_code("book.already-active"), "实际 {err:?}");
}

#[test]
fn switch_rejects_unavailable_dir_and_keeps_pointer() {
    let dir = temp_dir("switch-unavailable");
    create_entry(&dir, "一本");
    let second = create_entry(&dir, "二本");
    // 用户删目录后同路径放普通文件：目录不可再创建 → 不可用。
    let second_dir = dir.join(BOOKS_DIR_NAME).join(&second);
    std::fs::remove_dir_all(&second_dir).unwrap();
    std::fs::write(&second_dir, b"not a dir").unwrap();

    let err = switch_active_book(&dir, &second).unwrap_err();
    assert!(err.is_code("book.dir-unavailable"), "实际 {err:?}");
    // 活动指针保持原账本，注册表未被改写。
    let RegistryRead::Resolved(registry) = read_registry(&dir) else {
        panic!("现场应可解析");
    };
    assert_ne!(registry.active_id, second);
}

#[test]
fn rename_updates_display_name() {
    let dir = temp_dir("rename");
    create_entry(&dir, "一本");
    let second = create_entry(&dir, "二本");

    let renamed = rename_book_entry(&dir, &second, "家庭账本").unwrap();
    assert_eq!(renamed.name, "家庭账本");
    let RegistryRead::Resolved(registry) = read_registry(&dir) else {
        panic!("改名后应可解析");
    };
    let book = registry.books.iter().find(|b| b.id == second).unwrap();
    assert_eq!(book.name, "家庭账本");
    assert_eq!(registry.active_id, registry.books[0].id); // 活动指针不受影响
}

#[test]
fn rename_rejects_blank_and_unknown() {
    let dir = temp_dir("rename-bad");
    let first = create_entry(&dir, "一本");
    let err = rename_book_entry(&dir, &first, "  ").unwrap_err();
    assert!(err.is_code("book.name-required"), "实际 {err:?}");
    let err = rename_book_entry(&dir, "no-such-id", "新名").unwrap_err();
    assert!(err.is_code("book.not-found"), "实际 {err:?}");
}

#[test]
fn remove_drops_entry_and_keeps_files() {
    let dir = temp_dir("remove");
    create_entry(&dir, "一本");
    let second = create_entry(&dir, "二本");
    let second_dir = dir.join(BOOKS_DIR_NAME).join(&second);
    std::fs::write(second_dir.join("marker.txt"), b"keep me").unwrap();

    remove_book_entry(&dir, &second).unwrap();
    let RegistryRead::Resolved(registry) = read_registry(&dir) else {
        panic!("移除后应可解析");
    };
    assert_eq!(registry.books.len(), 2);
    assert!(registry.books.iter().all(|b| b.id != second));
    // 文件保留原则：目录与其内容原样保留（生命周期归用户）。
    assert!(second_dir.is_dir());
    assert_eq!(
        std::fs::read(second_dir.join("marker.txt")).unwrap(),
        b"keep me"
    );
}

#[test]
fn remove_rejects_active_book() {
    let dir = temp_dir("remove-active");
    create_entry(&dir, "一本");
    create_entry(&dir, "二本");
    // 活动账本（默认账本）不可移除：先切走才能移除。
    let RegistryRead::Resolved(registry) = read_registry(&dir) else {
        panic!("现场应可解析");
    };
    let err = remove_book_entry(&dir, &registry.active_id).unwrap_err();
    assert!(err.is_code("book.remove-active"), "实际 {err:?}");
}

#[test]
fn remove_rejects_unknown_id() {
    let dir = temp_dir("remove-unknown");
    create_entry(&dir, "一本");
    let err = remove_book_entry(&dir, "no-such-id").unwrap_err();
    assert!(err.is_code("book.not-found"), "实际 {err:?}");
}
