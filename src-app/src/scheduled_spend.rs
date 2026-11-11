//! 订阅花费面板（issue #2042；旧壳 `SubscriptionSpendPanel.vue` 的重生——
//! 订阅花费双口径分析区，ADR-0023 决策二）。
//!
//! 实际花费——本月 / 本年 + 过去 12 个月逐月趋势（不摊销，忠实统计期次生成
//! 的流水）；推算成本——折算月 / 年成本两个数（只统计进行中订阅，系数收口
//! 在后端 scheduled 域 `query_subscription_spend` 单点，本壳零口径逻辑只
//! 渲染——旧壳「数据全部来自只读聚合命令，前端零口径逻辑」同位）。
//!
//! 趋势图：旧壳 chart.js 柱状图的 NSH 自绘对应形态（ADR-0145 决策 5：line
//! 用 NSH 内建 plot、bar 自绘——投资线图随投资票走内建，本票先建自绘柱图）。
//! 每月一根柱（高度按与窗口峰值的比例），月键标签在柱下；金额文本的隐私
//! 掩码口径同 [`assemble_spend`]（柱高不掩码——形状呈现，可读数字由行表与
//! 统计数承担掩码，与旧壳 tooltip / 轴刻度掩码同义）。
//!
//! 数据口径由 [`assemble_spend`] 纯函数承担（花费口径的装配层单测钉点）；
//! 读失败呈现失败态，不静默混算（ADR-0023，旧壳 loadFailed 同口径）。
//!
//! 界面文案仅 zh-CN。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::assembly::{ColumnAlign, CurrencyTable};
use crate::scheduled::{amount_text, status_label};

/// 花费面板卡标题（旧壳 `scheduled.spend.title`）。
const SPEND_TITLE: &str = "实际花费";

/// 花费行格单元格（逐订阅行的装配产物）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SpendRowCells {
    pub plan_id: String,
    pub name: String,
    pub status: String,
    pub this_month: String,
    pub this_year: String,
}

/// 趋势柱（12 个月逐月的自绘数据）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SpendBar {
    /// 日历月 `YYYY-MM`。
    pub month: String,
    /// 柱高像素（与窗口峰值成比例；无花费月为 0）。
    pub height: f32,
    /// 金额展示文本（隐私口径同 [`amount_text`]）。
    pub amount_text: String,
}

/// 花费面板的装配产物（装载批一次性产出，渲染期零格式化）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SpendCells {
    /// 单位衬注（`单位：{currency}（本位币）· 不摊销`）。
    pub caption: String,
    pub this_month: String,
    pub this_year: String,
    pub projected_month: String,
    pub projected_year: String,
    /// 过去 12 个月趋势柱（含当月，旧→新）。
    pub bars: Vec<SpendBar>,
    /// 峰值金额文本（柱侧标签；全零窗口占位「—」）。
    pub peak_text: String,
    pub rows: Vec<SpendRowCells>,
}

/// 订阅花费总览 → 装配产物（旧壳 SubscriptionSpendPanel 各段 render 的装配层
/// 收口）：逐订阅行列名「备注 ?? 商户名 ?? —」（旧壳行内 note / merchant_name
/// 同口径）；本位币格式化消费币种字典（缺条目走缺省展示参数，不静默换符
/// 号）；隐私开启全量掩码（旧壳 formatAmount 内嵌口径）。**不静默混算**：
/// overview 只在域读取成功时被传入；读失败由视图呈现失败态。推算成本只计
/// 进行中订阅的过滤口径已在域内收口（`query_projected_cost` 只扫 active），
/// 本函数不做第二份过滤。
pub(crate) fn assemble_spend(
    overview: &ledger_scheduled::SubscriptionSpendOverview,
    currencies: &CurrencyTable,
    privacy_enabled: bool,
) -> SpendCells {
    let fmt = currencies.format_of(&overview.native_currency);
    let text = |cents: i64| amount_text(cents, &fmt, privacy_enabled);
    let peak = overview
        .months
        .iter()
        .map(|m| m.native_cents)
        .max()
        .unwrap_or(0);
    let bars = overview
        .months
        .iter()
        .map(|m| SpendBar {
            month: m.month.clone(),
            height: if peak <= 0 {
                0.0
            } else {
                108.0 * (m.native_cents as f64 / peak as f64) as f32
            },
            amount_text: text(m.native_cents),
        })
        .collect();
    SpendCells {
        caption: format!("单位：{}（本位币）· 不摊销", overview.native_currency),
        this_month: text(overview.this_month_native_cents),
        this_year: text(overview.this_year_native_cents),
        projected_month: text(overview.projected_month_native_cents),
        projected_year: text(overview.projected_year_native_cents),
        peak_text: if peak > 0 {
            text(peak)
        } else {
            "—".to_owned()
        },
        bars,
        rows: overview
            .rows
            .iter()
            .map(|row| SpendRowCells {
                plan_id: row.plan_id.clone(),
                name: row
                    .note
                    .clone()
                    .or_else(|| row.merchant_name.clone())
                    .unwrap_or_else(|| "—".to_owned()),
                status: status_label(&row.status),
                this_month: text(row.this_month_native_cents),
                this_year: text(row.this_year_native_cents),
            })
            .collect(),
    }
}

