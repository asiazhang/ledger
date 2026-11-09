//! 进程级现场隔离（issue #1989）：`$HOME` 与 XDG 两根重定向的仓库测试面单点。
//!
//! mock runtime 的 `app_data_dir()` / `app_config_dir()` 实现为 `dirs` 的
//! `data_dir()` / `config_dir()` 拼接 identifier：Linux 下 `data_dir()` 优先解析
//! `$XDG_DATA_HOME`（config 同理 `$XDG_CONFIG_HOME`），只重定向 `$HOME` 挡不住
//! ——本机设置了 XDG 变量时，集成测试的备份/恢复与默认数据目录写入即落在真实
//! `~/.local/share` 根（#1989 事故）。三根一齐重定向进进程专属临时区；调用点
//! 只学「测试函数首行先调 [`isolate_home`]」，现场文件操作发生在后。
//!
//! `Once` 语义（收编自 `tests/commands/isolation.rs` 等六处副本，CI 偶发红根因）：
//! 环境变量是进程级全局量，cargo 把同一测试目标的并行用例编进同一进程——写入点
//! 必须唯一且至多执行一次，否则并行时同一用例的两次目录解析（夹具取现场、随后
//! 命令取数据目录）会被对方插在中间，登记目录与引导目录分属两棵临时树。收编后
//! 静态住本模块、每测试二进制恰有一次写入，竞态无从发生。
//!
//! 隔离根不随用例删除：`$HOME` 由进程存活期引用，进程内删除即悬垂，不属暂存
//! 目录器具（[`super::scratch`]，其模块文档同此边界登记）；前缀仍归
//! `ledger-test-` 闭集，残留可一条 `rm -rf /tmp/ledger-test-*` 识别清理。
//! 契约测试住 `tests/home_env_isolation.rs`（毒化 XDG 两根 → 断言解析收进
//! 隔离根；删除 XDG 写入即红）。

use std::sync::Once;

use ledger_infra::db::new_uuid;

static ISOLATE: Once = Once::new();

/// 现场隔离（进程内一次）：`$HOME` + `$XDG_DATA_HOME` + `$XDG_CONFIG_HOME` 一齐
/// 重定向进 `temp_dir()/ledger-test-home-{uuid}/`（`data` / `config` 各设子目录，
/// 与真实目录布局同构）。
///
/// SAFETY：`set_var` 自 Rust 2024 起 unsafe；本函数是测试夹具唯一的环境变量
/// 写入点（契约测试先于本 `Once` 毒化 XDG 两根，是该禁令的显式登记例外），
/// `Once` 保证进程内至多执行一次，写入后值不再变化。
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
