//! bar 图自绘周长件（issue #2048 报表视图；ADR-0145 决策 5 的封装层落点——
//! NSH 内建 plot 只承载 line，柱状图组形在本件自绘首建，视图不直连绘制基元。
//! 旧壳 vue-chartjs Bar 的呈现语义忠实转译：柱体/柱尾标注/说明气泡；坐标轴
//! 刻度与图例按旧壳盘点后裁剪——数值口径收进说明气泡、三柱语义色表即图例）。
//!
//! 形态（按本票首建消费面建形，消费面外扩再行开放——just-in-time 纪律）：
//! - **纵向分组柱**（月度收支）：每个月一组（组 = 三根语义色并列柱 + x 轴
//!   月份标签）；柱高按全图最大正值等比例缩放，0 值不呈柱体（旧壳零高柱
//!   不可见同果）；
//! - **横向单柱行**（支出分类构成等）：名称列（按压 = 下钻/跳转，回调归
//!   调用方）+ 等比条 + 柱尾金额标注；条长比例与金额口径由调用方装配
//!   （统计口径单源在装配层），本件零统计语义。
//!
//! 说明气泡（旧壳 chart.js tooltip 的本壳对应形态）：桌面引擎无 hover
//! （nsh-testing 能力盘点 §1.4），交互语义 = 点按弹出（旧壳触控轴点按气泡
//! 同构，ADR-0088 决策 6 两轴分面）；每根柱独立受控气泡，开/关自含
//! Overlay Suppression 上报（[`crate::popover`] 同款纪律——追踪受控 open
//! 信号全路径覆盖），宿主关闭路径（ESC / 点外）经 on_open_change 回写信号。
//!
//! 输入纪律：规格面（[`VerticalGroupFace`] / [`HorizontalRowFace`]）由调用
//! 方落信号（reports.rs 装载产物），本件只消费——行/柱经 `each` 键控，
//! 行内内容随行信号响应式（键不变数据变、键变即重建两通道全覆盖）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{PopoverEvent, popover};
use nsh_core::prelude::*;

use crate::assembly::{REPORT_PALETTE_LEN, category_palette_index};
use crate::overlay;

/// 分类柱色板（旧壳 `category-chart.ts` PALETTE 的逐值镜像，hex 实色不分
/// 双态——canvas 图色随主题换色在旧壳即无此机制）：命中顺序不参与取色，
/// 分类按 id 稳定取色，商户排行按名次序复用同一色板。槽位深度一致性由
/// [`REPORT_PALETTE_LEN`] + 本模块单测钉住。
const PALETTE: [u32; REPORT_PALETTE_LEN] = [
    0x5470c6, 0x91cc75, 0xfac858, 0xee6666, 0x73c0de, 0x3ba272, 0xfc8452, 0x9a60b4, 0xea7ccc,
    0x18a058, 0xd03050, 0x2080f0,
];

/// 未分类柱固定灰（与真实分类一眼区分，旧壳 UNCATEGORIZED_COLOR 同值）。
pub(crate) fn uncategorized_color() -> AdaptiveColor {
    AdaptiveColor::rgb(0x909399, 0x909399)
}

/// 色板槽位 → 色值（越界回绕防崩：装配层散列已入界，越界只可能是色板长度
/// 漂移——回绕不断渲染，而非 panic）。
pub(crate) fn palette_color(index: usize) -> AdaptiveColor {
    let hex = PALETTE[index % REPORT_PALETTE_LEN];
    AdaptiveColor::rgb(hex, hex)
}

/// 商户排行名次色（旧壳 paletteColor 同口径：第 1 名 = 色板首位）。
pub(crate) fn ranking_color(index: usize) -> AdaptiveColor {
    palette_color(index)
}

/// 分类 id → 稳定色（同 id 恒同色；槽位口径归装配层 [`category_palette_index`]）。
pub(crate) fn category_color(id: &str) -> AdaptiveColor {
    palette_color(category_palette_index(id))
}

