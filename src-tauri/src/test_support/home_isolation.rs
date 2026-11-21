use std::sync::Once;

use ledger_infra::db::new_uuid;

static ISOLATE: Once = Once::new();

/// SAFETY：`set_var` 自 Rust 2024 起 unsafe；本函数是测试夹具唯一的环境变量
pub fn isolate_home() {
    ISOLATE.call_once(|| {
        let root = std::env::temp_dir().join(format!("ledger-test-home-{}", new_uuid()));
        std::fs::create_dir_all(root.join("data")).expect("创建隔离根 data 子目录");
        std::fs::create_dir_all(root.join("config")).expect("创建隔离根 config 子目录");
        // SAFETY：见函数文档。
        unsafe {
            std::env::set_var("HOME", &root);
            std::env::set_var("XDG_DATA_HOME", root.join("data"));
            std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        }
    });
}
