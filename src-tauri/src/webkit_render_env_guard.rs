//! WebKitGTK DMABUF 闪烁 workaround 的接线守门（issue #2190）：`run()` 顶部的
//! `WEBKIT_DISABLE_DMABUF_RENDERER` set_var 接线是根包源码形状事实——它必须先于
//! 任何 webview 创建生效，而集成测试不运行 `run()`，行为测试无从感知；删掉接线、
//! 挪到 Builder 装配之后、或丢掉「尊重用户已设值」的判断，只有扫描即红（同款
//! 守门先例：`sync_trigger_guard` / `db_slot_guard`，器具共用
//! [`crate::test_support::scan`]）。掩码会把字符串字面量内容清成空白且多字节
//! 注释掩码后字节长度与原文不同位，故掩码文本与原文各自独立取前缀，不做跨文本
//! 索引映射。

use crate::sync_trigger_guard::production_text;
use std::path::Path;

#[test]
fn run_wires_webkit_dmabuf_workaround_before_builder() {
    let raw = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("lib.rs"),
    )
    .expect("lib.rs 应可读");

    // Builder 装配是 run() 内第一处渲染相关构造（原文中仅代码一处命中），接线
    // 必须落在它之前——删接线或挪到其后（前缀无命中）都即红。
    let builder_at = raw
        .find("tauri::Builder::default()")
        .expect("run() 应装配 Tauri Builder");
    let prefix = &raw[..builder_at];

    // 带字面量的精确接线形状（原文可见）：变量名、取值与平台门一体断言。
    assert!(
        prefix.contains(r#"std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1")"#),
        "run() 必须在 Builder 装配前接线 WebKitGTK DMABUF workaround（删除 set_var 即红，issue #2190）"
    );
    assert!(
        prefix.contains(r#"all(target_os = "linux", not(target_os = "android"))"#),
        "接线必须收在 linux 目标门内且排除 Android（android 同属 linux 目标三元组，issue #2190）"
    );

    // 「尊重用户已设值」的判断（掩码生产文本上核对代码令牌）。
    let masked = production_text(&raw);
    assert!(
        masked[..masked
            .find("tauri::Builder::default()")
            .expect("run() 应装配 Tauri Builder")]
            .contains("var_os"),
        "接线必须尊重用户已设值（var_os 判断，不覆盖显式选择，issue #2190）"
    );
}