/// 花费面板装载态（视图信号持有）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SpendPanelState {
    /// 装配产物（None = 尚未装载成功，面板卡不渲染——旧壳 NSpin / NEmpty
    /// 门控同果）。
    pub cells: Option<SpendCells>,
    /// 读失败：呈现「加载失败，请重试」失败态，不静默混算（失败时 cells 不
    /// 落位——旧壳 loadFailed v-if 同果）。
    pub failed: bool,
}

impl SpendPanelState {
    /// 成功落位。
    pub fn settled(cells: SpendCells) -> Self {
        Self {
            cells: Some(cells),
            failed: false,
        }
    }

    /// 失败落位（呈现失败态卡）。
    pub fn failed() -> Self {
        Self {
            cells: None,
            failed: true,
        }
    }
}

/// 花费面板（订阅页签装载批的数据面之一）：标题卡 + 四统计 + 12 月趋势柱 +
/// 逐订阅行表。数据随页签装载批一并到达（重拉归页签 reload 回调），本面板
/// 零独立读路径（旧壳 onMounted reload 的对应物 = 页签装载批携带本面板
/// 数据）。
pub(crate) fn spend_panel(state: Signal<SpendPanelState>) -> impl IntoElement {
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());
    let bar_color = hsla(0.58, 0.82, 0.52, 0.55).into();

    when(
        move || state.get().failed,
        move || {
            view()
                .flex_col()
                .gap_2()
                .app_class("rounded-md border border-border p-3")
                .child(
                    text(SPEND_TITLE)
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg),
                )
                .child(
                    text("加载失败，请重试")
                        .text_size(px(12.0))
                        .text_color(muted)
                        .test_id("spend-failed"),
                )
        },
    )
    .otherwise(move || {
        // 就绪态：常挂框架 + 汇总文本/趋势柱走响应式读（ reloading 的
        // 数值变化就地刷新——when 分支不重跑，静态克隆会持首值，见
        // PlanDetailModal 同款结构教训）。整卡数据已就绪才挂（消费面
        // 锚点不会在无数据时出现）。
        let value = |pick: fn(&SpendCells) -> String| {
            move || state.get().cells.as_ref().map(pick).unwrap_or_default()
        };
        view()
            .flex_col()
            .gap_2()
            .app_class("rounded-md border border-border p-3")
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        text(SPEND_TITLE)
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg),
                    )
                    .child(
                        text(value(|c| c.caption.clone()))
                            .text_size(px(12.0))
                            .text_color(muted),
                    ),
            )
            .child(
                // 统计行（实际花费两数 + 推算成本两数与说明）。
                view()
                    .flex_row()
                    .flex_wrap()
                    .items_end()
                    .gap_8()
                    .child(stat_block(
                        "本月实际花费",
                        value(|c| c.this_month.clone()),
                        "spend-this-month",
                        fg,
                        muted,
                    ))
                    .child(stat_block(
                        "本年实际花费",
                        value(|c| c.this_year.clone()),
                        "spend-this-year",
                        fg,
                        muted,
                    ))
                    .child(stat_block(
                        "折算月成本（推算）",
                        value(|c| c.projected_month.clone()),
                        "spend-projected-month",
                        fg,
                        muted,
                    ))
                    .child(stat_block(
                        "折算年成本（推算）",
                        value(|c| c.projected_year.clone()),
                        "spend-projected-year",
                        fg,
                        muted,
                    ))
                    .child(
                        text("推算：只计进行中的计划")
                            .text_size(px(12.0))
                            .text_color(muted),
                    ),
            )
            .child(
                // 12 月趋势柱（自绘标尺：右上峰值标签 = 纵轴顶值）。
                view()
                    .flex_col()
                    .gap_1()
                    .child(
                        text(move || {
                            let snapshot = state.get();
                            format!(
                                "峰值 {}",
                                snapshot
                                    .cells
                                    .as_ref()
                                    .map(|c| c.peak_text.clone())
                                    .unwrap_or_default()
                            )
                        })
                        .text_size(px(11.0))
                        .text_color(muted)
                        .test_id("spend-peak"),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_end()
                            .gap(px(4.0))
                            .h(px(150.0))
                            .test_id("spend-trend")
                            .child(bar_row(state, bar_color)),
                    ),
            )
            .child(
                // 逐订阅行表（含已取消 / 暂停计划，历史花费如实保留；
                // 行操作列无边——花费面是只读呈现面）。
                view()
                    .test_id("spend-rows")
                    .flex_1()
                    .min_h(px(100.0))
                    .child(
                        nsh_base::data_table()
                            .columns(spend_row_columns())
                            .data(move || spend_rows_data(&state))
                            .classes(crate::transactions::table_classes())
                            .flex_1(),
                    ),
            )
    })
}

