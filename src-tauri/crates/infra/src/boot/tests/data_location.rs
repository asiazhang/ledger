//! [`crate::boot::data_location`] 单元测试：引导解析（注册表双格式兼容、
//! 损坏回退、文件保全）与更改意图读写。搬迁三分支的跨文件组合行为由 BDD
//! e2e 覆盖；注册表格式判定矩阵归 `book_registry` 内核单测（issue #832），
//! 此处钉引导语义。

use std::path::{Path, PathBuf};

use crate::boot::book_registry;
use crate::boot::book_registry::{BookRegistry, RegistryOrigin, RegistryRead, read_registry};
use crate::boot::data_location::*;
use crate::db::{check_integrity, open_connection};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ledger-dl-unit-{tag}-{}", crate::ids::new_uuid()));
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
            pending_relocation: None,
            origin: book_registry::RegistryOrigin::NewFormat,
        },
    )
    .unwrap();
    assert_eq!(configured_intent(&dir), Some(book_dir));
}

// -------------------------------------------------------------------------
// 账本注册表命令面支撑（issue #833）：mutable_registry 写入时机契约预检与
// gather_book_list 清单聚合。
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
    let boot = crate::boot::data_location::boot(&broken);
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

// ---------------------------------------------------------------------------
// 活动账本搬迁意图：引导消费与提交收窄（issue #836）
// ---------------------------------------------------------------------------

/// 造一本已登记账本的注册表现场工具。
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

/// 引导消费意图（分支一）：目标已有库 → 直接接管、内存态消费意图、其他账本
/// 原地不动；文件中的滞留意图由下一次登记变更自愈（引导自身不回写）。
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
    // 二次启动幂等（文件中的滞留意图按「目标已有库」再度消费）。
    let again = boot(&dir);
    assert_eq!(again.db_dir, target);
}

/// 引导消费意图（分支二）：目标为空而来源有库 → 整库搬迁（`VACUUM INTO`），
/// 数据就位目标、来源原样保留、其他账本不动；清单（文件态）显示账本在目标。
#[test]
fn boot_pending_relocation_moves_db_via_vacuum() {
    let dir = temp_dir("pending-move");
    let source = dir.join("source");
    let target = dir.join("target");
    let other = dir.join("other");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    // 来源库：经产品建连迁移入口造真实库文件（建连迁移本就是引导的既有步骤），
    // 保证 VACUUM INTO 产物可校验。
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
    // 目标库可打开且完整性通过（VACUUM INTO 产物）。
    check_integrity(&open_connection(target.join(DB_FILE_NAME)).unwrap()).unwrap();
    assert!(source.join("ledger.db").is_file(), "来源库原样保留");
    assert!(!other.join("ledger.db").exists(), "其他账本不动");
}

/// 引导消费意图（密文库推迟）：来源为密文库时搬迁待解锁补做——以来源位置
/// 生效、意图保留、`deferred_relocation` 指向目标（与旧格式推迟同语义）。
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
    // 推迟窗口内登记信息指向来源（物理真值），意图保留待补做。
    assert_eq!(registry.active_dir(), Some(source.as_path()));
    assert_eq!(
        registry.pending_relocation,
        Some(book_registry::PendingRelocation {
            book_id: "a".into(),
            from_dir: source.clone()
        })
    );
}

/// 引导消费意图（搬迁失败）：以来源位置生效并携带回退警示，意图保留待下次
/// 启动重试（与旧格式失败逐次重试同语义）。
#[test]
fn boot_pending_relocation_failure_falls_back_to_source_with_reason() {
    let dir = temp_dir("pending-fail");
    let source = dir.join("source");
    let target = dir.join("target");
    std::fs::create_dir_all(&source).unwrap();
    // 来源库损坏（非页对齐杂讯）：搬迁失败。
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
    // 目标目录不被写入任何库文件；来源损坏文件原样。
    assert!(!target.join("ledger.db").exists());
    assert_eq!(file_bytes(&source.join("ledger.db")), b"garbage-not-a-db");
}

/// 引导只消费匹配当前活动账本的意图；不匹配的滞留意图被忽略（无害滞留，
/// 留待登记变更自愈）。
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

/// 提交收窄（issue #836）：更改位置 = 活动账本登记目录改指目标 + 意图落盘，
/// 旧格式指针原地升级为新格式；下次启动只搬活动账本，其他账本原地不动。
#[test]
fn validate_and_commit_writes_pending_intent_for_active_book() {
    let dir = temp_dir("commit-narrow");
    let legacy_dir = dir.join("legacy-custom");
    let target = dir.join("new-place");
    let other = dir.join("other-book");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    // 活动账本的库（真实库文件）在旧位置；其他账本的库用占位字节。
    drop(crate::db::open_db_in(&legacy_dir).unwrap());
    std::fs::write(other.join("ledger.db"), b"other").unwrap();
    // 旧格式指针（存量安装形态）。
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

    // 意图落盘为新格式：活动账本（折叠默认账本）目录 = 目标，意图携带来源。
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

    // 下次启动：只搬活动账本；登记信息按文件真值。
    let boot = boot(&dir);
    assert_eq!(boot.db_dir, target);
    assert_eq!(boot.fallback_reason, None);
    // 目标库是活动账本数据的真实副本（可打开、完整性通过）。
    check_integrity(&open_connection(target.join("ledger.db")).unwrap()).unwrap();
    assert!(legacy_dir.join("ledger.db").is_file(), "原位置库保留");
    assert_eq!(file_bytes(&other.join("ledger.db")), b"other");
}

/// 提交幂等：目标即当前登记目录 → 已提交；引导文件同位重写为当前语义，
/// 维持「提交成功 ⇒ 引导文件已配置」不变量（与旧指针同位重写同语义），
/// 且不产生搬迁意图。
#[test]
fn validate_and_commit_is_noop_when_target_is_current_dir() {
    let dir = temp_dir("commit-noop");
    let target = dir.join("books").join("m");
    std::fs::create_dir_all(&target).unwrap();
    write_registry_with_books(&dir, &[("m", "默认账本", &target)], "m");

    let outcome = validate_and_commit(&dir, &target, false).unwrap();
    assert!(outcome.committed);
    // 同位重写：配置状态维持为当前目录，无待重启意图。
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

/// 目录唯一性：目标已登记为其他账本 → 拒绝（同一目录不得登记两个账本）。
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

/// 既有逃生舱保持：注册表损坏时更改位置视同出厂折叠继续提交（覆盖损坏文件
/// 正是该逃生舱的职责，与旧指针写入时代行为对齐）。
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
            from_dir: dir.clone(),
        })
    );
}

/// 二选一优先于任何写入：目标已有库且未选择接管 → 需要二选一，注册表不动。
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
