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

    let builder_at = raw
        .find("tauri::Builder::default()")
        .expect("run() 应装配 Tauri Builder");
    let prefix = &raw[..builder_at];

    assert!(
        prefix.contains(r#"std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1")"#),
        "run() 必须在 Builder 装配前接线 WebKitGTK DMABUF workaround（删除 set_var 即红，issue #2190）"
    );
    assert!(
        prefix.contains(r#"all(target_os = "linux", not(target_os = "android"))"#),
        "接线必须收在 linux 目标门内且排除 Android（android 同属 linux 目标三元组，issue #2190）"
    );

    let masked = production_text(&raw);
    assert!(
        masked[..masked
            .find("tauri::Builder::default()")
            .expect("run() 应装配 Tauri Builder")]
            .contains("var_os"),
        "接线必须尊重用户已设值（var_os 判断，不覆盖显式选择，issue #2190）"
    );
}