// ---------------------------------------------------------------------------
// 共享呈现小件
// ---------------------------------------------------------------------------

/// 柱条厚（pt）（纵向柱宽 = 横向条高；旧壳 barThickness 20 的量级，图观感
/// 归人工对表子集）。
const BAR_THICKNESS: f32 = 14.0;

fn foreground() -> AdaptiveColor {
    nsh_core::element::tokens()
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into())
}

fn muted_foreground() -> AdaptiveColor {
    nsh_core::element::tokens()
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into())
}

/// 受控说明气泡 + 柱体载体（纵向与横向共用）：按压柱体开气泡、宿主关闭路径
/// 回写、开/关抑制上报自含；内容与柱体几何随入参闭包响应式（键控行重建除外）。
fn bar_popover(
    anchor: String,
    lines: impl Fn() -> Vec<String> + Copy + 'static,
    body: impl Fn() -> (f32, f32, AdaptiveColor) + Copy + 'static,
) -> impl IntoElement {
    let open = signal(false);
    let token = overlay::OverlayToken::new();
    let open_watch = open.read_only();
    effect(move || {
        token.report(open_watch.get());
    });
    let (bar_w, bar_h, color) = (move || body().0, move || body().1, move || body().2);
    popover()
        .open(move || Some(open.get()))
        .on_open_change(move |e: &PopoverEvent| {
            if let PopoverEvent::OpenChange(shown, _) = *e {
                open.set(shown);
            }
        })
        .popup_class("bg-background border border-border rounded-md shadow-md p-3")
        .trigger(
            // 锚点载体 = 按压源（headless id: 命中可按压件，period_label 先例同形）。
            view()
                .test_id(anchor.as_str())
                .on_press(move |_: &PressEvent| open.set(!open.get_untracked()))
                .child(
                    view()
                        .w(move || px(bar_w()))
                        .h(move || px(bar_h()))
                        .rounded_sm()
                        .bg(color),
                ),
        )
        .child(view().flex_col().gap_1().max_w(px(300.0)).child(each(
            lines,
            |s: &String| s.clone(),
            move |item: ReadSignal<String>, _index: ReadSignal<usize>| {
                text(item.get())
                    .text_size(px(12.0))
                    .text_color(foreground())
            },
        )))
}

// ---------------------------------------------------------------------------
// 纵向分组柱（月度收支图，#2048）
// ---------------------------------------------------------------------------

/// 纵向分组柱单根柱（值 + 语义色；语义色与值口径归调用方/装配层）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VerticalBarFace {
    /// 柱值（整数分）。
    pub cents: i64,
    pub color: AdaptiveColor,
}

/// 纵向分组柱组行（一个月）：三根并列柱 + x 轴标签 + 行说明（按行弹出）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VerticalGroupFace {
    /// x 轴行标（月份键 YYYY-MM，旧壳 labels 同形直用）。
    pub label: String,
    /// 并列柱（序 = 展示序；旧壳 datasets 固定序：收入/支出/退款）。
    pub bars: Vec<VerticalBarFace>,
    /// 行说明完整内容行序（调用方装配；含三列毛值 + 净额，旧壳 afterBody 同界）。
    pub tooltip: Vec<String>,
    /// 逐柱按压锚点前缀（headless `id:` 命中载体，逐柱加后缀 `-0/1/…`）。
    pub anchor_prefix: String,
}

/// 绘制高（pt）——纵向图的柱区高度。
const PLOT_HEIGHT: f32 = 180.0;

