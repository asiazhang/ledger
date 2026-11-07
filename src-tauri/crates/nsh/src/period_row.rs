//! 时间维度行（交易列表与搜索视图的共用形态，#2040 起自 [`crate::app`]
//! 抽出单点）：期间步进器 `< 期间标签 >` + 期间选择弹层（预设闭集芯片）。
//!
//! 纯决策逻辑（游标反推 / 步进换算 / 边界钳制 / 预设换算）单点在
//! [`crate::period`]，本模块只做呈现与交互桥：期间快照信号与「应用回调」
//! 归调用方（快照住交易筛选会话态还是搜索视图本地、装载时机与翻页归零都
//! 是各视图自己的语义，见调用方）；本模块写快照、调回调、关弹层。
//!
//! 交互语义与旧壳 QuickTimeRange 同构（ADR-0070 交易页全闭集形态）：
//! 「全部」＝无区间＝默认态；步进越出数据期间边界置灰（钳制归
//! [`crate::period`]）；「全部」无游标可反推 → 步进器置灰；同芯片重复点击
//! 不重读（同值守卫，应用回调收 `changed` 由调用方裁决是否装载）。
//! 键位提示（`,`/`.` 内嵌步进按钮）只属交易列表（ADR-0141 快捷键闭集），
//! 搜索视图无快捷键不内嵌——由 `key_hints` 入参分流。
//!
//! 弹层抑制上报自含（Overlay Suppression，[`crate::overlay`]）：token 实例
//! 作用域持有 + effect 追踪受控 open 信号（受控关闭不产生 Popover 事件，
//! 只听事件会漏报）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{PopoverEvent, button, popover};
use nsh_core::prelude::*;

use crate::overlay;
use crate::period;
use crate::views::view_palette;

/// 「今天」（本地时区日历日，期间预设与边界抬升的相对点，与旧壳本地
/// 日历同口径）。非响应式：跨期翻转随下次重算呈现（分钟级时钟不立桩）。
fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// 期间标签文案（旧壳 quickTimeRange.periodLabel 同词表）：有游标呈期间，
/// 无游标（「全部」）呈「选择期间」（弹层入口的召唤文案）。
fn period_label_text(cursor: Option<period::NaturalPeriod>) -> String {
    cursor
        .as_ref()
        .map(period::format_period_label)
        .unwrap_or_else(|| "选择期间".to_owned())
}

/// 芯片点亮判定（UI 侧适配：读快照区间 → 高亮派生）。
fn match_preset_now(range: Option<period::PeriodRange>, preset: period::Preset) -> bool {
    let (from, to) = match range {
        Some(r) => (Some(r.from), Some(r.to)),
        None => (None, None),
    };
    period::match_preset(from.as_deref(), to.as_deref(), today()) == Some(preset)
}

/// 芯片稳定锚点后缀（headless 选择器用 id: 前缀，research doc §3.5）。
fn preset_test_suffix(preset: period::Preset) -> &'static str {
    match preset {
        period::Preset::All => "all",
        period::Preset::Month => "month",
        period::Preset::Quarter => "quarter",
        period::Preset::Year => "year",
        period::Preset::LastYear => "last-year",
    }
}

