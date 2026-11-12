//! 多选下拉选择封装（issue #2037；`App*` 封装族，ADR-0118 纪律的 NSH 延续——
//! 旧壳 `NSelect multiple`（交易列表类型筛选）的 NSH 对应件，视图不直连框架
//! 基元）。
//!
//! 形态：触发件（已选项 label 按选项闭集序拼接 / 占位文案）+ 锚定选项列表弹层
//! （行首勾选标记；点选**切换勾选、弹层不关**——旧壳 multiple 语义）。可清空：
//! 有选中时触发件右侧渲染 ✕（独立按压件，清除不开弹层——select.rs 同位）。
//!
//! 选择态归调用方（`is_selected` 读、`on_toggle` / `on_clear` 写的闭包受控
//! 形态——选择值不是本封装的状态，归一规则等业务语义归调用方的筛选层；
//! [`crate::select`] 的信号受控形态不适配「写入前需归一」的多选值）。
//!
//! 弹层纪律：开/关上报 Overlay Suppression 注册表（[`crate::overlay`]，每实例
//! 一个 token；effect 追踪受控 open 信号，全关闭路径覆盖）；ESC / 点外关闭由
//! 弹层宿主承担。数据源归调用方，本封装零业务语义。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{PopoverEvent, button, popover};
use nsh_core::prelude::*;

use crate::{assembly::CategoryOption, design::font_size, overlay};

/// 触发件锚点后缀（headless `id:` 选择器用；调用方给前缀）。
pub(crate) const TRIGGER_ID_SUFFIX: &str = "-trigger";
/// 选中值文本锚点后缀。
const VALUE_ID_SUFFIX: &str = "-value";
/// 选项行锚点后缀。
pub(crate) const OPTION_ID_SUFFIX: &str = "-option";
/// 清除按钮锚点后缀。
pub(crate) const CLEAR_ID_SUFFIX: &str = "-clear";

/// 多选下拉选择：`id_prefix` 是 headless 锚点前缀；`options` 是数据源闭包；
/// `is_selected` / `on_toggle` / `on_clear` 是选择态的读写面（归调用方）；
/// `placeholder` 是无选中时的触发件文案。
pub fn multi_select(
    id_prefix: impl Into<std::borrow::Cow<'static, str>>,
    options: impl Fn() -> Vec<CategoryOption> + Copy + 'static,
    is_selected: impl Fn(&str) -> bool + Copy + 'static,
    on_toggle: impl Fn(&str) + Copy + 'static,
    on_clear: impl Fn() + Copy + 'static,
    placeholder: &'static str,
) -> impl IntoElement {
    let id_prefix = id_prefix.into();
    let prefix_trigger = format!("{}{}", id_prefix, TRIGGER_ID_SUFFIX);
    let prefix_value = format!("{}{}", id_prefix, VALUE_ID_SUFFIX);
    let prefix_option = format!("{}{}", id_prefix, OPTION_ID_SUFFIX);
    let prefix_clear = format!("{}{}", id_prefix, CLEAR_ID_SUFFIX);
    let prefix_empty = format!("{}-empty", id_prefix);
    let open = signal(false);
    // 主题令牌（双态自适应）：色值住 theme.json 令牌表，读取经
    // [`crate::theme::color`] 单点，缺失落可读兜底——本模块零本地色常量
    // （issue #2107 视觉收口）。
    let fg = crate::theme::color("foreground");
    let muted = crate::theme::color("muted-foreground");

    // 开/关上报（Overlay Suppression 注册表单一通道：追踪受控 open 信号，全
    // 关闭路径覆盖）；组件卸载时 Drop 自动撤销（卸载兜底）。
    let token = overlay::OverlayToken::new();
    effect(move || {
        let shown = open.get();
        token.report(shown);
    });

    // 触发件文案：已选项 label 按选项闭集序拼接（旧壳 kindValue 按闭集序渲染
    // 同语义）；无选中回占位。
    let trigger_label = move || {
        let opts = options();
        let labels: Vec<&str> = opts
            .iter()
            .filter(|o| is_selected(&o.id))
            .map(|o| o.label.as_str())
            .collect();
        if labels.is_empty() {
            placeholder.to_owned()
        } else {
            labels.join("、")
        }
    };

    let toggle = move |option: CategoryOption| on_toggle(&option.id);

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
                .popup_class("bg-background border border-border rounded-(--radius) shadow-md p-1")
                .trigger(
                    view().test_id(prefix_trigger.as_str()).child(
                        button()
                            .app_class(
                                "px-2 py-1 rounded-(--radius) border border-border hover:bg-accent",
                            )
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .min_w(px(120.0))
                            .on_press(move |_: &()| open.set(true))
                            .child(
                                // 触发件文案（字阶 SECONDARY 档，design.rs font_size
                                // 同源——DESIGN.md §3 「菜单/控件标签」档）。
                                text(trigger_label)
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(move || {
                                        // 有选中呈前景、无选中呈占位弱化色。
                                        if options().iter().any(|o| is_selected(&o.id)) {
                                            fg
                                        } else {
                                            muted
                                        }
                                    })
                                    .number_of_lines(1)
                                    .test_id(prefix_value.as_str()),
                            )
                            .child(
                                text("▾")
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(muted),
                            ),
                    ),
                )
                .child(view().flex_col().gap_1().child(each(
                    options,
                    |o: &CategoryOption| o.id.clone(),
                    move |item: ReadSignal<CategoryOption>, _index: ReadSignal<usize>| {
                        let option = item.get();
                        let checked = is_selected(&option.id);
                        view().test_id(prefix_option.as_str()).child(
                            button()
                                .app_class(
                                    "w-full px-2 py-1 rounded-(--radius) text-left text-foreground \
                                         hover:bg-accent selected:bg-accent",
                                )
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .child(
                                    // 勾选标记（旧壳 NSelect multiple 选中行 ✓ 同位；
                                    // 字形 = 字阶 CAPTION 档）。
                                    text(if checked { "✓" } else { "" })
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(fg)
                                        .test_id(format!("{}-check", option.id).as_str()),
                                )
                                .child(
                                    text(option.label.clone())
                                        .text_size(px(font_size::SECONDARY))
                                        .text_color(fg),
                                )
                                .on_press(move |_: &()| toggle(option.clone())),
                        )
                    },
                )))
                .child(
                    // 全空占位（选项闭集为空的防御形态；正常闭集四项不触达）。
                    when(
                        move || options().is_empty(),
                        move || {
                            text("无数据")
                                .text_size(px(font_size::CAPTION))
                                .text_color(muted)
                                .test_id(prefix_empty.as_str())
                                .app_class("px-2 py-1")
                        },
                    )
                    .otherwise(view),
                ),
        )
        .child(
            // 清除钮（有选中才呈现；独立按压件，清除不开弹层——select.rs 同位）。
            when(
                move || options().iter().any(|o| is_selected(&o.id)),
                move || {
                    button()
                        .accessibility_label("清除")
                        .app_class("px-1 py-0.5 rounded-sm text-muted-foreground hover:bg-accent")
                        .child(
                            text("✕")
                                .text_size(px(font_size::CAPTION))
                                .text_color(muted)
                                .test_id(prefix_clear.as_str()),
                        )
                        .on_press(move |_: &()| on_clear())
                },
            )
            .otherwise(view),
        )
}
