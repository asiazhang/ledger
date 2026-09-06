//! 构建期命令注册生成（issue #315 / ADR-0047）：命令单一来源 = `#[tauri::command]` 注解本身。
//!
//! 扫描 `src/commands/**` 的裸 `#[tauri::command]` + 紧随 `pub fn` / `pub async fn`，
//! 生成两个产物（同一份扫描结果，零二次扫描）：
//! - `$(OUT_DIR)/commands_registry.rs`——按（域，命令名）字典序排列的
//!   `tauri::generate_handler![commands::<name>, ...]`，包在具名函数 `tauri_commands_handler`
//!   里（flat 路径风格，依赖 commands 扁平 pub use 链解析）。lib.rs 经 `include!` 接入，
//!   命令注册零手工清单。
//! - `$(OUT_DIR)/commands_manifest.rs`——命令名字典序清单 `IPC_COMMAND_MANIFEST`，
//!   信号交叉核对测试（`signals_cross_check`，ADR-0044 / #335）以之为 IPC 注册面真源，
//!   与 IPC 壳声明表双向比对（仅测试经 `include!` 消费）。
//!
//! 扫描维护边界：只认裸注解 + 紧随 fn 定义。带参注解（`#[tauri::command(rename_all = …)]`）、
//! cfg 条件命令、注解与 fn 之间的属性行均不支持——遇到不认识的形态直接 panic（fail loud，
//! 宁可编译失败不可静默漏注册）；未来扩展时须同步修改 `scripts/check-commands.ts`
//! （TS 调用面一致性校验与本文共享同一扫描规则）。
//
// 豁免（ADR-0060）：构建脚本 fail-loud 守门（ADR-0047）刻意用 panic!/expect 表达
// 「扫描器失灵即拒绝构建」；构建期代码不进入生产运行时，不受六件套门禁约束。
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // 只监听命令目录：生成物仅由 src/commands/** 决定（build.rs 自身变更 cargo 必然重跑）。
    println!("cargo:rerun-if-changed=src/commands");

    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR 未设置"));
    let commands = scan_commands_dir(&manifest_dir.join("src").join("commands"));
    write_registry(
        &commands,
        &PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR 未设置")),
    );

    tauri_build::build()
}

/// 扫描命令目录，返回（域，命令名）→ 定义文件的有序映射。
/// 域 = 命令目录下第一级路径段（`foo.rs` → `foo`，`foo/bar.rs` → `foo`），
/// 只用于排序稳定，不参与注册路径（注册一律 flat：`commands::<name>`）。
/// BTreeMap 同时兜底重名检测：同 (域, 名) 重复插入即 panic。
fn scan_commands_dir(dir: &Path) -> BTreeMap<(String, String), PathBuf> {
    let mut commands = BTreeMap::new();
    for file in collect_rust_files(dir) {
        let domain = domain_of(dir, &file);
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("读取命令文件失败 {}：{e}", file.display()));
        for name in scan_source(&file, &source) {
            let key = (domain.clone(), name.clone());
            if let Some(existing) = commands.insert(key, file.clone()) {
                panic!(
                    "命令名重复定义：{name}（{} 与 {}）——generate_handler 注册路径必须唯一",
                    existing.display(),
                    file.display()
                );
            }
        }
    }
    if commands.is_empty() {
        panic!(
            "未在 src/commands/** 扫描到任何 #[tauri::command]——命令目录为空或扫描器失灵，拒绝生成空注册表"
        );
    }
    commands
}