/// 统计块（标签 + 数值）。
fn stat_block(
    label: &'static str,
    value: impl Fn() -> String + 'static,
    id: &'static str,
    fg: nsh_core::element::AdaptiveColor,
    muted: nsh_core::element::AdaptiveColor,
) -> View {
    view()
        .flex_col()
        .gap_1()
        .child(text(label).text_size(px(12.0)).text_color(muted))
        .child(
            text(value)
                .text_size(px(20.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .test_id(id),
        )
}

/// 自绘柱行（12 个月一行，末端对齐的柱体 + 月键标签）。柱高随窗口峰值
/// 比例变化（柱高比例归 [`assemble_spend`]）；数据源响应式（重拉后就地
/// 刷新——`each` 键 = 月键，键稳定行复用）。
fn bar_row(
    state: Signal<SpendPanelState>,
    bar_color: nsh_core::element::AdaptiveColor,
) -> impl IntoElement {
    let muted = nsh_core::element::tokens()
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());
    each(
        move || {
            state
                .get()
                .cells
                .as_ref()
                .map(|c| c.bars.clone())
                .unwrap_or_default()
        },
        |b: &SpendBar| b.month.clone(),
        move |item: ReadSignal<SpendBar>, _index: ReadSignal<usize>| {
            let bar = item.get();
            view()
                .flex_col()
                .items_center()
                .justify_end()
                .gap(px(2.0))
                .flex_1()
                .child(
                    view()
                        .w_full()
                        .h(move || px(bar.height.max(2.0)))
                        .bg(bar_color),
                )
                .child(text(bar.month).text_size(px(9.0)).text_color(muted))
        },
    )
}

/// 花费行表列定义（旧壳 rowColumns 四列：订阅/状态/本月实际花费/本年实际
/// 花费；金额右对齐）。
fn spend_row_columns() -> Vec<nsh_base::DataTableColumn> {
    let defs = [
        ("subscription", "订阅", 160.0, ColumnAlign::Start),
        ("status", "状态", 80.0, ColumnAlign::Start),
        ("this_month", "本月实际花费", 110.0, ColumnAlign::End),
        ("this_year", "本年实际花费", 110.0, ColumnAlign::End),
    ];
    defs.into_iter()
        .map(|(key, title, width, align)| {
            let mut column = nsh_base::DataTableColumn::default();
            column.key = key.into();
            column.title = Some(title.into());
            column.width = Some(Points(width));
            column.align = match align {
                ColumnAlign::Start => nsh_base::DataTableAlign::Start,
                ColumnAlign::End => nsh_base::DataTableAlign::End,
            };
            column
        })
        .collect()
}

/// 行表数据（闭包读面板信号快照；行格文本已是装配产物）。追踪读（DataTable
/// 的 data prop 响应式闭包，订阅页签清单先例同口径）。
fn spend_rows_data(state: &Signal<SpendPanelState>) -> nsh_base::DataTableRows {
    let cells = state.get().cells.unwrap_or_default();
    nsh_base::DataTableRows::new(cells.rows.len())
        .texts("subscription", cells.rows.iter().map(|r| r.name.as_str()))
        .texts("status", cells.rows.iter().map(|r| r.status.as_str()))
        .texts(
            "this_month",
            cells.rows.iter().map(|r| r.this_month.as_str()),
        )
        .texts("this_year", cells.rows.iter().map(|r| r.this_year.as_str()))
}

// ---------------------------------------------------------------------------
// lib 单测（花费口径：12 月窗口 / 比例柱 / 掩码全覆盖 / 行名与状态口径）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_currencies::Currency;
    use ledger_scheduled::{SubscriptionMonthSpend, SubscriptionSpendRow};

    fn overview() -> ledger_scheduled::SubscriptionSpendOverview {
        ledger_scheduled::SubscriptionSpendOverview {
            native_currency: "CNY".to_owned(),
            this_month_native_cents: 2_500,
            this_year_native_cents: 15_000,
            months: vec![
                SubscriptionMonthSpend {
                    month: "2026-01".to_owned(),
                    native_cents: 3_000,
                },
                SubscriptionMonthSpend {
                    month: "2026-02".to_owned(),
                    native_cents: 4_500,
                },
                SubscriptionMonthSpend {
                    month: "2026-03".to_owned(),
                    native_cents: 2_500,
                },
                SubscriptionMonthSpend {
                    month: "2026-04".to_owned(),
                    native_cents: 0,
                },
            ],
            rows: vec![
                SubscriptionSpendRow {
                    plan_id: "p-1".to_owned(),
                    note: Some("视频会员".to_owned()),
                    merchant_name: Some("视频平台".to_owned()),
                    status: "active".to_owned(),
                    amount_cents: 3_000,
                    currency_code: "CNY".to_owned(),
                    this_month_native_cents: 2_500,
                    this_year_native_cents: 15_000,
                },
                SubscriptionSpendRow {
                    plan_id: "p-2".to_owned(),
                    note: None,
                    merchant_name: None,
                    status: "cancelled".to_owned(),
                    amount_cents: 100,
                    currency_code: "CNY".to_owned(),
                    this_month_native_cents: 0,
                    this_year_native_cents: 3_000,
                },
            ],
            projected_month_native_cents: 7_000,
            projected_year_native_cents: 84_000,
        }
    }

    fn currencies() -> CurrencyTable {
        let currency = Currency {
            code: "CNY".to_owned(),
            name: "人民币".to_owned(),
            symbol: "¥".to_owned(),
            decimal_places: 2,
        };
        CurrencyTable::from_currencies([&currency])
    }

    #[test]
    fn assemble_spend_cells_match_panel_vocabulary() {
        let cells = assemble_spend(&overview(), &currencies(), false);
        assert_eq!(cells.caption, "单位：CNY（本位币）· 不摊销");
        assert_eq!(
            cells.this_month, "¥25",
            "本月实际花费 = ¥25（2500 分，CNY 两位小数）"
        );
        assert_eq!(cells.this_year, "¥150", "本年实际花费按本位币忠实统计");
        assert_eq!(
            cells.projected_month, "¥70",
            "折算月成本（域内已按周期系数折算）"
        );
        assert_eq!(cells.projected_year, "¥840", "折算年成本 = 折算月成本 × 12");
        assert_eq!(cells.peak_text, "¥45", "峰值 = 12 月窗口最大月花费");
        // 行名 = note ?? merchant_name ?? —（旧壳同口径）；状态经 status_label。
        assert_eq!(cells.rows[0].name, "视频会员");
        assert_eq!(cells.rows[0].this_year, "¥150");
        assert_eq!(cells.rows[1].name, "—");
        assert_eq!(cells.rows[1].status, "已取消", "已取消计划历史花费如实保留");
    }

    #[test]
    fn spend_rows_include_non_active_plans_with_history() {
        let cells = assemble_spend(&overview(), &currencies(), false);
        assert_eq!(cells.rows.len(), 2, "行不过滤计划状态");
        assert_eq!(
            cells.rows[1].this_year, "¥30",
            "已取消计划的当月 0、历史窗内花费如常"
        );
    }

    #[test]
    fn spend_bars_are_proportional_and_complete() {
        let cells = assemble_spend(&overview(), &currencies(), false);
        assert_eq!(cells.bars.len(), 4, "12 月窗口逐月（含当月）全量柱");
        assert!(
            (cells.bars[0].height - 108.0 * 3000.0 / 4500.0).abs() < 0.01,
            "柱高与峰值成比例：{:?}",
            cells.bars[0].height
        );
        assert_eq!(cells.bars[3].height, 0.0, "无花费月零高度（形状不出数字）");
        assert_eq!(cells.bars[1].amount_text, "¥45", "柱侧金额 = 月内实际花费");
    }

    #[test]
    fn spend_masks_all_amount_texts_when_privacy_enabled() {
        let cells = assemble_spend(&overview(), &currencies(), true);
        for text in [
            cells.this_month.clone(),
            cells.this_year.clone(),
            cells.projected_month.clone(),
            cells.projected_year.clone(),
            cells.peak_text.clone(),
        ] {
            assert_eq!(text, "••••", "隐私掩码覆盖四个统计与峰值");
        }
        assert!(cells.bars.iter().all(|b| b.amount_text == "••••"));
        assert!(
            cells
                .rows
                .iter()
                .all(|r| r.this_month == "••••" && r.this_year == "••••")
        );
    }

    #[test]
    fn spend_rows_do_not_filter_by_unfinished_status() {
        // 面板行不区分计划进行与否（旧壳不过滤状态）；is_unfinished_status
        // 的唯一消费面是行操作矩阵——本测钉「面板不做第二份过滤」的口径边界。
        let cells = assemble_spend(&overview(), &currencies(), false);
        assert!(crate::scheduled::is_unfinished_status("active"));
        assert!(!cells.rows.is_empty());
    }
}
