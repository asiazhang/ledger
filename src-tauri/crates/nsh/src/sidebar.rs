//! 侧栏（issue #2032 壳导航骨架）：旧壳 `App.vue` 侧栏桌面档的重生——品牌行、
//! 固定项（概览/AI/设置）、三组（记账/资产/洞察，组与组序固定）、组标题行
//! 「更多」链接（收纳清单剔关闭后非空才渲染）、组内成员项（键位提示小字 +
//! 右键排序菜单）、菜单点击导航。
//!
//! 语义对齐旧壳：关闭只筛入口不改数据（重开即回原位）；收纳成员不入侧栏
//! （入组「更多」容器页签）；右键排序仅指针轴、仅侧栏在册成员（含移回种子）；
//! 键位提示随键位带推导（关闭功能退出、组内重排带内随动）；窄窗自适应不在
//! 本票（迁移系列收尾横切票）。
//!
//! 弹层纪律：右键菜单经 [`crate::context_menu`] 封装（原生菜单 + 存续期间
//! 抑制上报），零新抑制机制；文案仅 zh-CN（旧壳 common.sidebar* 同词表）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::element::desktop::MenuItem;
use nsh_core::platform::Os;
use nsh_core::prelude::*;

use crate::context_menu;
use crate::nav::{self, NavStore};
use crate::nav_model::{self, GroupId, NavRequest, SortAction, ViewName};

/// 侧栏宽度（桌面档固定宽；折叠与自适应不在本票——旧壳折叠态随窄窗横切票
/// 另裁）。
pub(crate) const SIDEBAR_WIDTH: f32 = 176.0;

/// 侧栏（桌面档）：消费 [`NavStore`] 渲染分组导航。挂载即消费导航状态（
/// `App` 就绪分支内挂载）。
#[component]
pub fn SidebarView() -> impl IntoElement {
    let nav = nav::store();
    // 主题令牌（双态自适应；裸 text 默认前景黑——暗色桌面不可见，#2017 先例）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());
    // 键位提示符号（⌘ / ⌃）：平台判定一次（bin 当前仅 Linux；macOS 随平台票）。
    let apple = os() == Os::MacOs;
    // 键位带现推导（组内序/关闭集合响应式随动：重排即带内重排、关闭即退出）。
    let shortcuts =
        computed(move || nav_model::derive_view_shortcuts(&nav.orders.get(), &nav.closed.get()));
    let hint_of = move |name: ViewName| -> Option<String> {
        shortcuts
            .get()
            .iter()
            .find(|sc| sc.view == name)
            .map(|sc| nav::shortcut_hint_text(sc.key, apple))
    };

    // 固定项（概览首位 / AI 倒数第二 / 设置末位；不可关、不可排、无右键菜单）。
    let brand = view()
        .flex_row()
        .items_center()
        .px_3()
        .py_2()
        .child(
            text(move || {
                // 应用名（旧壳 common.app.name 同词表）。
                "开源记账".to_owned()
            })
            .text_size(px(14.0))
            .text_color(fg),
        )
        .app_class("border-b border-border");
    let fixed_item = |name: ViewName| nav_item(name, hint_of, fg, muted);

    // 三组（组序固定）：组标题行（组名 + 「更多」链接）+ 组内成员（关闭只筛
    // 入口不改数据；收纳成员不入侧栏）。成员列表用响应式键控列表（`each`，
    // 键 = 视图名）：组内序/关闭集合变化即重排成员行，行内交互件随键存亡。
    let group_rows = nav_model::GROUPS
        .iter()
        .copied()
        .map(|gid| {
            let header = group_header(nav, gid, fg, muted);
            let members = view().flex_col().child(each(
                // 响应式成员源：当前组内序剔关闭（旧壳 buildMenuOptions 的
                // filter 同口径——关闭只筛入口不改数据）。
                move || {
                    nav.orders
                        .get()
                        .get(gid)
                        .iter()
                        .copied()
                        .filter(|v| !nav.closed.get().contains(v))
                        .collect::<Vec<_>>()
                },
                |v| *v,
                move |item: ReadSignal<ViewName>, _index: ReadSignal<usize>| {
                    nav_item(item.get(), hint_of, fg, muted)
                },
            ));
            view().flex_col().gap_1().child(header).child(members)
        })
        .collect::<Vec<_>>();

    view()
        .w(px(SIDEBAR_WIDTH))
        .flex_col()
        .app_class("bg-background border-r border-border")
        .child(brand)
        .child(fixed_item(ViewName::Dashboard).mt_2())
        .children(group_rows)
        .child(fixed_item(ViewName::Ai).mt_2())
        .child(fixed_item(ViewName::Settings))
}

