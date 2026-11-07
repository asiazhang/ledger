//! 壳内视图层（issue #2032 壳导航骨架）：未迁移视图的空态占位与组「更多」
//! 容器页签壳。
//!
//! **占位视图**是下钻接缝的示范消费面：落位时 [`nav::take_intent`] 读一次，
//! 携带定位参数时呈现「已记录定位目标」，真实视图迁移时以同形消费替换本组件
//! （旧壳 useFocusParam 的四落点形态——开弹窗 / 滚动高亮 / 分页锚点——归各
//! 迁移票）。消费时机 = 落位（effect 键当前视图/激活页签，落位与意图同轮
//! 落地）；读一次语义 = 消费即封闸，页签切换/视图间往复不重放，重启即失。
//! 文案仅 zh-CN。
//!
//! **「更多」容器**是收纳语义的呈现壳（旧壳 GroupMoreView 的重生）：页签 =
//! 该组收纳清单剔关闭（响应式）；页签序 = 清单序；默认首签、页签态会话内
//! 有效不跨启动持久化（导航状态 [`crate::nav`] 的会话槽）；页签点击经导航
//! 接缝（[`nav::open`] 携容器页签意图，守卫裁决归一）；页签右键「移回侧栏」
//! （组满置灰 + 提示，上限可见可学习）；成员全关闭 = 零页签（收纳清单保留，
//! 侧栏「更多」链接随之消失）。容器零业务逻辑：被收视图整体装载——已迁移的
//! 交易列表挂真实组件，其余挂占位。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{tab, tabs};
use nsh_core::element::desktop::MenuItem;
use nsh_core::prelude::*;

use crate::app::transactions_view;
use crate::context_menu;
use crate::items;
use crate::nav::{self, NavIntent};
use crate::nav_model::{self, GroupId, NavRequest, ViewName};

/// 视图令牌便捷面（前景/弱化，双态自适应；令牌缺失回退亮灰/中灰可读兜底，
/// app.rs 先例同形）。
#[derive(Clone, Copy)]
pub(crate) struct ViewPalette {
    /// 前景。
    pub fg: AdaptiveColor,
    /// 弱化前景。
    pub muted: AdaptiveColor,
}

/// 取视图令牌。
pub(crate) fn view_palette() -> ViewPalette {
    let tokens = nsh_core::element::tokens();
    ViewPalette {
        fg: tokens
            .color("foreground")
            .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into()),
        muted: tokens
            .color("muted-foreground")
            .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into()),
    }
}

/// 占位呈现体（共享壳）：屏名 + 「迁移中」反馈 + （有下钻意图时）定位目标
/// 回显。`title` 与意图消费时机归调用方（独立占位 = 挂载消费；容器激活成员
/// = effect 键激活切换消费）。
fn placeholder_shell(
    title: impl Fn() -> String + Copy + 'static,
    focused: Signal<Option<NavIntent>>,
    palette: ViewPalette,
) -> View {
    view()
        .flex_1()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .app_class("bg-background")
        .child(
            text(title)
                .text_size(px(20.0))
                .text_color(palette.fg)
                .test_id("view-title"),
        )
        .child(
            text("迁移中，功能将随迁移系列票就位")
                .text_size(px(12.0))
                .text_color(palette.muted)
                .test_id("placeholder-note"),
        )
        .child(
            // 下钻意图回显（有定位参数时）：真实视图迁移后呈现高亮/弹窗，
            // 占位期以回显证明意图投递与读一次消费。
            when(
                move || focused.get().is_some(),
                move || {
                    text(format!(
                        "已记录定位目标 {}",
                        focused.get().and_then(|i| i.focus).unwrap_or_default()
                    ))
                    .text_size(px(12.0))
                    .text_color(palette.muted)
                    .test_id("focus-target")
                },
            )
            .otherwise(view),
        )
}

/// 反应式占位壳（视图宿主的未迁移分支落点）：标题随当前视图响应式；意图
/// 消费时机归调用方（effect 键 current，落位与意图同轮落地）。
pub fn placeholder_shell_current(focused: Signal<Option<NavIntent>>) -> impl IntoElement {
    let palette = view_palette();
    placeholder_shell(
        move || nav::store().current.get().label().to_owned(),
        focused,
        palette,
    )
}