/// 纵向分组柱图：组行等分宽、组内柱并列、柱高 ∝ 值 / 全图最大正值（全图同
/// 线性标度，组间可比——旧壳 y 轴同一线性标度同义）。
pub(crate) fn vertical_grouped_bars(
    groups: impl IntoReactive<Vec<VerticalGroupFace>> + Copy + 'static,
) -> View {
    let groups = groups.into_reactive();
    let groups_for_max = groups.clone();
    let max = computed(move || {
        groups_for_max
            .get()
            .iter()
            .flat_map(|g| g.bars.iter().map(|b| b.cents))
            .fold(0i64, i64::max)
            .max(1)
    });
    view()
        .test_id("reports-monthly-chart")
        .flex_row()
        .items_start()
        .gap_1()
        .child(each(
            move || groups.get(),
            |g: &VerticalGroupFace| g.label.clone(),
            move |item: ReadSignal<VerticalGroupFace>, _index: ReadSignal<usize>| {
                group_column(item, move || max.get())
            },
        ))
}

/// 单组列（柱行 + x 轴标签）：柱高闭包实读组行信号（响应式随数据重绘）。
fn group_column(
    item: ReadSignal<VerticalGroupFace>,
    max: impl Fn() -> i64 + Copy + 'static,
) -> impl IntoElement {
    let label_id = move || format!("reports-month-label-{}", item.get().label);
    view()
        .flex_1()
        .flex_col()
        .items_center()
        .justify_end()
        .gap_1()
        .child(view().flex_row().items_end().gap_1().child(each(
            move || item.get().bars,
            |b: &VerticalBarFace| (b.cents, b.color.light.h.to_bits()),
            move |bar: ReadSignal<VerticalBarFace>, index: ReadSignal<usize>| {
                vertical_bar_popover(item, bar, index, max)
            },
        )))
        .child(
            text(move || item.get().label)
                .text_size(px(11.0))
                .text_color(muted_foreground())
                .number_of_lines(1)
                .test_id(label_id().as_str()),
        )
}

/// 纵向单柱：柱高 ∝ 值 / 全图最大值（响应式随数据重算），0/负值零高不呈体；
/// 气泡内容随组行信号。
fn vertical_bar_popover(
    item: ReadSignal<VerticalGroupFace>,
    bar: ReadSignal<VerticalBarFace>,
    index: ReadSignal<usize>,
    max: impl Fn() -> i64 + Copy + 'static,
) -> impl IntoElement {
    let anchor = format!(
        "{}-{}",
        item.get_untracked().anchor_prefix,
        index.get_untracked()
    );
    bar_popover(
        anchor,
        move || item.get().tooltip.clone(),
        move || {
            let cents = bar.get().cents;
            let color = bar.get().color;
            let height = if cents > 0 {
                (cents as f32 / max() as f32 * PLOT_HEIGHT).max(2.0)
            } else {
                0.0
            };
            (BAR_THICKNESS, height, color)
        },
    )
}

// ---------------------------------------------------------------------------
// 横向单柱行（支出分类构成图，#2048）
// ---------------------------------------------------------------------------

/// 名称列宽（pt，行名截断预算）与柱尾金额区宽（pt）。
pub(crate) const HBAR_NAME_WIDTH: f32 = 140.0;
pub(crate) const HBAR_VALUE_WIDTH: f32 = 96.0;

/// 横向单柱行（一根柱）规格：名称按压（下钻/跳转，回调归调用方）与条身按压
///（说明气泡）分离——两者不必同触发。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HorizontalRowFace {
    /// 行名按压锚点（headless `id:` 命中）。
    pub name_anchor: String,
    /// 条身（气泡）按压锚点。
    pub bar_anchor: String,
    pub name: String,
    /// 条长比例（0..1；基准与归一归调用方装配——占比/比例口径单源）。
    pub bar_ratio: f64,
    /// 条色。
    pub bar_color: AdaptiveColor,
    /// 柱尾金额标注（旧壳 barEndAmountPlugin 默认标注 = 金额；占比开启形态
    /// 随视图侧交互面拼装后由调用方传文）。
    pub end_label: String,
    /// 条身说明（占比分母随层级的口径在装配层）。
    pub tooltip: Vec<String>,
    /// 行高（pt；旧壳 CATEGORY_ROW_HEIGHT 32 的行高）。
    pub row_height: f32,
}

