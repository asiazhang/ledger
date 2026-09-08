//! data_location 单元测试：引导解析（注册表双格式兼容、损坏回退、文件保全）
//! 与更改意图读写。搬迁三分支的跨文件组合行为由 BDD e2e 覆盖；注册表格式
//! 判定矩阵归 `book_registry` 内核单测（issue #832），此处钉引导语义。

use super::*;

fn temp_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("ledger-dl-unit-{tag}-{}", super::super::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

// ---------------------------------------------------------------------------
// 引导解析（boot）
// ---------------------------------------------------------------------------

#[test]
fn boot_missing_registry_uses_default_dir_as_factory_default_book() {
    let dir = temp_dir("boot-missing");
    let boot = boot(&dir);
    assert_eq!(boot.db_dir, dir);
    assert_eq!(boot.fallback_reason, None);
    assert_eq!(boot.deferred_relocation, None);
    // 出厂形态：唯一默认账本位于默认目录，兼活动账本。
    let registry = boot.registry.expect("出厂登记信息应可用");
    assert_eq!(registry.origin, RegistryOrigin::LegacyPointer);
    assert_eq!(registry.books.len(), 1);
    assert_eq!(registry.active_dir(), Some(dir.as_path()));
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
    // 兼容读取零副作用：两个库文件与引导文件全部原样。
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
    assert_eq!(boot.db_dir, dir);
    let reason = boot.fallback_reason.expect("损坏应携带回退原因");
    assert!(!reason.is_empty());
    assert!(
        boot.registry.is_none(),
        "损坏时登记信息不可用（写入被禁止）"
    );
    // 回退零副作用：既有库文件原样，目录无残留。
    assert_eq!(file_bytes(&dir.join("ledger.db")), b"payload");
    assert_eq!(dir_listing(&dir), vec![POINTER_FILE_NAME, "ledger.db"]);
}

#[test]
fn boot_new_format_registry_activates_registered_dir() {
    let dir = temp_dir("boot-new");
    let book_a = dir.join("books").join("a");
    std::fs::create_dir_all(&book_a).unwrap();
    std::fs::write(book_a.join("ledger.db"), b"payload-a").unwrap();
    let book_b = dir.join("books").join("b"); // 目录尚不存在：新账本登记后由引导自动创建
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
    // 新格式绝不从默认目录搬迁：book_b 只被创建目录，不出现库文件；book_a 原样。
    assert_eq!(file_bytes(&book_a.join("ledger.db")), b"payload-a");
    assert!(!book_b.join("ledger.db").exists());
    assert!(!dir.join("ledger.db").exists());
}

#[test]
fn boot_new_format_never_relocates_default_dir_db_into_empty_active_book() {
    // 回归钉子（issue #832）：默认目录的库属于默认账本。新格式下活动账本目录
    // 为空时，绝不能把默认目录的库 VACUUM 进去（那会把一本账复制成两本）。
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
    // 用普通文件占住父路径，使活动账本目录无法创建。
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
    assert_eq!(boot.db_dir, dir);
    let reason = boot.fallback_reason.expect("目录不可用应携带回退原因");
    assert!(reason.contains("目标目录不可用"), "实际 {reason}");
    // 注册表本身可读：登记信息仍可展示（用户可切回默认账本脱困）。
    let registry = boot.registry.expect("可读注册表的登记信息应保留");
    assert_eq!(registry.origin, RegistryOrigin::NewFormat);
    assert_eq!(boot.deferred_relocation, None);
    // 回退零副作用：既有库文件原样、占位文件原样、目录无残留。
    assert_eq!(file_bytes(&dir.join("ledger.db")), b"payload");
    assert_eq!(file_bytes(&occupied), b"not a dir");
    assert_eq!(
        dir_listing(&dir),
        vec![POINTER_FILE_NAME, "ledger.db", "occupied"]
    );
}

#[test]
fn boot_deferred_legacy_relocation_reports_source_dir_in_registry() {
    // 密文库搬迁推迟窗口（issue #570 / ADR-0075 决策 7）：引导以源库位置生效，
    // 登记信息同步指向源目录（物理真值，仅内存改写、不落盘）。
    let dir = temp_dir("boot-deferred");
    // 页对齐、非明文魔数 = 真密文库形态（启动搬迁的既有实用判别）。
    std::fs::write(dir.join("ledger.db"), vec![0x7bu8; 4096]).unwrap();
    let target = dir.join("custom");
    std::fs::write(
        dir.join(POINTER_FILE_NAME),
        serde_json::json!({ "data_dir": target.to_string_lossy() }).to_string(),
    )
    .unwrap();

    let boot = boot(&dir);
    assert_eq!(boot.db_dir, dir);
    assert_eq!(boot.deferred_relocation, Some(target));
    assert_eq!(boot.fallback_reason, None, "推迟搬迁不是回退，无警示");
    let registry = boot.registry.expect("推迟窗口登记信息应可用");
    assert_eq!(registry.active_dir(), Some(dir.as_path()));
    // 密文源库原样保留。
    assert_eq!(file_bytes(&dir.join("ledger.db")).len(), 4096);
}

// ---------------------------------------------------------------------------
// 注册表/指针文件读写
// ---------------------------------------------------------------------------

#[test]
fn pointer_roundtrip_and_missing_means_unconfigured() {
    let dir = temp_dir("roundtrip");
    // 缺失 → 未配置。
    assert!(matches!(read_registry(&dir), RegistryRead::Unconfigured));
    // 写入（旧格式搬迁意图）→ 读作唯一默认账本。
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
    // 损坏 JSON。
    std::fs::write(dir.join(POINTER_FILE_NAME), "{not json").unwrap();
    assert!(matches!(read_registry(&dir), RegistryRead::Corrupt(_)));
    // 缺字段 / 空路径同样视同损坏。
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
    // 二次写入应整体替换，不残留临时文件。
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".data_location.json."))
        .collect();
    assert!(leftovers.is_empty(), "不应残留指针临时文件: {leftovers:?}");
    // 更改位置意图保持旧格式的单字段落盘（新格式只随登记变更写入，见内核）。
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(POINTER_FILE_NAME)).unwrap())
            .unwrap();
    assert_eq!(raw["data_dir"], b.to_string_lossy().to_string());
    assert!(raw.get("books").is_none());
}

