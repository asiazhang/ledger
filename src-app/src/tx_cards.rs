//! 交易卡片列表（issue #2057 窄窗自适应横切；旧壳 `TransactionCardList.vue` 的
//! 本壳重生，ADR-0088 决策 9 断点双渲染）。
//!
//! **装配复用纪律**（票面 Implementation Decisions）：卡片格文本与宽形态同一
//! 产物——渲染只消费装配层 [`crate::assembly::transaction_cards`]（同一
//! [`crate::assembly::assemble_rows`] 行集的卡片视角投影），本模块零格式化、
//! 零词表、零口径。
//!
//! **两消费方同构复用**（旧壳 #846 同构）：交易列表（可交互——卡片「⋯」与行
//! 右键共用同一菜单入口）与搜索结果（只读，无菜单）。字段口径沿旧壳卡片：
//! 头部（日期 + 类型)、分类 · 商户合并行、账户行（转账双向文本同宽形态）、
//! 来源行、购买项块（逐项行）、金额行（语义色与宽形态同源）。
//!
//! **偏差留痕**：旧壳「整卡点击 = 行激活」不上——NSH press 无阻断冒泡基元
//! （卡根激活监听与卡内「⋯」菜单触发器冒泡冲突）；编辑入口 = 卡片「⋯」菜单
//! 首个菜单项（一击可达，语义与旧壳行激活等价、交互步数 +1，见交付报告）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::assembly::{RowCells, TransactionCard};
use crate::transactions::amount_color;
use crate::views::view_palette;

/// 交易卡片列表（可交互消费方：卡片「⋯」经 `open_menu` 打开与表格行右键同一
/// 菜单——目标交易随卡片固定，派生行同 id 的操作目标恒是交易本体）。挂载点带
/// `tx-cards` 键（headless 锚点，窄窗双渲染断言消费面）。
pub(crate) fn transaction_cards<M>(rows: Signal<Vec<RowCells>>, open_menu: M) -> View
where
    M: Fn(&TransactionCard, Point<Points>) + Copy + 'static,
{
    let palette = view_palette();
    card_list(rows, Some(open_menu), palette.fg, palette.muted)
}

/// 交易卡片列表（只读消费方：搜索结果复用同构卡片形态，无菜单无写入口——旧
/// 壳搜索结果不传 openRowMenu 的同口径）。
pub(crate) fn readonly_cards(rows: Signal<Vec<RowCells>>) -> View {
    let palette = view_palette();
    card_list::<fn(&TransactionCard, Point<Points>)>(rows, None, palette.fg, palette.muted)
}

/// 卡片列表本体（滚动容器 + 响应式键控列表；键 = 交易 id）：两消费方共用，
/// `open_menu` 只有交互消费方给（只读方 `None`——菜单触发器不挂载）。
fn card_list<M>(
    rows: Signal<Vec<RowCells>>,
    open_menu: Option<M>,
    fg: AdaptiveColor,
    muted: AdaptiveColor,
) -> View
where
    M: Fn(&TransactionCard, Point<Points>) + Copy + 'static,
{
    view().flex_1().child(
        view().test_id("tx-cards").flex_1().child(
            scroll()
                .flex_1()
                .flex_col()
                .gap_2()
                .px_3()
                .py_2()
                .child(each(
                    move || crate::assembly::transaction_cards(&rows.get()),
                    |card| card.id.clone(),
                    move |card: ReadSignal<TransactionCard>, index: ReadSignal<usize>| {
                        card_view(card.get(), index.get(), open_menu, fg, muted)
                    },
                )),
        ),
    )
}

/// 单卡（旧壳 TransactionCard 类同位）：头部（日期 + 类型 +「⋯」）、分类 ·
/// 商户行、账户行、来源行、购买项块、金额行。空格不占行（订单级格随块首行
/// 装配恒带值，「-」空值口径照常呈现）。
fn card_view<M>(
    card: TransactionCard,
    index: usize,
    open_menu: Option<M>,
    fg: AdaptiveColor,
    muted: AdaptiveColor,
) -> View
where
    M: Fn(&TransactionCard, Point<Points>) + Copy + 'static,
{
    view()
        .test_id("tx-card")
        .flex_col()
        .gap_1()
        .px_3()
        .py_2()
        .rounded_md()
        .app_class("bg-background border border-border")
        // 头部：日期（弱化）+ 类型（主分量）；交互消费方行尾「⋯」（test_id 落
        // 在文本上——headless id: 锚点命中文本、物理命中到可按压件，先例期间
        // 芯片同形；序号锚点使命中面随多卡唯一）。
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text(card.date.clone())
                                .text_size(px(12.0))
                                .text_color(muted)
                                .test_id("tx-card-date"),
                        )
                        .child(
                            text(card.kind.clone())
                                .text_size(px(13.0))
                                .text_color(fg)
                                .test_id("tx-card-kind"),
                        ),
                )
                .children(open_menu.map({
                    let menu_card = card.clone();
                    move |open_menu: M| {
                        let for_menu = menu_card;
                        view()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .on_press(move |e: &PressEvent| open_menu(&for_menu, e.page_position))
                            .child(
                                text("⋯")
                                    .text_size(px(16.0))
                                    .text_color(muted)
                                    .test_id(format!("tx-card-menu-{index}").as_str()),
                            )
                    }
                })),
        )
        // 分类 · 商户合并行。
        .child(meta_line(&card.meta, fg))
        // 账户行（转账双向 / 单账户 / 出资分解标注的文本与宽形态同源）。
        .child(meta_line(&card.account, muted))
        // 来源行（来源名 + 状态标注）。
        .child(meta_line(&card.source, muted))
        // 购买项块（逐项：商品名 + 共 N 件；缩进两档与备注列语义一致）。
        .children(
            card.purchases
                .iter()
                .map(|purchase| meta_line(purchase, muted).app_class("pl-2")),
        )
        // 金额行：末端对齐（数字惯例与宽形态金额列同口径）+ 语义色。
        .child(
            view().flex_row().justify_end().child(
                text(card.amount.clone())
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(amount_color(card.amount_semantic))
                    .test_id("tx-card-amount"),
            ),
        )
}

/// 卡内信息行：单行截断（对齐表格自绘格行面约束——长文本在窄窗下不挤压金额
/// 读感）。
fn meta_line(body: &str, color: AdaptiveColor) -> View {
    view().flex_row().child(
        text(body.to_owned())
            .text_size(px(13.0))
            .text_color(color)
            .number_of_lines(1),
    )
}
