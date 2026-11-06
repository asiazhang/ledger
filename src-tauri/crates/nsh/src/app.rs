//! NSH 根组件与启动结果视图模型（issue #2017 / ADR-0145）。
//!
//! 装配入口可注入（docs/research/nsh-testing-capabilities.md §3.1：lib + bin
//! 双靶，bin 薄启动）：bin（`main.rs`）解析启动参数后经 [`set_book_dir`] 注入
//! 账本目录；headless 组件测试以同一入口注入测试库路径后起真实视图树
//! （`Root::of::<App>`），生产与测试同一条装配路径。
//!
//! 引导在组件任务的后台线程执行（`spawn` + `blocking`：SQLite 是阻塞 IO，不占
//! 引擎线程）；呈现按启动结果三态（加载中 / 就绪 / 失败），文案仅 zh-CN
//! （spec #2015 实施决策）。

use std::path::PathBuf;
use std::sync::OnceLock;

use nsh_core::prelude::*;

use crate::boot;

/// 启动装配入口：账本目录（bin 注入；headless 测试注入测试库路径）。
static BOOK_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 注入账本目录（幂等：先注入者优先，与接缝注册点同形）。bin 启动时调用一次；
/// 未注入时 [`App`] 呈现缺参失败态（不静默退窗，窗口即错误呈现面）。
pub fn set_book_dir(dir: PathBuf) {
    let _ = BOOK_DIR.set(dir);
}

/// 启动结果三态（窗口呈现的全部状态）。
#[derive(Clone, Debug, PartialEq)]
pub enum StartupView {
    /// 未指定账本目录（bin 缺参）：呈现用法。
    MissingArg,
    /// 引导中（域栈在后台线程打开账本库）。
    Loading,
    /// 就绪：交易总数（读路径直调域服务的立桩靶标）。
    Ready { total: i64 },
    /// 引导失败：码化错误的用户可见文案（zh-CN 原样呈现）。
    Failed { message: String },
}

/// 执行启动竖井（引导域栈 → 读交易总数），返回窗口呈现态。
///
/// 阻塞（SQLite IO）；组件内在 `blocking` 任务里调用。会话句柄（`DbState`）
/// 随计数完成即释放——立桩骨架只消费总数，列表页立桩票（spec #2015 后续票）
/// 重构会话生命周期。
pub fn run_startup() -> StartupView {
    let Some(book_dir) = BOOK_DIR.get().cloned() else {
        return StartupView::MissingArg;
    };
    match boot::boot_ledger(&book_dir).and_then(|db| boot::transaction_total(&db)) {
        Ok(total) => StartupView::Ready { total },
        Err(e) => StartupView::Failed {
            message: e.to_string(),
        },
    }
}

/// 主行文案（就绪 = 交易总数，立桩靶标；`text:` 选择器与 headless 断言锚点）。
pub fn headline_text(view: &StartupView) -> String {
    match view {
        StartupView::MissingArg => "未指定账本库目录".to_owned(),
        StartupView::Loading => "正在打开账本库…".to_owned(),
        StartupView::Ready { total } => format!("交易总数：{total} 条"),
        StartupView::Failed { message } => format!("启动失败：{message}"),
    }
}

/// 次行文案（用法提示 / 账本目录回显，验证立桩的可见凭据）。
pub fn secondary_text(view: &StartupView) -> String {
    match view {
        StartupView::MissingArg => "用法：nsh-app <账本目录>".to_owned(),
        _ => match BOOK_DIR.get() {
            Some(dir) => dir.display().to_string(),
            None => String::new(),
        },
    }
}

/// 立桩根组件：居中呈现启动结果（三态主行 + 目录回显次行）。
#[component]
pub fn App() -> impl IntoElement {
    let state = signal(StartupView::Loading);
    // 主行/次行前景取主题令牌（双态自适应，registry 缺省带主题双态令牌）：
    // 裸 text 的默认前景是黑（ResolvedTextStyle::default 的 React Native
    // 惯例），暗色桌面上黑字贴暗底不可见——窗口文字必须显式带色。令牌缺失
    // 时回退亮灰/中灰（缺省主题 registry 缺失时的可读兜底，非双态）。
    let tokens = nsh_core::element::tokens();
    tracing::debug!(
        foreground = ?tokens.color("foreground"),
        muted = ?tokens.color("muted-foreground"),
        "启动视图主题令牌"
    );
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());
    spawn(async move {
        // blocking 的 JoinHandle：启动竖井不 panic（失败走 Result）；JoinError 仅在
        // 任务异常退出时出现，归入失败态呈现（不 panic 引擎线程）。
        let outcome = blocking(run_startup)
            .await
            .unwrap_or_else(|_| StartupView::Failed {
                message: "内部错误：启动任务异常退出".to_owned(),
            });
        state.set(outcome);
    });
    view()
        .flex_1()
        .items_center()
        .justify_center()
        .gap_2()
        .child(
            text(move || headline_text(&state.get()))
                .text_size(px(24.0))
                .text_color(fg)
                .test_id("startup-headline"),
        )
        .child(
            text(move || secondary_text(&state.get()))
                .text_size(px(12.0))
                .text_color(muted),
        )
}
