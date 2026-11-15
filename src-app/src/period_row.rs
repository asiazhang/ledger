//! 时间维度行（交易列表与搜索视图的共用形态，#2040 起自 [`crate::app`]
//! 抽出单点）：期间步进器 `< 期间标签 >` + 期间选择弹层（预设闭集芯片 +
//! 月/季/年直达面板，#2128 补齐直达面板——旧壳 QuickTimeRange.periodPicker
//! 平价）。
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
//! 直达面板（#2128，旧壳 QuickTimeRange.periodPicker 同构）：面板单位随当
//! 前游标（月游标 → 月格、季 → 季格、年 → 年格），「全部」态默认月面板；
//! 点选写精确自然周期快照并关弹层（同芯片出口）；格子钳制与步进器同源——
//! 受数据期间边界约束（边界外不可选），年面板年份闭集按边界裁剪；面板格
//! 词表与 [`crate::period`] 期间文案同源（「5月」「三季度」「2025年」）。
//!
//! 弹层抑制上报自含（Overlay Suppression，[`crate::overlay`]）：token 实例
//! 作用域持有 + effect 追踪受控 open 信号（受控关闭不产生 Popover 事件，
//! 只听事件会漏报）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use chrono::Datelike;
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

/// 直达面板格子文案（zh-CN，与 [`crate::period`] 期间词表同源；旧壳
/// quickTimeRange.periodPicker 同构）。index 语义随单位（0 起月/季，年恒 0）。
fn panel_cell_label(p: &period::NaturalPeriod) -> String {
    match p.unit {
        period::PeriodUnit::Month => format!("{}月", p.index + 1),
        period::PeriodUnit::Quarter => {
            let name = ["一", "二", "三", "四"][p.index as usize % 4];
            format!("{name}季度")
        }
        period::PeriodUnit::Year => format!("{}年", p.year),
    }
}

/// 直达面板格子稳定锚点后缀（headless 选择器用 id: 前缀，同芯片）。
fn panel_cell_test_suffix(p: &period::NaturalPeriod) -> String {
    match p.unit {
        period::PeriodUnit::Month => format!("m{}", p.index),
        period::PeriodUnit::Quarter => format!("q{}", p.index),
        period::PeriodUnit::Year => format!("y{}", p.year),
    }
}

