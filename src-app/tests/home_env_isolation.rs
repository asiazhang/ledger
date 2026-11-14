//! `test_support::isolate_home` 的契约测试（issue #1989；#2083 切轨随迁 NSH 形态）。
//!
//! 模拟「本机预置 `XDG_DATA_HOME` / `XDG_CONFIG_HOME`」的环境：现场隔离夹具必须
//! 在重定向 `$HOME` 的同时把两根 XDG 变量一并收进进程专属临时区——只重定向
//! `$HOME` 时壳层数据根解析（`cross_book::default_data_dir_from_env`，XDG 优先、
//! HOME 回落，与旧壳 `dirs`/`app_data_dir()` 同一优先级语义）仍会命中真实
//! `~/.local/share` 根，备份/恢复等数据根消费点即对真实用户目录写入（#1989 事故）。
//!
//! 断言对准事故的同一条解析路径（NSH 壳层的数据根解析函数）；删除夹具内的
//! XDG 变量写入即红（删除即变红）。
//!
//! 进程级环境变量是全局量：本二进制只放这一个用例，毒化写入发生在夹具 `Once`
//! 之前，进程内无第二写入点、无并行竞态。已知边界：断言面是 Linux 平台语义
//!（XDG 优先解析），与事故平台一致。

// 测试整体豁免（ADR-0060）：集成测试 crate 经 cfg(test) 放行六件套，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

use nsh_app::test_support::isolate_home;

/// 毒化 XDG 两根后调用隔离夹具：数据根解析必须落进隔离根（XDG 优先语义生效），
/// 不得命中毒化路径（命中即说明夹具漏重定向对应变量）。
#[test]
fn isolate_home_redirects_xdg_data_and_config_roots() {
    // 毒化路径带 PID 后缀（#2112）：固定名会让并发测试进程互相指到同一路径。
    let poison_data = std::env::temp_dir().join(format!(
        "ledger-test-home-poison-data-{}",
        std::process::id()
    ));
    let poison_config = std::env::temp_dir().join(format!(
        "ledger-test-home-poison-config-{}",
        std::process::id()
    ));
    // SAFETY：set_var 自 Rust 2024 起 unsafe；本写入先于夹具 Once（本进程唯一
    // 用例），毒化值只被夹具覆盖、不再被读取为真实现场。
    unsafe { std::env::set_var("XDG_DATA_HOME", &poison_data) };
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &poison_config) };

    isolate_home();

    // 事故同一条解析路径：数据根 = XDG_DATA_HOME（或 HOME 回落）+ 应用标识。
    let resolved = nsh_app::cross_book::default_data_dir_from_env()
        .expect("现场隔离后数据根应可解析（HOME 必被重定向）");
    assert!(
        resolved.starts_with(std::env::var_os("XDG_DATA_HOME").expect("XDG_DATA_HOME 应被重定向")),
        "数据根应落进隔离根的 data 子树（夹具漏重定向 XDG_DATA_HOME 即红）：{resolved:?}"
    );
    assert!(
        !resolved.starts_with(&poison_data) && !resolved.starts_with(&poison_config),
        "数据根不得命中毒化路径：{resolved:?}"
    );
}