#[test]
fn configured_intent_maps_all_pointer_states() {
    let dir = temp_dir("intent");
    // 缺失 → None。
    assert!(configured_intent(&dir).is_none());
    // 损坏 → None（回退警示由 boot 结果承载，此处只看意图）。
    std::fs::write(dir.join(POINTER_FILE_NAME), "{broken").unwrap();
    assert!(configured_intent(&dir).is_none());
    // 旧格式已配置 → Some(意图目录)。
    let target = dir.join("elsewhere");
    write_pointer(&dir, &target).unwrap();
    assert_eq!(configured_intent(&dir), Some(target.clone()));
    // 新格式 → Some(活动账本目录)。
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
            origin: book_registry::RegistryOrigin::NewFormat,
        },
    )
    .unwrap();
    assert_eq!(configured_intent(&dir), Some(book_dir));
}

// -------------------------------------------------------------------------
// 账本注册表命令面支撑（issue #833）：mutable_registry 写入时机契约预检与
// gather_book_list_from_boot 清单聚合。
// -------------------------------------------------------------------------

#[test]
fn mutable_registry_follows_write_timing_contract() {
    // 三个禁因各自稳定码化（ADR-0050 一条件一码）。
    // 未登记（极端时序）→ registry-unavailable。
    let err = mutable_registry(None).unwrap_err();
    assert!(err.is_code("book.registry-unavailable"), "实际 {err:?}");
    // 注册表损坏（registry None）→ registry-corrupt，原因随参数携带。
    let corrupt = Boot {
        db_dir: PathBuf::from("/data/default"),
        fallback_reason: Some("损坏".into()),
        deferred_relocation: None,
        registry: None,
    };
    let err = mutable_registry(Some(&corrupt)).unwrap_err();
    assert!(err.is_code("book.registry-corrupt"), "实际 {err:?}");
    // 推迟搬迁窗口 → registry-busy（registry Some 也不放行）。
    let mut deferred = corrupt.clone();
    deferred.registry = Some(BookRegistry::single_default(Path::new("/data/default")));
    deferred.db_dir = PathBuf::from("/data/default");
    deferred.deferred_relocation = Some(PathBuf::from("/data/target"));
    let err = mutable_registry(Some(&deferred)).unwrap_err();
    assert!(err.is_code("book.registry-busy"), "实际 {err:?}");
    // 可读且无搬迁窗口 → 放行。
    let mut ready = deferred;
    ready.deferred_relocation = None;
    assert!(mutable_registry(Some(&ready)).is_ok());
}

#[test]
fn gather_book_list_follows_boot_states() {
    use book_registry::RegistryRead;

    // 可读注册表：清单 + 活动指针 + 可变。
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
            origin: book_registry::RegistryOrigin::NewFormat,
        },
    )
    .unwrap();
    let boot = super::boot(&dir);
    let info = gather_book_list(&dir, Some(&boot));
    assert_eq!(info.books.len(), 1);
    assert_eq!(info.active_id.as_deref(), Some("m"));
    assert!(info.mutable);
    assert!(info.fallback_reason.is_none());

    // 登记变更落盘后清单立即可见（现场权威，不被引导快照留在旧态）：
    // 快照仍是引导时的单本，现场已有两本，清单应报两本。
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

    // 损坏注册表：清单不可信（空 + 不可变），回退原因随行。
    let broken = temp_dir("list-broken");
    std::fs::write(broken.join(POINTER_FILE_NAME), "{broken").unwrap();
    let boot = super::boot(&broken);
    let info = gather_book_list(&broken, Some(&boot));
    assert!(info.books.is_empty());
    assert_eq!(info.active_id, None);
    assert!(!info.mutable);
    assert!(info.fallback_reason.is_some());

    // 推迟搬迁窗口（直接构造引导态，聚合只消费 Boot 字段）：清单可展示，但变更被禁。
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

    // 未登记（极端时序）：现场可读则清单照常，但不可变。
    let info = gather_book_list(&dir, None);
    assert_eq!(info.books.len(), 2);
    assert!(!info.mutable);
}