/// 组「更多」容器页签壳（`group` 由视图宿主按落位视图传入；三容器各一挂载
/// 分支，组间切换即重挂、页签态按会话槽恢复）。
pub fn group_more_view(group: GroupId) -> impl IntoElement {
    let nav = nav::store();
    let palette = view_palette();

    // 可见页签 = 收纳清单剔关闭（响应式；关闭只筛页签入口不改清单——重开即
    // 回原位）；空清单 = 零页签（含出厂无成员与成员全关闭两态）。
    let visible = computed(move || {
        nav.containment
            .get()
            .get(group)
            .iter()
            .copied()
            .filter(|v| !nav.closed.get().contains(v))
            .collect::<Vec<_>>()
    });
    // 页签会话槽（导航落位意图 / 页签点击写；合法可见才生效，否则默认首签）。
    let active = computed(move || {
        let vis = visible.get();
        let wanted = nav.container_tab.get();
        vis.iter()
            .find(|v| Some(**v) == wanted)
            .or_else(|| vis.first())
            .copied()
    });

    // 激活成员的下钻意图消费（落位与意图同轮落地；effect 键激活切换，读一次
    // 语义由 [nav::take_intent] 封闸承担——页签往复不重放）。激活成员是已迁
    // 移真实视图时本占位壳不抢消费（真实视图挂载自带落位消费，#2037 下钻
    // 接缝——抢了即吞掉落位意图；#2044 起物品视图同位）。
    let focused = signal(None::<NavIntent>);
    effect(move || {
        let current = active.get();
        if current != Some(ViewName::Transactions) && current != Some(ViewName::Items) {
            focused.set(nav::take_intent());
        }
    });

    // 页签行（nsh-base Tabs 行为件：roving 焦点 + 无障碍页签列表；指示器取
    // 主题弱化面）。页签列表响应式键控（`each`，键 = 视图名）：收纳清单/
    // 关闭集合变化即增删页签行。
    let tabs_row = view()
        .px_4()
        .py_2()
        .app_class("border-b border-border")
        .child(
            tabs()
                .indicator_class("bg-accent")
                .child(view().flex_row().gap_2().child(each(
                    move || visible.get(),
                    |v| *v,
                    move |item: ReadSignal<ViewName>, _index: ReadSignal<usize>| {
                        tab_shell(group, item.get(), active, palette.fg)
                    },
                ))),
        );

    // 被收视图整体装载（容器零业务逻辑）：已迁移的真实视图（交易列表 /
    // 物品 #2044）挂真实组件，其余挂激活页签的占位（标题随激活成员响应式，
    // 意图消费归上方 effect）。
    let content = when(
        move || active.get() == Some(ViewName::Transactions),
        transactions_view,
    )
    .otherwise(move || {
        when(
            move || active.get() == Some(ViewName::Items),
            items::items_view,
        )
        .otherwise(move || {
            placeholder_shell(
                move || {
                    active
                        .get()
                        .map(|v| v.label().to_owned())
                        .unwrap_or_default()
                },
                focused,
                palette,
            )
        })
    });

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            // 屏名（内容区标题，旧壳 page-title 同位；「组 · 更多」同旧壳
            // nav.bookkeeping-more 词表形态）。
            view().px_4().py_2().child(
                text(format!("{} · 更多", group.label()))
                    .text_size(px(16.0))
                    .text_color(palette.fg)
                    .test_id("view-title"),
            ),
        )
        .child(tabs_row)
        .child(content)
}

/// 页签载体（右键「移回侧栏」挂在包裹视图上——右键事件就近上浮到最近监听
/// 祖先，页签本体保持 nsh-base Tab 行为件：按压、roving 焦点、无障碍角色）。
fn tab_shell(
    group: GroupId,
    name: ViewName,
    active: Memo<Option<ViewName>>,
    fg: AdaptiveColor,
) -> View {
    view()
        .on_context_menu(move |e: &ContextMenuEvent| {
            open_back_to_sidebar_menu(group, name, e.page_position);
        })
        .child(
            tab()
                .selected(move || active.get() == Some(name))
                .accessibility_label(name.label())
                .app_class("px-2 py-1 rounded-md selected:bg-accent")
                // test_id 落在页签内文本上（headless id: 锚点命中文本、物理命中
                // 到内层页签可按压件，先例期间芯片同形）；右键事件就近上浮到本
                // 包裹视图。
                .child(
                    text(name.label())
                        .text_size(px(13.0))
                        .text_color(fg)
                        .test_id(format!("more-tab-{}", name.as_str()).as_str()),
                )
                .on_press(move |_: &()| {
                    nav::open(NavRequest::container(group.more_view(), Some(name)))
                }),
        )
}

/// 页签右键「移回侧栏」：单菜单项（目标随右键页签固定），组满置灰并附提示
/// 项（上限可见、可学习，旧壳 buildTabContextMenuOptions 两行标签的原生菜单
/// 对应形态）；点选即从清单删除并落本组主项末位
/// （[`nav::apply_move_back_to_sidebar`] 写路径，组满拒写在写路径同判定兜底）。
fn open_back_to_sidebar_menu(group: GroupId, name: ViewName, at: Point<Points>) {
    let full = nav_model::is_group_full(nav::store().orders.get_untracked().get(group));
    let mut items = vec![
        MenuItem::new("移回侧栏")
            .disabled(full)
            .on_select(move || nav::apply_move_back_to_sidebar(name)),
    ];
    if full {
        items.push(MenuItem::new("本组主项已满 3 项，先移出一个主项腾位").disabled(true));
    }
    context_menu::open_context_menu(items, at);
}