/// 逐行扫描单个源文件：裸 `#[tauri::command]` 的下一行必须是 `pub fn <name>` 或
/// `pub async fn <name>`，否则 panic（扫描边界 fail loud，见模块注释）。
fn scan_source(file: &Path, source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut armed = false;
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if armed {
            let name = trimmed
                .strip_prefix("pub async fn ")
                .or_else(|| trimmed.strip_prefix("pub fn "))
                .and_then(|rest| {
                    rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                        .next()
                })
                .filter(|n| !n.is_empty());
            match name {
                Some(name) => names.push(name.to_string()),
                None => panic!(
                    "扫描器不认识的命令形态：{}:{}：{trimmed}\n\
                     扫描边界：只认裸 #[tauri::command] + 紧随 pub fn / pub async fn；\
                     带参注解 / cfg 条件命令需同步扩展 build.rs 与 scripts/check-commands.ts 的扫描规则（ADR-0047）",
                    file.display(),
                    idx + 1
                ),
            }
            armed = false;
        } else if trimmed == "#[tauri::command]" {
            armed = true;
        }
    }
    if armed {
        panic!(
            "扫描器不认识的命令形态：{}：文件以 #[tauri::command] 结尾，其后无 fn 定义（ADR-0047）",
            file.display()
        );
    }
    names
}

/// 域 = 相对命令目录的第一级路径段；单文件（如 `ai.rs`）取文件 stem。
fn domain_of(commands_dir: &Path, file: &Path) -> String {
    let rel = file
        .strip_prefix(commands_dir)
        .expect("命令文件在命令目录之外");
    let mut components = rel.components();
    let first = components
        .next()
        .expect("空相对路径")
        .as_os_str()
        .to_string_lossy()
        .to_string();
    if components.next().is_none() {
        // 单文件：去 .rs 后缀
        first.strip_suffix(".rs").unwrap_or(&first).to_string()
    } else {
        first
    }
}

/// 递归收集目录下全部 .rs 文件（按路径排序，保证扫描顺序确定）
fn collect_rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("读取命令目录失败 {}：{e}", dir.display()))
        .map(|e| e.expect("读取目录项失败").path())
        .collect();
    entries.sort();
    entries
        .into_iter()
        .flat_map(|path| {
            if path.is_dir() {
                collect_rust_files(&path)
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                vec![path]
            } else {
                Vec::new()
            }
        })
        .collect()
}

/// 生成注册表文件：具名函数包 `tauri::generate_handler!`，按（域，命令名）字典序，
/// flat 路径与原 lib.rs 手工清单逐字同风格。生成内容不含任何机器相关路径，跨机稳定。
fn write_registry(commands: &BTreeMap<(String, String), PathBuf>, out_dir: &Path) {
    let mut code = String::from(
        "// 由 src-tauri/build.rs 生成（命令注册单一来源，ADR-0047）——请勿手改：\n\
         // 本文件是 #[tauri::command] 注解的派生物，cargo build 时自动重新生成。\n\
         // 排序键 = （域，命令名）字典序；注册路径 flat（commands::<name>），\n\
         // 依赖 src/commands/mod.rs 的扁平 pub use 链解析。\n\
         pub fn tauri_commands_handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {\n\
         \x20   tauri::generate_handler![\n",
    );
    for (_, name) in commands.keys() {
        let _ = writeln!(code, "        commands::{name},");
    }
    code.push_str("    ]\n}\n");
    fs::write(out_dir.join("commands_registry.rs"), code).expect("写 commands_registry.rs 失败");

    // 命令名清单（同一扫描结果的第二个产物）：信号声明表交叉核对（ADR-0044 / #335）
    // 的 IPC 注册面真源——声明表须与注册清单完全互等，新命令漏声明测试期即红。
    // 与注册表不同，本清单按命令名字典序排列（不含域信息，与文件头自述一致）。
    let mut names: Vec<&str> = commands.keys().map(|(_, name)| name.as_str()).collect();
    names.sort_unstable();
    let mut manifest = String::from(
        "// 由 src-tauri/build.rs 生成（ADR-0047 命令单一来源的派生物）——请勿手改：\n\
         // 全部 #[tauri::command] 命令名字典序清单，cargo build 时自动重新生成。\n\
         pub const IPC_COMMAND_MANIFEST: &[&str] = &[\n",
    );
    for name in names {
        let _ = writeln!(manifest, "    \"{name}\",");
    }
    manifest.push_str("];\n");
    fs::write(out_dir.join("commands_manifest.rs"), manifest)
        .expect("写 commands_manifest.rs 失败");
}
