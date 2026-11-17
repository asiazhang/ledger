// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。

//! 列表卡装配 helper（issue #2156，#2135 review 判断性发现的单点）：卡片
//! flex_1 容器 + 自然高标题 + flex_1 虚拟滚动表格容器三件，items /
//! accounts / merchants / policies / savings 五处列表视图消费。
//!
//! 装配纪律住本单点（历史五处手写同构，一处布局纪律变化要逐文件改五处，
//! 复制漂移即 #2084 盘点到的容器空白缺陷，#2135 布局对齐线逐处修复）：
//! 卡片自身 flex_1——虚拟化表格取弹性链高度，卡片不定高则虚拟化量出零高、
//! 行不呈现（#2041 先例）；卡标题自然高度、不参与弹性分配（#2135 布局对
//! 齐线：flex_1 标题行曾与表格容器对分卡内剩余高度，少数据时表头被推到
//! 卡内中部，web 卡头即标题）；表格容器 flex_1 承接卡内剩余高度，test_id
//! 即调用方传入的 `table_id`（headless 锚点面不变）。
//!
//! `before` / `after` 收纳标题与表格之间（过滤意图行等）、表格之后（分页
//! 条等）的额外行，纯三件调用面传 `None`。行为零变化重构：样式与结构按
//! 消费面原样搬入，不改变任何行为与 test_id 面（既有 headless 旅程即护栏）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::element::{AnyElement, FontWeight};
use nsh_core::prelude::*;

use crate::design::font_size;

/// 列表卡装配：卡片容器（flex_1 + flex_col + gap_2 + 卡片圆角描边内衬）+
/// 自然高标题（SECONDARY / SEMIBOLD / 前景）+ flex_1 表格容器（test_id =
/// `table_id`，child = `table`）三件；`before` / `after` 收纳卡内额外行。
pub(crate) fn list_card_of(
    title: &'static str,
    fg: AdaptiveColor,
    before: Option<AnyElement>,
    table_id: &'static str,
    table: impl IntoElement,
    after: Option<AnyElement>,
) -> AnyElement {
    view()
        .flex_1()
        .flex_col()
        .gap_2()
        .app_class("rounded-(--radius-card) border border-border p-3")
        .child(
            // 卡标题自然高度（#2135 布局对齐线）：标题不参与弹性分配，
            // 否则少数据时表头被推到卡内中部。
            text(title)
                .text_size(px(font_size::SECONDARY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg),
        )
        .children(before)
        .child(view().test_id(table_id).flex_1().child(table))
        .children(after)
        .into_element()
}
