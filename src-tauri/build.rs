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
        first.strip_suffix(".rs").unwrap_or(&first).to_string()
    } else {
        first
    }
}

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