/// 时间维度行（步进器 + 期间选择弹层）。
///
/// - `prefix`：弹层与芯片 test_id 前缀（交易列表 `"period"`、搜索视图
///   `"search-period"`、报表视图 `"reports-period"`——headless 锚点互不串扰）。
/// - `key_hints`：步进按钮内嵌键位字符（交易列表 `","`/`"."`，ADR-0141；
///   搜索/报表视图无快捷键传 `false`）。
/// - `include_all`：预设闭集是否含「全部」（报表页日期闭集无全部——期间必有
///   界，ADR-0057；交易/搜索含全部 = 默认态哨兵）。
/// - `range`：期间快照信号（唯一事实源，调用方持有）。
/// - `boundary_dates`：数据期间边界原始日期对（None = 在途/失败 → 钳制退化
///   为不钳制，旧壳 silent 同口径）。
/// - `on_apply`：快照应用回调（`changed` = 快照是否真的变化；装载时机与
///   翻页归零归调用方）。
///
/// 返回 `(行视图, 步进动作)`：步进动作供外部快捷键与按钮共用同一决策半边
/// （无抑制闸门——闸门归键盘派发路径，按钮点击无闸门，先例分页条点击）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn period_row(
    prefix: &'static str,
    key_hints: bool,
    include_all: bool,
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

    // —— 直达面板派生（#2128，旧壳 periodPicker 同构）——面板单位随当前
    // 游标，「全部」态默认月面板（旧壳 periodPanelUnit 同式）；面板边界
    // 取面板单位下的数据期间边界（不随游标缺席而缺席——「全部」态同样钳
    // 制，旧壳 periodPanelBoundary 同式）；锚年随开弹层重置为游标年或今年。
    let panel_unit = computed(move || {
        cursor
            .get()
            .map(|p| p.unit)
            .unwrap_or(period::PeriodUnit::Month)
    });
    let panel_year = signal(today().year());
    let panel_boundary = computed(move || {
        let (min, max) = boundary_dates.get()?;
        Some(period::derive_boundary(
            panel_unit.get(),
            min.as_deref(),
            max.as_deref(),
            today(),
        ))
    });
    // 面板锚定年导航可达性：锚年不越出数据期间边界年份（年区间按边界裁剪）。
    // 边界未知退化为不钳制（同步进器）。
    let can_panel_prev = computed(move || match panel_boundary.get() {
        Some(b) => panel_year.get() > b.earliest.year,
        None => true,
    });
    let can_panel_next = computed(move || match panel_boundary.get() {
        Some(b) => panel_year.get() < b.latest.year,
        None => true,
    });
    // 面板格子闭集：月面板 = 锚年 12 个月、季面板 = 锚年 4 个季度、年面板 =
    // 数据期间边界年份全闭集（边界未知回退锚年 ±10 的有界窗，旧壳 ±100 同
    // 意图的收敛形态——格子闭集有界即全部可达）。
    let panel_cells = computed(move || match panel_unit.get() {
        period::PeriodUnit::Month => (0..12)
            .map(|m0| period::NaturalPeriod {
                unit: period::PeriodUnit::Month,
                year: panel_year.get(),
                index: m0,
            })
            .collect::<Vec<_>>(),
        period::PeriodUnit::Quarter => (0..4)
            .map(|q0| period::NaturalPeriod {
                unit: period::PeriodUnit::Quarter,
                year: panel_year.get(),
                index: q0,
            })
            .collect::<Vec<_>>(),
        period::PeriodUnit::Year => match panel_boundary.get() {
            Some(b) => (b.earliest.year..=b.latest.year)
                .map(|y| period::NaturalPeriod {
                    unit: period::PeriodUnit::Year,
                    year: y,
                    index: 0,
                })
                .collect::<Vec<_>>(),
            None => {
                let y = panel_year.get();
                (y - 10..=y + 9)
                    .map(|year| period::NaturalPeriod {
                        unit: period::PeriodUnit::Year,
                        year,
                        index: 0,
                    })
                    .collect::<Vec<_>>()
            }
        },
    });
    // 面板格子钳制（面板版步进器置灰）：数据期间边界外不可选；边界未知
    // 退化为不钳制（同同步进器，旧壳 isPeriodDateDisabled 同语义）。
    let panel_cell_enabled = move |p: &period::NaturalPeriod| -> bool {
        match panel_boundary.get() {
            Some(b) => period::period_within_boundary(p, &b),
            None => true,
        }
    };

    // 开弹层重置面板锚年：随当前游标年，「全部」态锚今年（旧壳面板值随
    // currentPeriod 同语义）。
    let on_open_change = move |e: &PopoverEvent| {
        if let PopoverEvent::OpenChange(open, _) = *e {
            period_open.set(open);
            if open {
                panel_year.set(
                    cursor
                        .get()
                        .map(|p| p.year)
                        .unwrap_or_else(|| today().year()),
                );
            }
        }
    };

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

    // 直达面板点选动作（#2128）：写精确自然周期快照并关弹层——与芯片同一
    // 出口（快照唯一事实源在调用方信号，装载与否归 on_apply(changed) 裁决；
    // 「全部」态仅在确认选定后才离开无过滤默认态，旧壳 onPeriodPanelSelect
    // 同语义）。
    let apply_panel = move |p: period::NaturalPeriod| {
        let Some(next) = period::period_range(&p) else {
            return;
        };
        let changed = range.get_untracked().as_ref() != Some(&next);
        range.set(Some(next));
        on_apply(changed);
        period_open.set(false);
    };

    // 键位提示（交易列表内嵌 `,`/`.`，ADR-0141；搜索视图无快捷键 → 无子元素）。
    let key_hint = |hint: &'static str| -> Option<View> {
        key_hints.then(|| view().child(text(hint).text_xs().text_color(muted)))
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
                .app_class("px-2 py-1 rounded-(--radius) text-foreground hover:bg-accent disabled:opacity-40")
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
                .on_open_change(on_open_change)
                .popup_class("bg-background border border-border rounded-(--radius) shadow-md p-1")
                .trigger(
                    // test_id 载体视图（组件元素无 test_id 槽，先例 tx-pagination 的
                    // 载体形态）；headless 经 id: 锚点命中载体、物理命中到内层按钮。
                    view().test_id(format!("{prefix}-label").as_str()).child(
                        button()
                            .app_class("px-2 py-1 rounded-(--radius) text-foreground hover:bg-accent")
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(text(move || period_label_text(cursor.get())).text_color(fg))
                            .child(text("▾").text_xs().text_color(muted)),
                    ),
                )
                .child(
                    // 弹层主体：预设芯片列（既有形态保留）+ 直达面板（#2128）。
                    view().flex_col().gap_1().child(
                        // 芯片 test_id 落在按钮内文本上（headless press 只认可按元素
                        // 本身或其内部文本，包裹视图不命中，nsh-testing Screen::pressable）。
                        view()
                            .flex_col()
                            .gap_1()
                            .children(period::PRESETS.iter().copied().filter(|p| include_all || *p != period::Preset::All).map(|preset| {
                                button()
                                    .selected(move || match_preset_now(range.get(), preset))
                                    .app_class(
                                        "px-2 py-1 rounded-(--radius) text-foreground hover:bg-accent selected:bg-accent",
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
                    )
                    .child(
                        // 直达面板（#2128）：锚定年导航行（‹ 年 ›，越界年禁用）
                        // + 格子闭集（单位随游标；锚年变化整组重建，`each` 键 =
                        // 单位-年-序）。格子锚点落在按钮内文本上（同芯片形态）。
                        view()
                            .flex_col()
                            .gap_1()
                            .app_class("border-t border-border pt-1")
                            .test_id(format!("{prefix}-panel").as_str())
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        button()
                                            .disabled(move || !can_panel_prev.get())
                                            .accessibility_label("面板上一年")
                                            .app_class(
                                                "px-2 py-1 rounded-(--radius) text-foreground hover:bg-accent disabled:opacity-40",
                                            )
                                            .on_press({
                                                let year = panel_year;
                                                move || year.set(year.get() - 1)
                                            })
                                            .child(text("‹").test_id(format!("{prefix}-panel-prev").as_str()).text_color(fg)),
                                    )
                                    .child(
                                        text(move || format!("{}年", panel_year.get()))
                                            .test_id(format!("{prefix}-panel-year").as_str())
                                            .text_xs()
                                            .text_color(muted),
                                    )
                                    .child(
                                        button()
                                            .disabled(move || !can_panel_next.get())
                                            .accessibility_label("面板下一年")
                                            .app_class(
                                                "px-2 py-1 rounded-(--radius) text-foreground hover:bg-accent disabled:opacity-40",
                                            )
                                            .on_press({
                                                let year = panel_year;
                                                move || year.set(year.get() + 1)
                                            })
                                            .child(text("›").test_id(format!("{prefix}-panel-next").as_str()).text_color(fg)),
                                    ),
                            )
                            .child(
                                view()
                                    .flex_row()
                                    .flex_wrap()
                                    .gap_1()
                                    .child(each(
                                        move || panel_cells.get(),
                                        |p: &period::NaturalPeriod| {
                                            format!("{:?}-{}-{}", p.unit, p.year, p.index)
                                        },
                                        move |p: ReadSignal<period::NaturalPeriod>, _index: ReadSignal<usize>| {
                                            let current = p.get();
                                            button()
                                                .selected(move || cursor.get() == Some(p.get()))
                                                .disabled(move || !panel_cell_enabled(&p.get()))
                                                .app_class(
                                                    "w-16 py-1 rounded-(--radius) text-foreground hover:bg-accent selected:bg-accent disabled:opacity-40",
                                                )
                                                .flex_row()
                                                .items_center()
                                                .justify_center()
                                                .child(
                                                    text(panel_cell_label(&current))
                                                        .text_xs()
                                                        .test_id(
                                                            format!("{prefix}-panel-{}", panel_cell_test_suffix(&current))
                                                                .as_str(),
                                                        ),
                                                )
                                                .on_press(move || apply_panel(p.get()))
                                        },
                                    )),
                            ),
                    ),
                ),
        )
        .child(
            button()
                .disabled(move || !can_step_next.get())
                .accessibility_label("下一个周期")
                .app_class("px-2 py-1 rounded-(--radius) text-foreground hover:bg-accent disabled:opacity-40")
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
