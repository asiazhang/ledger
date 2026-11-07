//! 普通下拉选择封装（issue #2034；`App*` 封装族，ADR-0118 纪律的 NSH 延续——
//! 旧壳 `AppSelect` 的 NSH 对应件，视图不直连框架基元）。
//!
//! 形态：触发件（显示选中项 label / 占位文案）+ 锚定选项列表弹层（无搜索——
//! 小闭集列表的直选形态；可搜形态归 [`crate::pinyin_select`]）。可清空：选中
//! 时触发件右侧渲染 ✕（独立按压件，清除不开弹层——冒泡语义与旧壳 NSelect
//! clearable 同位：清除只落 None）。
//!
//! 弹层纪律：开/关上报 Overlay Suppression 注册表（[`crate::overlay`]，每实例
//! 一个 token；effect 追踪受控 open 信号，全关闭路径覆盖）；ESC / 点外关闭由
//! 弹层宿主承担；选项选中即关。数据源归调用方（调用方持词表与选项闭集），本
//! 封装零业务语义。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{PopoverEvent, button, popover};
use nsh_core::prelude::*;

use crate::{assembly::CategoryOption, overlay};

/// 触发件锚点（headless `id:` 选择器用）。
pub(crate) const TRIGGER_ID_SUFFIX: &str = "-trigger";
/// 选项行的稳定锚点后缀（headless `id:` 选择器用；调用方给前缀）。
pub(crate) const OPTION_ID_SUFFIX: &str = "-option";
/// 清除按钮锚点后缀。
pub(crate) const CLEAR_ID_SUFFIX: &str = "-clear";

/// 普通下拉选择：`id_prefix` 是 headless 锚点前缀（触发件 `{prefix}-trigger`、
/// 选项行 `{prefix}-option`、清除钮 `{prefix}-clear`；动态前缀以 `Cow` 承载，
/// 静态常量调用点零改动）；`options` 是数据源闭包；`selected` 是选中值信号
/// （调用方持有、读写皆可；None = 未选，等价旧壳 clearable 后的空值）；
/// `placeholder` 是未选中时的触发件文案。
pub fn app_select(
    id_prefix: impl Into<std::borrow::Cow<'static, str>>,
    options: impl Fn() -> Vec<CategoryOption> + Copy + 'static,
    selected: Signal<Option<String>>,
    placeholder: &'static str,
) -> impl IntoElement {
    let id_prefix = id_prefix.into();
    // 后缀拼接预派生（子行动态前缀：各 move 闭包捕获自己的 String）。
    let prefix_trigger = format!("{}{}", id_prefix, TRIGGER_ID_SUFFIX);
    let prefix_value = format!("{}-value", id_prefix);
    let prefix_option = format!("{}{}", id_prefix, OPTION_ID_SUFFIX);
    let prefix_clear = format!("{}{}", id_prefix, CLEAR_ID_SUFFIX);
    let open = signal(false);
    // 主题令牌（双态自适应；令牌缺失回退亮灰/中灰可读兜底，先例同形）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());

    // 开/关上报（Overlay Suppression 注册表单一通道：追踪受控 open 信号，全
    // 关闭路径覆盖）；组件卸载时 Drop 自动撤销（卸载兜底）。
    let token = overlay::OverlayToken::new();
    effect(move || {
        let shown = open.get();
        token.report(shown);
    });

    // 选中项 label（触发件呈现；选项被外部换源时按 value 回查）。
    let selected_label = move || {
        let id = selected.get()?;
        options()
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.label.clone())
    };
    let choose = move |option: CategoryOption| {
        selected.set(Some(option.id));
        open.set(false);
    };

    view()
        .flex_row()
        .items_center()
        .gap_1()
        .child(
            popover()
                .open(move || Some(open.get()))
                .on_open_change(move |e: &PopoverEvent| {
                    if let PopoverEvent::OpenChange(shown, _) = *e {
                        open.set(shown);
                    }
                })
                .popup_class("bg-background border border-border rounded-md shadow-md p-1")
                .trigger(
                    view().test_id(prefix_trigger.as_str()).child(
                        button()
                            .app_class("px-2 py-1 rounded-md border border-border hover:bg-accent")
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .min_w(px(160.0))
                            .on_press(move |_: &()| open.set(true))
                            .child(
                                text(move || {
                                    selected_label().unwrap_or_else(|| placeholder.to_owned())
                                })
                                .text_size(px(13.0))
                                .text_color(move || {
                                    if selected_label().is_some() {
                                        fg
                                    } else {
                                        muted
                                    }
                                })
                                .number_of_lines(1)
                                .test_id(prefix_value.as_str()),
                            )
                            .child(text("▾").text_size(px(11.0)).text_color(muted)),
                    ),
                )
                .child(each(
                    options,
                    |o: &CategoryOption| o.id.clone(),
                    move |item: ReadSignal<CategoryOption>, _index: ReadSignal<usize>| {
                        let option = item.get();
                        view().test_id(prefix_option.as_str()).child(
                            button()
                                .app_class(
                                    "w-full px-2 py-1 rounded-md text-left text-foreground \
                                         hover:bg-accent selected:bg-accent",
                                )
                                .child(
                                    text(option.label.clone())
                                        .text_size(px(13.0))
                                        .text_color(fg),
                                )
                                .on_press(move |_: &()| choose(option.clone())),
                        )
                    },
                )),
        )
        .child(
            // 清除钮（可清空语义：选中时呈现；独立按压件，清除不开弹层）。
            when(
                move || selected.get().is_some(),
                move || {
                    button()
                        .accessibility_label("清除")
                        .app_class("px-1 py-0.5 rounded-md text-muted-foreground hover:bg-accent")
                        .child(
                            text("✕")
                                .text_size(px(11.0))
                                .text_color(muted)
                                .test_id(prefix_clear.as_str()),
                        )
                        .on_press(move |_: &()| selected.set(None))
                },
            )
            .otherwise(view),
        )
}