/// 横向单柱行图（平铺不滚动——滚动容器与高度由调用方承担）。
/// 名称按压回调入参 = 行序（调用方以序取装配产物造下钻/跳转载荷）。
pub(crate) fn horizontal_bars(
    rows: impl IntoReactive<Vec<HorizontalRowFace>> + Copy + 'static,
    content_width: impl Fn() -> f32 + Copy + 'static,
    on_name_press: impl Fn(usize) + Copy + 'static,
) -> View {
    let rows = rows.into_reactive();
    view()
        .flex_col()
        .test_id("reports-category-chart")
        .child(each(
            move || rows.get(),
            |r: &HorizontalRowFace| r.name_anchor.clone(),
            move |item: ReadSignal<HorizontalRowFace>, index: ReadSignal<usize>| {
                horizontal_row(item, index, content_width, on_name_press)
            },
        ))
}

/// 单行：名称（按压 = 下钻）+ 条身（气泡）+ 柱尾金额标注；三者同排不互换
/// 结构（旧壳 canvas 形态按行分解为三段，读感「行名在类目轴、条 + 标注同行」）。
fn horizontal_row(
    item: ReadSignal<HorizontalRowFace>,
    index: ReadSignal<usize>,
    content_width: impl Fn() -> f32 + Copy + 'static,
    on_name_press: impl Fn(usize) + Copy + 'static,
) -> impl IntoElement {
    let pos = index.get_untracked();
    let row_height = item.get_untracked().row_height;
    let name_anchor = item.get_untracked().name_anchor.clone();
    let end_label = item.get_untracked().end_label.clone();
    let bar_anchor = item.get_untracked().bar_anchor.clone();
    // 条长非法回落 0（不越轨）；轨道宽 = 视口宽 − 名称列 − 柱尾值宽 − 缝。
    let bar_len = move || {
        let ratio = item.get().bar_ratio.clamp(0.0, 1.0);
        ((content_width() - HBAR_NAME_WIDTH - HBAR_VALUE_WIDTH).max(0.0) * ratio as f32)
            .min(content_width())
    };
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(row_height))
        .child(
            // 名称按压源（下钻/跳转；行名紧跟类目轴语义——条与标注右移一行）。
            view()
                .test_id(name_anchor.as_str())
                .on_press(move |_: &PressEvent| on_name_press(pos))
                .w(px(HBAR_NAME_WIDTH))
                .child(
                    text(move || item.get().name)
                        .text_size(px(13.0))
                        .text_color(foreground())
                        .number_of_lines(1),
                ),
        )
        .child(bar_popover(
            bar_anchor,
            move || item.get().tooltip.clone(),
            move || {
                let (w, h, color) = (
                    bar_len(),
                    BAR_THICKNESS.max(row_height * 0.44),
                    item.get().bar_color,
                );
                (w, h, color)
            },
        ))
        .child(
            // 柱尾金额标注（旧壳 barEndAmountPlugin 标在条端右侧同位）。
            text(end_label)
                .text_size(px(12.0))
                .text_color(muted_foreground())
                .number_of_lines(1),
        )
}

// ---------------------------------------------------------------------------
// 单测（口径外自测：色板槽位深度一致；呈现路径归 headless 组件测试）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 色板槽位深度 = 装配层口径常量（散列不越界的前置）、未分类灰与分类色
    /// 不同（一眼区分的口头约定必须实质成立）。
    #[test]
    fn palette_len_matches_assembly_constant_and_slots_are_stable() {
        assert_eq!(REPORT_PALETTE_LEN, 12);
        assert_eq!(PALETTE.len(), REPORT_PALETTE_LEN);
        // 同 id 恒同色。
        assert_eq!(category_color("r-food"), category_color("r-food"));
        // 越界回绕不断渲染。
        assert_eq!(
            palette_color(REPORT_PALETTE_LEN),
            palette_color(0),
            "越界回绕（防崩而非 panic）"
        );
        assert_ne!(
            uncategorized_color(),
            category_color("r-food"),
            "未分类灰 ≠ 任一分类色"
        );
    }
}