/// 导航项（固定项与组内成员共用）：视图名 + 键位提示小字，当前视图高亮，
/// 点击导航（[`nav::open`] 裸访问）；成员项的右键排序菜单在
/// [`attach_sort_menu`] 按「侧栏在册」挂载（固定项不可排、无菜单）。
fn nav_item(
    name: ViewName,
    hint_of: impl Fn(ViewName) -> Option<String> + Copy + 'static,
    fg: AdaptiveColor,
    muted: AdaptiveColor,
) -> View {
    let item = view()
        .test_id(format!("nav-{}", name.as_str()).as_str())
        .flex_row()
        .items_center()
        .justify_between()
        .px_3()
        .py_1()
        .mx_1()
        .rounded_md()
        .on_press(move |_: &()| nav::open(NavRequest::bare(name)))
        .style_when(
            move || nav::store().current.get() == name,
            move |s| s.bg(accent_background()),
        )
        .child(
            text(name.label())
                .text_size(px(13.0))
                .text_color(fg)
                .number_of_lines(1),
        )
        .child(
            text(move || hint_of(name).unwrap_or_default())
                .text_size(px(10.0))
                .text_color(muted),
        );
    attach_sort_menu(name, item)
}

/// 右键排序菜单挂载（仅侧栏在册成员——含移回的种子成员；固定项与收纳成员
/// 不附事件，旧壳 isSidebarMember 判定同语义）：手动定位弹出
/// [`crate::context_menu`]（原生菜单 + 抑制上报自含）。
fn attach_sort_menu(name: ViewName, item: View) -> View {
    item.on_context_menu(move |e: &ContextMenuEvent| {
        let store = nav::store();
        let is_member = store.orders.get_untracked().contains_any(name)
            && nav_model::group_of_view(name).is_some();
        if !is_member {
            return;
        }
        let Some(gid) = nav_model::group_of_view(name) else {
            return;
        };
        let order = store.orders.get_untracked().get(gid).to_vec();
        let index = order.iter().position(|v| *v == name);
        let at_top = index.is_none_or(|i| i == 0);
        let at_bottom = index.is_some_and(|i| i + 1 == order.len());
        context_menu::open_context_menu(
            vec![
                sort_item("上移一位", SortAction::Up, name, at_top),
                sort_item("下移一位", SortAction::Down, name, at_bottom),
                sort_item("移到顶部", SortAction::Top, name, at_top),
                sort_item("移到底部", SortAction::Bottom, name, at_bottom),
                MenuItem::separator(),
                // 移入更多（移入自由恒可用；主项退出组内序 + 入清单尾）。
                MenuItem::new("移入更多").on_select(move || {
                    nav::apply_move_into_more(name);
                }),
                MenuItem::separator(),
                MenuItem::new("恢复默认排序").on_select(nav::reset_sidebar_order),
            ],
            e.page_position,
        );
    })
}

/// 排序菜单项（边界置灰 = 首位上移/移顶、末位下移/移底，旧壳
/// buildSidebarSortMenuOptions 同判定）。
fn sort_item(label: &str, action: SortAction, name: ViewName, disabled: bool) -> MenuItem {
    MenuItem::new(label)
        .disabled(disabled)
        .on_select(move || nav::apply_sort(name, action))
}

/// 组标题行：组名 + 「更多」链接（仅收纳清单剔关闭后非空才渲染；点击落组
/// 「更多」容器，旧壳 group-more-link 同语义）。
fn group_header(nav: NavStore, gid: GroupId, fg: AdaptiveColor, muted: AdaptiveColor) -> View {
    let has_more = computed(move || {
        nav.containment
            .get()
            .get(gid)
            .iter()
            .any(|v| !nav.closed.get().contains(v))
    });
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .px_3()
        .pt_2()
        .child(
            text(gid.label())
                .text_size(px(11.0))
                .text_color(muted)
                .app_class("uppercase"),
        )
        .child(
            // 「更多」链接（收纳清单非空才渲染；链接点击 = 容器裸访问，容器
            // 页签回默认首签）。
            when(
                move || has_more.get(),
                move || {
                    view()
                        .test_id(format!("nav-more-{}", gid.as_str()).as_str())
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .px_1()
                        .rounded_md()
                        .on_press(move |_: &()| nav::open(NavRequest::bare(gid.more_view())))
                        .child(text("更多").text_size(px(11.0)).text_color(fg))
                        .child(text("›").text_size(px(10.0)).text_color(muted))
                },
            )
            .otherwise(view),
        )
}

/// 当前视图高亮底色（弱化面；令牌缺失回退中灰可读兜底）。
fn accent_background() -> AdaptiveColor {
    nsh_core::element::tokens()
        .color("accent")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.5, 0.35).into())
}
