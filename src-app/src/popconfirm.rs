//! 轻量确认气泡封装（issue #2033；`App*` 封装族，ADR-0118 纪律的 NSH 延续——
//! 旧壳 `AppPopconfirm` 的 NSH 对应件，视图不直连框架基元）。
//!
//! 形态：锚定触发件的气泡（说明文案 + 取消/确认两键），轻量确认语义——破坏
//! 性但轻量的操作（预算删除：软删可同步恢复、不涉不可逆损失）不升危险确认
//! 对话框（ADR-0078 分级判据的轻量档）。ESC / 点外关闭由弹层宿主承担并经
//! [`on_open_change`] 回写受控信号；确认键只回 [`on_confirm`] + 关闭，执行
//! 归调用方。
//!
//! 弹层纪律：开/关上报 Overlay Suppression 注册表（[`crate::overlay`]，每实例
//! 一个 token；effect 追踪受控 open 信号，全关闭路径覆盖），组件卸载时 Drop
//! 自动撤销；文案全部由调用方传入（调用方持词表），本封装零业务语义。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{PopoverEvent, button, popover};
use nsh_core::prelude::*;

use crate::overlay;

/// 气泡说明文案锚点后缀（headless `id:` 选择器用；调用方给前缀）。
pub(crate) const MESSAGE_ID_SUFFIX: &str = "-message";
/// 取消键锚点后缀。
pub(crate) const CANCEL_ID_SUFFIX: &str = "-cancel";
/// 确认键锚点后缀。
pub(crate) const CONFIRM_ID_SUFFIX: &str = "-confirm";

/// 气泡的内容与文案（调用方持词表，本封装零业务语义）。
#[derive(Debug, Clone)]
pub struct PopconfirmSpec {
    /// 说明文案。
    pub message: String,
    /// 确认按钮文案。
    pub confirm_label: String,
    /// 取消按钮文案。
    pub cancel_label: String,
}

/// 轻量确认气泡：`open` 是受控开合（读闭包），开合回写（含 ESC / 点外的宿主
/// 关闭）经 `on_open_change` 回给调用方；确认键先回 `on_confirm` 再关。触发件
/// 外包一层按压视图（开气泡的按压源），锚定与命中区以它为准。
pub fn popconfirm(
    id_prefix: &'static str,
    open: impl Fn() -> bool + Clone + 'static,
    on_open_change: impl Fn(bool) + Clone + 'static,
    spec: PopconfirmSpec,
    trigger: impl IntoElement,
    on_confirm: impl Fn() + Clone + 'static,
) -> impl IntoElement {
    // 开/关上报（Overlay Suppression 注册表单一通道：追踪受控开合，全路径
    // 覆盖——确认 / 取消 / ESC / 点外四条关闭路径都走信号回写）；组件卸载时
    // Drop 自动撤销（卸载兜底，与期间弹层同形）。
    let token = overlay::OverlayToken::new();
    let open_for_effect = open.clone();
    effect(move || {
        let shown = open_for_effect();
        token.report(shown);
    });
    // 主题令牌（双态自适应；令牌缺失回退亮灰/中灰可读兜底，app.rs 先例同形）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let (message_text, confirm_text, cancel_text) =
        (spec.message, spec.confirm_label, spec.cancel_label);
    // 开合回写多站点复用（触发件 / 宿主回写 / 取消键 / 确认键），闭包捕获
    // 调用方状态非 Copy，各站点持克隆。
    let open_for_popover = open.clone();
    let on_open_for_trigger = on_open_change.clone();
    let on_open_for_popover = on_open_change.clone();
    let on_open_for_cancel = on_open_change.clone();
    let on_confirm_and_close = move || {
        on_confirm();
        on_open_change(false);
    };
    popover()
        .open(move || Some(open_for_popover()))
        .on_open_change(move |e: &PopoverEvent| {
            if let PopoverEvent::OpenChange(shown, _) = *e {
                on_open_for_popover(shown);
            }
        })
        .popup_class("bg-background border border-border rounded-md shadow-md p-2")
        .trigger(
            view()
                .on_press(move |_: &PressEvent| on_open_for_trigger(true))
                .child(trigger.into_element()),
        )
        .child(
            view()
                .flex_col()
                .gap_2()
                .child(
                    text(message_text)
                        .text_size(px(13.0))
                        .text_color(fg)
                        .test_id(format!("{}{}", id_prefix, MESSAGE_ID_SUFFIX).as_str()),
                )
                .child(
                    view()
                        .flex_row()
                        .justify_end()
                        .gap_2()
                        .child(
                            button()
                                .app_class(
                                    "px-2 py-1 rounded-md border border-border text-foreground hover:bg-accent",
                                )
                                .child(
                                    text(cancel_text)
                                        .text_size(px(12.0))
                                        .text_color(fg)
                                        .test_id(format!("{}{}", id_prefix, CANCEL_ID_SUFFIX).as_str()),
                                )
                                .on_press(move |_: &()| on_open_for_cancel(false)),
                        )
                        .child(
                            button()
                                .app_class("px-2 py-1 rounded-md bg-destructive hover:opacity-90")
                                .child(
                                    text(confirm_text)
                                        .text_size(px(12.0))
                                        .text_color(rgb(0xffffff))
                                        .test_id(format!("{}{}", id_prefix, CONFIRM_ID_SUFFIX).as_str()),
                                )
                                .on_press(move |_: &()| on_confirm_and_close()),
                        ),
                ),
        )
}