/// 时间维度行（步进器 + 期间选择弹层）。
///
/// - `prefix`：弹层与芯片 test_id 前缀（交易列表 `"period"`、搜索视图
///   `"search-period"`——headless 锚点互不串扰）。
/// - `key_hints`：步进按钮内嵌键位字符（交易列表 `","`/`"."`，ADR-0141；
///   搜索视图无快捷键传 `false`）。
/// - `range`：期间快照信号（唯一事实源，调用方持有）。
/// - `boundary_dates`：数据期间边界原始日期对（None = 在途/失败 → 钳制退化
///   为不钳制，旧壳 silent 同口径）。
/// - `on_apply`：快照应用回调（`changed` = 快照是否真的变化；装载时机与
///   翻页归零归调用方）。
///
/// 返回 `(行视图, 步进动作)`：步进动作供外部快捷键与按钮共用同一决策半边
/// （无抑制闸门——闸门归键盘派发路径，按钮点击无闸门，先例分页条点击）。
pub(crate) fn period_row(
    prefix: &'static str,
    key_hints: bool,
    range: Signal<Option<period::PeriodRange>>,
    boundary_dates: Signal<Option<(Option<String>, Option<String>)>>,
    on_apply: impl Fn(bool) + Copy + 'static,
) -> (View, impl Fn(i32) + Copy + 'static) {
    let palette = view_palette();
    let fg = palette.fg;
    let muted = palette.muted;

    // 弹层开合（实例本地）与抑制上报（token 实例作用域持有，effect 追踪
    // 受控 open 信号——全路径覆盖，组件卸载时 Drop 自动撤销）。
    let period_open = signal(false);
    let overlay_token = overlay::OverlayToken::new();
    effect(move || {
        let open = period_open.get();
        overlay_token.report(open);
    });

    // 期间派生（纯函数单点，UI 只消费）：游标、当前游标单位下的边界、步进
    // 可达性。
    let cursor = computed(move || {
        let r = range.get();
        period::range_to_period(
            r.as_ref().map(|p| p.from.as_str()),
            r.as_ref().map(|p| p.to.as_str()),
        )
    });
    let boundary = computed(move || {
        let unit = cursor.get()?.unit;
        let (min, max) = boundary_dates.get()?;
        Some(period::derive_boundary(
            unit,
            min.as_deref(),
            max.as_deref(),
            today(),
        ))
    });
    let can_step_prev = computed(move || {
        let r = range.get();
        period::can_step_current(
            r.as_ref().map(|p| p.from.as_str()),
            r.as_ref().map(|p| p.to.as_str()),
            -1,
            boundary.get().as_ref(),
        )
    });
    let can_step_next = computed(move || {
        let r = range.get();
        period::can_step_current(
            r.as_ref().map(|p| p.from.as_str()),
            r.as_ref().map(|p| p.to.as_str()),
            1,
            boundary.get().as_ref(),
        )
    });

    // 周期步进动作（ADR-0141 决策 2：游标反推、边界钳制、越界幂等全在
    // [`period::step_current`] 单点；步进必然产生新快照，按已变化回调）。
    // 按钮与外部快捷键共用（快捷键路径的抑制闸门归调用方）。
    let apply_step = move |delta: i32| {
        let r = range.get();
        let Some(next) = period::step_current(
            r.as_ref().map(|p| p.from.as_str()),
            r.as_ref().map(|p| p.to.as_str()),
            delta,
            boundary.get().as_ref(),
        ) else {
            return;
        };
        range.set(Some(next));
        on_apply(true);
    };

    // 预设选择动作（弹层芯片点击）：写精确自然周期快照并关弹层；同值守卫
    // ——同芯片重复点击不重读（快照等值时 `changed` 为 false，装载与否归
    // 调用方）。「全部」= 无过滤区间（默认态，旧壳同语义）。
    let apply_preset = move |preset: period::Preset| {
        let next = period::preset_range(preset, today());
        let changed = range.get_untracked() != next;
        range.set(next);
        on_apply(changed);
        period_open.set(false);
    };

    // 键位提示（交易列表内嵌 `,`/`.`，ADR-0141；搜索视图无快捷键 → 无子元素）。
    let key_hint = |hint: &'static str| -> Option<View> {
        key_hints.then(|| view().child(text(hint).text_size(px(11.0)).text_color(muted)))
    };

    let row = view()
        .flex_row()
        .items_center()
        .gap_2()
        .px_4()
        .py_2()
        .app_class("border-b border-border")
        .child(
            button()
                .disabled(move || !can_step_prev.get())
                .accessibility_label("上一个周期")
                .app_class("px-2 py-1 rounded-md text-foreground hover:bg-accent disabled:opacity-40")
                .flex_row()
                .items_center()
                .gap_1()
                .on_press({
                    let apply = apply_step;
                    move || apply(-1)
                })
                .child(text("‹").text_color(fg))
                .children(key_hint(",")),
        )
        .child(
            // 期间标签 → 期间选择弹层（on_open_change 只回写受控 open 信号，
            // 开合上报住上方 effect 单通道；token 随组件卸载撤销）。
            popover()
                .open(move || Some(period_open.get()))
                .on_open_change(move |e: &PopoverEvent| {
                    if let PopoverEvent::OpenChange(open, _) = *e {
                        period_open.set(open);
                    }
                })
                .popup_class("bg-background border border-border rounded-md shadow-md p-1")
                .trigger(
                    // test_id 载体视图（组件元素无 test_id 槽，先例 tx-pagination 的
                    // 载体形态）；headless 经 id: 锚点命中载体、物理命中到内层按钮。
                    view().test_id(format!("{prefix}-label").as_str()).child(
                        button()
                            .app_class("px-2 py-1 rounded-md text-foreground hover:bg-accent")
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(text(move || period_label_text(cursor.get())).text_color(fg))
                            .child(text("▾").text_size(px(11.0)).text_color(muted)),
                    ),
                )
                .child(
                    // 芯片 test_id 落在按钮内文本上（headless press 只认可按元素
                    // 本身或其内部文本，包裹视图不命中，nsh-testing Screen::pressable）。
                    view()
                        .flex_col()
                        .gap_1()
                        .children(period::PRESETS.iter().copied().map(|preset| {
                            button()
                                .selected(move || match_preset_now(range.get(), preset))
                                .app_class(
                                    "px-2 py-1 rounded-md text-foreground hover:bg-accent selected:bg-accent",
                                )
                                .child(
                                    text(period::preset_label(preset)).test_id(
                                        format!("{prefix}-chip-{}", preset_test_suffix(preset))
                                            .as_str(),
                                    ),
                                )
                                .on_press({
                                    let apply = apply_preset;
                                    move || apply(preset)
                                })
                        })),
                ),
        )
        .child(
            button()
                .disabled(move || !can_step_next.get())
                .accessibility_label("下一个周期")
                .app_class("px-2 py-1 rounded-md text-foreground hover:bg-accent disabled:opacity-40")
                .flex_row()
                .items_center()
                .gap_1()
                .on_press({
                    let apply = apply_step;
                    move || apply(1)
                })
                .child(text("›").text_color(fg))
                .children(key_hint(".")),
        );

    (row, apply_step)
}
