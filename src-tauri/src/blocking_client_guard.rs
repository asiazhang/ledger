//! 阻塞客户端禁令守门（ADR-0125 决策 8，issue #1414）：**生产面
//! `reqwest::blocking` 零命中**。
//!
//! 为什么守门：行情域 HTTP 面已随 #1411 / #1413 换 reqwest async 客户端，阻塞
//! 客户端（自持运行时线程，构造与每次请求等待都检查调用线程的运行时状态）整体
//! 退役——同一能力留两种形态，纪律就要守两遍（ADR-0125 决策 9）。回潮一处就把
//! #1403 现场类缺陷（异步上下文内构造/析构即 panic，且 debug 断言只在 dev 构建
//! 存在、生产分支测试不可达时退化不可见）带回来；行为测试看不见「某文件里多了
//! 一行构造」，只有源码扫描即红（先例：连接槽独占守门 `db_slot_guard`、
//! ADR-0104 决策 6 的锁仪式反向守门）。
//!
//! **判定形态**：生产文本（[`crate::sync_trigger_guard::production_text`]，掩码
//! 注释与字符串、剔除 `#[cfg(test)]` 附属体）上的 token 扫描；换行拆写或别名改写
//! 不可达，靠评审兜底——与既有 bypass 守门同款取舍（ADR-0125 决策 8 扫描根）。
//!
//! **扫描根**（ADR-0125 决策 8）：壳层源码全递归（旁路点就在壳层子目录与命令
//! 目录内）+ 全部生产 crate 源码。基础设施 `db` 区不再豁免——阻塞客户端在门面
//! 与连接层同样无合法住址，扫描面取全域更严。壳层 `test_support` 夹具模块虽仅
//! 被测试消费，但属生产编译面（`pub mod`，ADR-0060 C 类豁免声明），留在扫描面
//! 内。测试面（`tests.rs` 模块索引、`tests/` 目录、`#[cfg(test)]` 附属体、
//! `tests/` 集成测试目标）不进面：多端同步域的测试桩仍用阻塞客户端直连本地
//! S3 桩（同步域异步化 #1405 另案，迁移完成前属「测试面按需保留」）。
//!
//! **豁免**：无。生产面零命中是闭集规格，不设豁免台账；确需阻塞客户端的路径
//! 先修订 ADR-0125 再改本守门。

use crate::sync_trigger_guard::{production_text, walk_rust_sources};
use std::path::Path;

/// 阻塞客户端构造形态的 token：`reqwest::blocking` 路径段（构造点、类型标注与
/// `use` 引入都含此子串）。
const BLOCKING_CLIENT_TOKEN: &str = "reqwest::blocking";

/// 扫描根（相对 `src-tauri`）：壳层 `src/` 全递归 + 全部成员 crate 源码。
fn scan_roots() -> Vec<std::path::PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut roots = vec![root.join("src")];
    for member in crate_member_dirs() {
        roots.push(root.join("crates").join(member).join("src"));
    }
    roots
}

/// 成员 crate 目录名清单（扫描根与扫描面自检共用，排序确定）。
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

/// 测试面路径判定：`tests.rs` 模块索引与 `tests/` 目录随其 `#[cfg(test)]` 声明
/// 整体属测试面，不进扫描（声明点被 `production_text` 剔除，文件本体按路径剔除
/// ——文本扫描不跨文件追踪模块归属）。
fn is_test_face(rel: &str) -> bool {
    rel.split('/')
        .any(|seg| seg == "tests" || seg == "tests.rs")
}

/// 扫描面上的（相对 `src-tauri` 路径, 生产文本）清单。
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

/// 扫描面自检：壳层与每个成员 crate 都必须真的进面（扫描根写错即静默空扫，
/// 与既有 bypass 守门同款「拒绝以空集假绿」口径）。
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

/// 测试面剔除是真实过滤而非恒真（守门不许「扫描面吞掉已知命中」）：多端同步域
/// 的测试桩仍有阻塞客户端（#1405 另案迁移前按需保留）——该文件必须被路径剔除
/// 规则挡在面外，且文件内确有 token（两边任何一侧失守都说明判定失真）。
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

/// 判定原语自检：token 扫描打在生产文本上（掩码后代码保留、注释与字符串不误报）。
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
