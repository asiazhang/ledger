//! 自由文本可搜输入封装（issue #2034；`App*` 封装族，ADR-0118 纪律的 NSH 延续
//! ——旧壳 `PinyinSelect` 的 tag 形态对应件，视图不直连框架基元）。
//!
//! 形态：文本输入框（原样承载键入）+ 键入期间锚定的建议弹层（统一模糊搜索
//! 语义过滤，消费 [`crate::pinyin_select::filter_options`] 单点）。双态值语义
//! （旧壳 tag 下拉的承继）：选定建议项 → `selected` 落 id；键入改动 → 选定
//! 失效、值退回自由文本（`typed`）。空文本 = 未挂。商户「选择在用商户或直接
//! 输入新名称」是首个消费面。
//!
//! 弹层纪律：开/关上报 Overlay Suppression 注册表（[`crate::overlay`]，每实例
//! 一个 token；effect 追踪受控 open 信号，全关闭路径覆盖）；ESC / 点外关闭由
//! 弹层宿主承担。本封装零业务语义：id 与文本归调用方信号持有。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{PopoverEvent, input, popover};
use nsh_core::prelude::*;

use crate::{assembly::CategoryOption, overlay, pinyin_select};

/// 输入框锚点（headless `id:` 选择器用）。
pub(crate) const INPUT_ID_SUFFIX: &str = "-input";
/// 建议行的稳定锚点后缀（headless `id:` 选择器用；调用方给前缀）。
pub(crate) const OPTION_ID_SUFFIX: &str = "-option";

/// 自由文本可搜输入：`id_prefix` 是 headless 锚点前缀（输入框
/// `{prefix}-input`、建议行 `{prefix}-option`）；`options` 是建议数据源闭包；
/// `typed` 是输入框原样文本信号；`selected` 是选定建议项 id 信号（键入即失
/// 效）；`placeholder` 是空态占位文案。
pub fn combo_select(
    id_prefix: &'static str,
    options: impl Fn() -> Vec<CategoryOption> + Copy + 'static,
    typed: Signal<String>,
    selected: Signal<Option<String>>,
    placeholder: &'static str,
) -> impl IntoElement {
    let open = signal(false);
    // 主题令牌（双态自适应；令牌缺失回退亮灰/中灰可读兜底，先例同形）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());

    // 开/关上报（Overlay Suppression 注册表单一通道：追踪受控 open 信号，全
    // 关闭路径覆盖）；组件卸载时 Drop 自动撤销（卸载兜底）。
    let token = overlay::OverlayToken::new();
    effect(move || {
        let shown = open.get();
        token.report(shown);
    });

    // 过滤后的建议行集（过滤语义单点在 pinyin_select::filter_options——统一
    // 模糊搜索语义 ADR-0027 的直接消费，不在本封装另立第二份匹配）。
    let shown = computed(move || pinyin_select::filter_options(&typed.get(), &options()));

    let choose = move |option: CategoryOption| {
        selected.set(Some(option.id));
        typed.set(option.label);
        open.set(false);
    };

    popover()
        .open(move || Some(open.get()))
        .on_open_change(move |e: &PopoverEvent| {
            if let PopoverEvent::OpenChange(shown, _) = *e {
                open.set(shown);
            }
        })
        .popup_class("bg-background border border-border rounded-md shadow-md p-1")
        .trigger(
            view()
                // 聚焦/按压即开建议（旧壳 PinyinSelect focus 开面板同语义；
                // 键入经 on_change 同样开合）。
                .on_press(move |_: &PressEvent| open.set(true))
                .child(
                    input()
                        .test_id(format!("{}{}", id_prefix, INPUT_ID_SUFFIX).as_str())
                        .value(move || typed.get())
                        .placeholder(placeholder)
                        .text_color(fg)
                        .min_w(px(200.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |value: &str| {
                            // 键入即失效选定（值退回自由文本语义）并随键入开/关
                            // 建议弹层（空文本 = 无可建议，关）。
                            typed.set(value.to_owned());
                            selected.set(None);
                            open.set(!value.trim().is_empty());
                        }),
                ),
        )
        .child(each(
            move || shown.get(),
            |o: &CategoryOption| o.id.clone(),
            move |item: ReadSignal<CategoryOption>, _index: ReadSignal<usize>| {
                let option = item.get();
                view()
                    .test_id(format!("{}{}", id_prefix, OPTION_ID_SUFFIX).as_str())
                    .child(
                        nsh_base::button()
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
        ))
}
