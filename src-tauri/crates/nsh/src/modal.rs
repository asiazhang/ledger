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
    // 主题令牌（双态自适应；令牌缺失回退亮灰/中灰可读兜底，app.rs 先例同形）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());
    when(open, move || {
        overlay()
            .inset_0()
            // 遮罩层：半透明黑，遮罩点击不关（吞掉点击，#252 弹层关闭语义）。
            .app_class("bg-black/50")
            .flex_col()
            .items_center()
            .justify_center()
            .on_press(move |_: &PressEvent| {})
            // ESC（弹层关闭语义）→ 关闭。
            .on_cancel(on_close)
            .child(
                view()
                    .test_id(MODAL_ID)
                    .flex_col()
                    .gap_3()
                    .w(px(420.0))
                    .app_class("bg-background rounded-md border border-border shadow-lg p-4")
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                text(title())
                                    .text_size(px(16.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(fg)
                                    .test_id(MODAL_TITLE_ID),
                            )
                            .child(
                                button()
                                    .accessibility_label("关闭")
                                    .app_class(
                                        "px-2 py-0.5 rounded-md text-muted-foreground hover:bg-accent",
                                    )
                                    .child(text("✕").text_size(px(13.0)).text_color(muted).test_id(MODAL_CLOSE_ID))
                                    .on_press(on_close),
                            ),
                    )
                    .child(content()),
            )
    })
}
