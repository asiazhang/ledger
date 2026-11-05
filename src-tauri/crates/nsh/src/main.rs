//! NSH 原生壳骨架入口（issue #2017 / ADR-0145）：薄启动——解析账本目录、
//! 注入装配入口、拉起 Linux 平台后端窗口（lib 持全部逻辑与测试）。
//!
//! 立桩期仅 Linux（ADR-0145 硬伤清单的战场）；macOS / Windows 随迁移系列票
//! 逐平台开门（`compile_error` 显式拒绝，不静默编出空壳）。

#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
fn main() -> impl std::process::Termination {
    // 域栈日志面（tracing 基座，ADR-0006）：stderr + RUST_LOG 过滤；显式
    // RUST_LOG 优先，缺省 info。重复初始化（如嵌入测试宿主）不视为失败。
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
    // 启动参数：账本目录（身份即目录，内含固定名 ledger.db，ADR-0089）。缺参
    // 不退进程——窗口即呈现面（用法提示见 app::secondary_text）。UI 组件集走
    // 平台缺省（nsh_base registry，含主题双态令牌）。
    if let Some(dir) = std::env::args().nth(1).map(PathBuf::from) {
        nsh_app::app::set_book_dir(dir);
    }
    // Linux 平台后端（GTK4 原生控件）：Rust 模式根组件直调域栈，无 JS VM。
    use nsh_platform_linux as platform;
    const APP_ID: &str = "com.zhangheng.ledger.nsh";
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<nsh_app::app::App>().app_id(APP_ID)),
        platform::AppOptions {
            title: "Ledger 原生壳（立桩）".to_owned(),
            app_id: APP_ID.to_owned(),
            size: (520, 360),
            ..Default::default()
        },
    )
}

#[cfg(not(target_os = "linux"))]
compile_error!(
    "NSH 原生壳立桩仅支持 Linux（issue #2017 / ADR-0145 硬伤清单的战场）；macOS / Windows 随迁移系列票逐平台开门"
);
