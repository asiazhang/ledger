//! data_location 单元测试：指针文件读写与损坏输入（搬迁三分支由 BDD e2e 覆盖）。

use super::*;

fn temp_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("ledger-dl-unit-{tag}-{}", super::super::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn pointer_roundtrip_and_missing_means_unconfigured() {
    let dir = temp_dir("roundtrip");
    // 缺失 → 未配置。
    assert!(matches!(read_pointer(&dir), PointerRead::Unconfigured));
    // 写入 → 原样读回。
    let target = dir.join("somewhere");
    write_pointer(&dir, &target).unwrap();
    match read_pointer(&dir) {
        PointerRead::Configured(p) => assert_eq!(p, target),
        other => panic!("应读回已配置意图，实际 {other:?}"),
    }
}

#[test]
fn corrupt_pointer_is_normal_input() {
    let dir = temp_dir("corrupt");
    // 损坏 JSON。
    std::fs::write(dir.join(POINTER_FILE_NAME), "{not json").unwrap();
    assert!(matches!(read_pointer(&dir), PointerRead::Corrupt(_)));
    // 缺字段 / 空路径同样视同损坏。
    std::fs::write(dir.join(POINTER_FILE_NAME), "{}").unwrap();
    assert!(matches!(read_pointer(&dir), PointerRead::Corrupt(_)));
    std::fs::write(dir.join(POINTER_FILE_NAME), r#"{"data_dir": "  "}"#).unwrap();
    assert!(matches!(read_pointer(&dir), PointerRead::Corrupt(_)));
}

#[test]
fn pointer_write_is_atomic_replace() {
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
    match read_pointer(&dir) {
        PointerRead::Configured(p) => assert_eq!(p, b),
        other => panic!("应读回最新意图，实际 {other:?}"),
    }
}
