//! visited-once keep-mounted 页签宿主（crate 级单点，#2073）。
//!
//! 机制是旧壳 `display-directive="show:lazy"` + 显式 key 的 NSH 对应物（先例
//! [`crate::settings`] / [`crate::investments`] 两页签视图曾各写一份同形实现，
//! 本模块收拢）：已到访页签走 display 隐藏保持挂载——切页签不重挂重拉（show
//! 半边，页签内会话态存活）；未到访页签不挂载不预载（lazy 半边）。三个部件：
//! - **到访标记**：`visited: Signal<Vec<bool>>` 与页签闭集等长，初值仅首签为
//!   `true`（默认首签挂载即呈现）；到访 effect 在激活页签变化时置位——once
//!   语义，只置位不复位；
//! - **宿主门**：每个页签一个宿主，`when(visited[index])` 守门，未到访渲染
//!   空视图；一旦到访宿主永久挂载；
//! - **display 切换**：非激活宿主打成 display none（退出布局流、不收点位），
//!   节点与页内信号存活。删除 display 切换接线（改回卸载重建）即红：headless
//!   keep-mounted 探针见 tests/app_headless.rs（切回后已访页签的会话态随卸载
//!   丢失 → 红，设置页与投资页各一条）。
//!
//! 泛型参数：页签枚举（`Copy + PartialEq` 闭集）、激活判定与内容工厂皆闭包
//! （激活信号来源是局部态还是会话槽、palette 是否入场，由调用方闭包吸收）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::prelude::*;

/// 到访标记初值：与页签闭集等长的全 `false`，仅首签置 `true`（默认首签挂载
/// 即呈现，[`crate::settings`] / [`crate::investments`] 同口径）。
pub(crate) fn visited_flags(tab_count: usize) -> Signal<Vec<bool>> {
    signal({
        let mut flags = vec![false; tab_count];
        flags[0] = true;
        flags
    })
}

/// 到访标记 effect：激活页签变化时把对应下标置 `true`。`current` 闭包内部读
/// 激活信号（局部态或会话槽由调用方决定），信号读取让 effect 建立依赖。
pub(crate) fn mark_visited_on_change<T>(
    visited: Signal<Vec<bool>>,
    tabs: &'static [T],
    current: impl Fn() -> T + Copy + 'static,
) where
    T: PartialEq + Copy + 'static,
{
    effect(move || {
        let current_tab = current();
        if let Some(index) = tabs.iter().position(|tab| *tab == current_tab) {
            visited.update(|flags| flags[index] = true);
        }
    });
}

/// keep-mounted 页签宿主：未到访不挂载（不预载）；已到访的宿主随激活状态
/// display 切换（隐藏样式，退出布局流、不收点位）——「切页签不重挂重拉」
/// 的呈现本体。`tabs` 是页签闭集全序（宿主按下标对位到访标记）；`is_active`
/// 判定本签是否激活；`pane_body` 是内容工厂（settings 传视图分支 + palette，
/// investments 传 pane 装配，各自行内统一成同一元素类型）。
pub(crate) fn keep_mounted_pane<T, B, F>(
    tabs: &'static [T],
    tab: T,
    visited: Signal<Vec<bool>>,
    is_active: impl Fn(T) -> bool + Copy + 'static,
    pane_body: F,
) -> impl IntoElement
where
    T: PartialEq + Copy + 'static,
    B: IntoElement,
    F: Fn(T) -> B + Copy + 'static,
{
    let index = tabs.iter().position(|t| *t == tab).unwrap_or(0);
    when(
        move || visited.get()[index],
        move || {
            view()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .style_when(move || !is_active(tab), hide_when_inactive)
                .child(pane_body(tab))
        },
    )
    .otherwise(view)
}

/// 不活动页签宿主的隐藏样式（display none —— 旧壳「非激活 pane 不在文档流」
/// 的等效物；独立 fn 让删除映射即编译红）。
fn hide_when_inactive(
    style: nsh_core::element::StyleRefinement,
) -> nsh_core::element::StyleRefinement {
    use nsh_core::element::Styled;
    style.hidden()
}
