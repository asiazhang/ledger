use crate::sync_trigger_guard::{production_text, walk_rust_sources};
use std::path::Path;

const BLOCKING_CLIENT_TOKEN: &str = "reqwest::blocking";

fn scan_roots() -> Vec<std::path::PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut roots = vec![root.join("src")];
    for member in crate_member_dirs() {
        roots.push(root.join("crates").join(member).join("src"));
    }
    roots
}

fn crate_member_dirs() -> Vec<String> {
    let mut members: Vec<String> =
        std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("crates"))
            .expect("crates 目录应可读")
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
    members.sort();
    members
}

fn is_test_face(rel: &str) -> bool {
    rel.split('/')
        .any(|seg| seg == "tests" || seg == "tests.rs")
}

fn production_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for root in scan_roots() {
        walk_rust_sources(&root, &mut out);
    }
    out.into_iter()
        .filter(|(rel, _)| !is_test_face(rel))
        .map(|(rel, text)| (rel, production_text(&text)))
        .collect()
}

#[test]
fn production_face_has_no_blocking_client() {
    let mut hits: Vec<(String, usize)> = Vec::new();
    for (rel, masked) in production_sources() {
        let count = masked.matches(BLOCKING_CLIENT_TOKEN).count();
        if count > 0 {
            hits.push((rel, count));
        }
    }
    assert!(
        hits.is_empty(),
        "生产面出现阻塞客户端（{hits:?}）——reqwest 阻塞客户端自持运行时线程，\
         在异步上下文内构造/析构即 panic（#1403 现场，ADR-0125 决策 8 阻塞客户端禁令）。\
         行情 HTTP 面一律用 reqwest async 客户端（issue #1411）；\
         确需阻塞形态先修订 ADR-0125 再改本守门"
    );
}

#[test]
fn scan_surface_covers_shell_and_every_crate() {
    let sources = production_sources();
    assert!(
        sources.iter().any(|(rel, _)| rel == "src/lib.rs"),
        "扫描面应含壳层源码（src/**）"
    );
    let member_dirs = crate_member_dirs();
    for member in &member_dirs {
        assert!(
            sources
                .iter()
                .any(|(rel, _)| rel.starts_with(&format!("crates/{member}/src/"))),
            "成员 crate {member} 在扫描面上零文件——扫描根或过滤写错即静默漏检"
        );
    }
    assert!(
        sources.len() > 200,
        "扫描面过小（{} 个文件）——扫描根或过滤写错即静默漏检",
        sources.len()
    );
}

#[test]
fn known_test_face_hit_stays_out_of_the_surface() {
    let test_file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/sync-engine/src/tests/transport_s3.rs");
    let text = std::fs::read_to_string(&test_file)
        .expect("同步域 S3 传输测试应可读（路径漂移即同步更新本守门）");
    assert!(
        text.contains(BLOCKING_CLIENT_TOKEN),
        "已知测试面命中点不再含 {BLOCKING_CLIENT_TOKEN}——#1405 迁移完成后删除本断言"
    );
    assert!(
        is_test_face("crates/sync-engine/src/tests/transport_s3.rs"),
        "测试面路径判定失真——tests/ 目录必须被剔除，否则测试桩命中会误伤生产面守门"
    );
}

#[test]
fn token_scan_targets_masked_production_text() {
    use crate::sync_trigger_guard::production_text as mask;
    let code = "fn f() { let c = reqwest::blocking::Client::new(); }";
    assert!(
        mask(code).contains(BLOCKING_CLIENT_TOKEN),
        "掩码必须保留真实代码上的 token，否则守门漏检"
    );
    let commented = "// reqwest::blocking::Client::new()";
    assert!(
        !mask(commented).contains(BLOCKING_CLIENT_TOKEN),
        "注释里的 token 必须被掩码，否则文档性提及误报"
    );
}
