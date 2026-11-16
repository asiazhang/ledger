//! 聚焦行滚动定位（#2140）的共享接缝：来源跳转落点「行高亮 + 滚动定位」中
//! 「高亮行滚进视口」一段的单一来源（code-review 重复代码收口：物品视图
//! [`crate::items`] 与保单视图 [`crate::policies`] 两消费面逐字同形的
//! scroll_to_row effect 块归此一处）。
//!
//! 机制：封装层虚拟滚动容器的定位句柄（DataTable 的组件 ref →
//! scroll_to_row）在 focus 消费后把高亮行滚进视口。触发面 = 高亮 × 行集
//! （行集落位或失效重读变化即滚——高亮保持到实例消亡，重读后仍归位）。等
//! 一帧再调：行集 prop 与行高在下一帧提交后组件才可读，本帧直读是旧行集。
//! 聚焦 id 不在行集（软删无锚）不滚；行集短于视口时 scroll_to_row 钳制在
//! 滚动范围内，空转幂等；窄窗卡片形态无虚拟滚动容器、高亮即全可见，本接缝
//! 不涉及。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{DataTable, DataTableApi};
use nsh_core::prelude::*;

/// 把「聚焦行索引闭包」命中的行滚进视口：`focused_index` 在 effect 内读取
/// （高亮 × 行集两路信号的响应式读都在闭包内），命中行后等一帧调
/// `scroll_to_row`。`table_ref` 为调用方视图体构造并挂到 DataTable 上的组
/// 件 ref（Copy 句柄，调用方继续持有不影响）；任务挂 `view_owner`（视图实
/// 例 owner，弹层卸载不取消在途滚动）。
pub(crate) fn scroll_focused_row_into_view(
    table_ref: ComponentRef<DataTable>,
    focused_index: impl Fn() -> Option<u32> + Copy + 'static,
    view_owner: Owner,
) {
    effect(move || {
        let Some(index) = focused_index() else {
            return;
        };
        view_owner.run(move || {
            spawn(async move {
                next_frame().await;
                if let Some(api) = table_ref.api::<DataTableApi>() {
                    api.scroll_to_row(index, None);
                }
            });
        });
    });
}
