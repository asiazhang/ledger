//! NSH 原生壳骨架入口（issue #2017 / ADR-0145）：薄启动——解析账本目录、
//! 注入装配入口、拉起 Linux 平台后端窗口（lib 持全部逻辑与测试）。
//!
//! 立桩期仅 Linux（ADR-0145 硬伤清单的战场）；macOS / Windows 随迁移系列票
//! 逐平台开门（`compile_error` 显式拒绝，不静默编出空壳）。

#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
fn main() -> impl std::process::Termination {
    // 跨账本注册表数据根（#2055）：与旧壳同一数据根（XDG 拼 identifier，
    //Linux 平台语义）；解析失败不阻断启动（汇总读取码化拒绝同码呈现）。
    // 先于日志初始化（#2077）：日志文件面的目录解析消费数据根槽位，槽位未
    // 注入时文件面降级为仅 stderr（接线顺序由 src/tests.rs 源码扫描钉住）。
    if let Some(dir) = nsh_app::cross_book::default_data_dir_from_env() {
        nsh_app::cross_book::set_default_data_dir(dir);
    }
    // 域栈日志面（tracing 基座，ADR-0006；crate::logging 单点）：stderr +
    // 日滚文件双输出（数据根下 logs 目录，按天滚动 7 天保留）+ RUST_LOG 过滤
    //（显式优先，缺省 info），可重载滤镜登记后设置卡调档立即生效、库就绪后
    // 按持久化档位接管。重复初始化（如嵌入测试宿主）不视为失败。
    nsh_app::logging::init();
    // 启动参数：账本目录（身份即目录，内含固定名 ledger.db，ADR-0089）。缺参
    // 不退进程——窗口即呈现面（用法提示见 app::secondary_text）。UI 组件集走
    // 平台缺省（nsh_base registry，含主题双态令牌）。
    if let Some(dir) = std::env::args().nth(1).map(PathBuf::from) {
        nsh_app::app::set_book_dir(dir);
    }
    // Linux 平台后端（GTK4 原生控件）：Rust 模式根组件直调域栈，无 JS VM。
    use nsh_platform_linux as platform;
    const APP_ID: &str = "com.zhangheng.ledger.nsh";
    // 自动备份退出兜底（issue #2078）：平台事件循环返回 = 窗口关闭的正常退出
    // 路径（nsh-platform-linux 0.4.0 无 RunEvent::Exit 等价物，裁决见
    // backup_host 模块文档）——退出前若脏且当天尚未自动备份过则补一次。
    let exit_code = platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<nsh_app::app::App>().app_id(APP_ID)),
        platform::AppOptions {
            title: "Ledger 原生壳（立桩）".to_owned(),
            app_id: APP_ID.to_owned(),
            // 交易表格首屏（issue #2018）：三列表格 + 底栏（总数/分页条）的
            // 常规桌面窗口尺寸。
            size: (1024, 720),
            ..Default::default()
        },
    );
    nsh_app::backup_host::exit_fallback();
    exit_code
}

#[cfg(not(target_os = "linux"))]
compile_error!(
    "NSH 原生壳立桩仅支持 Linux（issue #2017 / ADR-0145 硬伤清单的战场）；macOS / Windows 随迁移系列票逐平台开门"
);
