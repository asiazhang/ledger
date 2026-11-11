//! 模态弹窗封装（issue #2033；`App*` 封装族，ADR-0118 纪律的 NSH 延续——旧壳
//! `AppModal` 薄封装的 NSH 对应件，视图不直连框架基元）。
//!
//! 弹层纪律继承旧壳 AppModal（issue #251 / #252 弹层关闭语义）：遮罩点击不关
//! （遮罩吞掉点击，不构成关闭意图）、ESC 可关、✕ / 取消 / 保存等显式动作由
//! 调用方经 [`on_close`] 关闭；开/关上报 Overlay Suppression 注册表
//! （[`crate::overlay`]，快捷键抑制零新机制，ADR-0035）——每实例一个 token，
//! effect 追踪受控 open 信号（全关闭路径覆盖），组件卸载时 Drop 自动撤销。
//!
//! 呈现面（应用内模态）：遮罩层内居中卡片（标题行 + 内容槽），宽度取旧壳
//! cardSize sm 档（420px，编辑类弹窗；预算编辑弹窗为本封装首个消费面）。
//! 标题与内容全部由调用方传入（调用方持词表），本封装零业务语义。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::button;
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::design::font_size;
use crate::overlay;

/// 弹层内容锚点（headless `id:` 选择器用）。
pub(crate) const MODAL_ID: &str = "app-modal";
/// 标题锚点。
pub(crate) const MODAL_TITLE_ID: &str = "app-modal-title";
/// ✕ 关闭按钮锚点。
pub(crate) const MODAL_CLOSE_ID: &str = "app-modal-close";

/// 模态弹窗：`open` 是受控开合（读闭包，追踪信号），关闭意图（✕ / ESC）经
/// `on_close` 回给调用方（调用方负责关窗——置回 None——与执行后续动作）。
/// 遮罩不关、ESC 可关、开/关上报抑制注册表。
pub fn app_modal(
    open: impl Fn() -> bool + Copy + 'static,
    title: impl Fn() -> String + Copy + 'static,
    on_close: impl Fn() + Copy + 'static,
    content: impl Fn() -> View + Copy + 'static,
) -> impl IntoElement {
    // 开/关上报（Overlay Suppression 注册表单一通道：追踪受控开合，全路径
    // 覆盖——ESC / ✕ / 调用方置关三条关闭路径都走信号回写）；组件卸载时 Drop
    // 自动撤销（卸载兜底，与期间弹层同形）。
    let token = overlay::OverlayToken::new();
    effect(move || {
        let shown = open();
        token.report(shown);
    });
    // 主题令牌（双态自适应）：色值住 theme.json 令牌表，读取经
    // [`crate::theme::color`] 单点，缺失落可读兜底——本模块零本地色常量
    // （issue #2107 视觉收口）。
    let fg = crate::theme::color("foreground");
    let muted = crate::theme::color("muted-foreground");
    when(open, move || {
        overlay()
            .inset_0()
            // 遮罩层：半透明黑，遮罩点击不关（吞掉点击，#252 弹层关闭语义）。
            .app_class("bg-black/50")
            // ESC（弹层关闭语义）→ 关闭。
            .on_cancel(on_close)
            .on_press(move |_: &PressEvent| {})
            .child(
                // 弹窗窄窗全屏化分支（#2057 / ADR-0088 决策 8）：<840 移动档卡
                // 片近全屏（遮罩四边呼吸边内填充，内容可滚）；桌面档保持
                // cardSize sm 档 420pt 固定宽居中。标签上置/按钮行形态由调用
                // 方内容承载（本壳表单弹窗内容本就是纵向排列，旧壳移动档
                // label-placement 的等价形态零改动）。
                when(crate::adaptive::is_narrow, move || {
                    view().flex_1().flex_col().p_3().child(
                        view()
                            .test_id(MODAL_ID)
                            .flex_col()
                            .flex_1()
                            .gap_3()
                            .app_class(
                                "bg-background rounded-(--radius) border border-border shadow-lg p-4",
                            )
                            .child(title_row(fg, muted, title, on_close))
                            .child(scroll().flex_1().flex_col().child(content())),
                    )
                })
                .otherwise(move || {
                    view()
                        .flex_1()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .child(
                            view()
                                .test_id(MODAL_ID)
                                .flex_col()
                                .gap_3()
                                .w(px(420.0))
                                .app_class(
                                    "bg-background rounded-(--radius) border border-border shadow-lg p-4",
                                )
                                .child(title_row(fg, muted, title, on_close))
                                .child(content()),
                        )
                }),
            )
    })
}

/// 弹窗标题行（标题 + ✕；两档共用——全屏化分支与桌面档同一关闭通道：✕ /
/// ESC / 调用方显式动作，遮罩不关语义不变）。
fn title_row(
    fg: AdaptiveColor,
    muted: AdaptiveColor,
    title: impl Fn() -> String + Copy + 'static,
    on_close: impl Fn() + Copy + 'static,
) -> View {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            // 弹窗标题（字阶 BODY 档，design.rs font_size 同源——DESIGN.md §3
            // 「页面/卡片标题」档）。
            text(title())
                .text_size(px(font_size::BODY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .test_id(MODAL_TITLE_ID),
        )
        .child(
            // ✕ 关闭钮（小幽灵钮：圆角半档 rounded-sm，txn_form 同形小钮；
            // 字形 = 字阶 SECONDARY 档）。
            button()
                .accessibility_label("关闭")
                .app_class("px-2 py-0.5 rounded-sm text-muted-foreground hover:bg-accent")
                .child(
                    text("✕")
                        .text_size(px(font_size::SECONDARY))
                        .text_color(muted)
                        .test_id(MODAL_CLOSE_ID),
                )
                .on_press(on_close),
        )
}
