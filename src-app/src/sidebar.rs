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
//! 品牌行（issue #2137）：应用名 + 金额隐私眼睛按钮——旧壳 `App.vue`
//! brandRow（侧栏标题行，#566）的 NSH 对应形态，侧栏与导航抽屉同一渲染面
//! （[`brand_row`] 单点，旧壳桌面档与移动档壳层复用同一渲染函数同构）；
//! 点击切换设备轻量设置项 `amount_privacy_enabled`（写入口在
//! [`crate::transactions`] 单点），掩码消费面零改动（装载时机读取语义已在
//! 各视图落地：切换后已挂载视图不即时翻转，落位重挂/失效重读即按新态装配）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::button;
use nsh_core::element::desktop::MenuItem;
use nsh_core::platform::Os;
use nsh_core::prelude::*;

use crate::context_menu;
use crate::nav::{self, NavStore};
use crate::nav_model::{self, GroupId, NavRequest, SortAction, ViewName};
use crate::transactions::{amount_privacy_enabled, set_amount_privacy_enabled};

/// 侧栏宽度（桌面档固定宽；折叠与自适应不在本票——旧壳折叠态随窄窗横切票
/// 另裁）。
pub(crate) const SIDEBAR_WIDTH: f32 = 176.0;

/// 侧栏（桌面档）：消费 [`NavStore`] 渲染分组导航。挂载即消费导航状态（
/// `App` 就绪分支内挂载）。
#[component]
pub fn SidebarView() -> impl IntoElement {
    let nav = nav::store();
    // 主题令牌（双态自适应；裸 text 默认前景黑——暗色桌面不可见，#2017 先例）。
    let fg = crate::theme::color("foreground");
    let muted = crate::theme::color("muted-foreground");
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
    let brand = brand_row();
    let fixed_item = |name: ViewName| nav_item(name, hint_of, fg, muted, NavSurface::Desktop);

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
                    nav_item(item.get(), hint_of, fg, muted, NavSurface::Desktop)
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
        // 弹性占位 + 底部账本切换区（web sider 骨架同构：菜单区伸展、入口
        // 沉底；#2129）。
        .child(view().flex_1())
        .child(crate::sidebar_books::book_switch_area())
}

/// 品牌行（侧栏与导航抽屉同一渲染面，入口唯一——旧壳 `App.vue` brandRow
/// 桌面档与移动档壳层复用同构）：应用名 + 金额隐私眼睛按钮（issue #2137）。
///
/// 眼睛按钮点击切换设备轻量设置项 `amount_privacy_enabled`（写入口在
/// [`crate::transactions::set_amount_privacy_enabled`] 单点，同键同值形态）；
/// 睁/闭两态字形与无障碍标签反映当前状态（文案 = 旧壳 common.amountPrivacy*
/// zh-CN 词面：关→「隐藏金额」、开→「显示金额」，zh-CN 硬编码现状）；按钮
/// selected 态携带开关语义（旧壳 aria-pressed 的原生对应物，toggle button
/// 模式）；无禁用态（旧壳同口径，侧栏折叠时按钮不可见在本壳无折叠态）。
///
/// 掩码消费面零改动：各视图仍装载时机读取，切换后已挂载视图不即时翻转，
/// 落位重挂/失效重读即按新态装配。镜像信号（挂本组件实例）只承担按钮自身
/// 的响应式呈现——存储读无响应式依赖，不镜像则切换后字形不随动。
fn brand_row() -> View {
    let fg = crate::theme::color("foreground");
    let muted = crate::theme::color("muted-foreground");
    // 开关镜像信号：初值 = 存储读取（挂本组件实例）；点击即写存储 + 置镜像。
    let privacy = signal(amount_privacy_enabled());
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .px_3()
        .py_2()
        .child(
            // 应用名（旧壳 common.app.name 同词表）。
            text("开源记账").text_sm().text_color(fg),
        )
        .child(
            button()
                .selected(move || privacy.get())
                .accessibility_label(move || {
                    // 两态无障碍标签（旧壳 common.amountPrivacy* zh-CN 词面）。
                    let label: SharedString = if privacy.get() {
                        "显示金额".into()
                    } else {
                        "隐藏金额".into()
                    };
                    Some(label)
                })
                .app_class("rounded-full px-1.5 py-0.5 hover:bg-accent")
                .child(
                    text(move || {
                        // 两态字形（旧壳 EyeOutline / EyeOffOutline 图标的本壳
                        // 字形对应物：睁眼 = 金额可见；掩码字面 = 金额已隐藏，
                        // 与全应用隐私掩码同词）。
                        if privacy.get() {
                            "••••".to_owned()
                        } else {
                            "👁".to_owned()
                        }
                    })
                    .text_xs()
                    .text_color(muted)
                    .test_id("brand-privacy"),
                )
                .on_press(move |_: &()| {
                    let enabled = !privacy.get_untracked();
                    set_amount_privacy_enabled(enabled);
                    privacy.set(enabled);
                }),
        )
        .app_class("border-b border-border")
}

/// 侧栏呈现面（桌面侧栏 / 导航抽屉同构消费导航状态，形态差异收口）。
#[derive(Clone, Copy, PartialEq)]
enum NavSurface {
    /// 桌面侧栏：键位提示 + 右键排序菜单（决策 10 桌面专属管理动作）。
    Desktop,
    /// 导航抽屉（移动档）：无键位提示、无右键菜单（ADR-0088 决策 6/10 号响
    /// ——触控轴快捷键退役渲染、桌面专属管理动作不上抽屉）。
    Drawer,
}

/// 导航项（固定项与组内成员共用）：视图名 + 键位提示小字，当前视图高亮，
/// 点击导航（[`nav::open`] 裸访问）；成员项的右键排序菜单在
/// [`attach_sort_menu`] 按「侧栏在册」挂载（固定项不可排、无菜单）。
fn nav_item(
    name: ViewName,
    hint_of: impl Fn(ViewName) -> Option<String> + Copy + 'static,
    fg: AdaptiveColor,
    muted: AdaptiveColor,
    surface: NavSurface,
) -> View {
    let item = view()
        .test_id(format!("nav-{}", name.as_str()).as_str())
        .flex_row()
        .items_center()
        .justify_between()
        .px_3()
        .py_1()
        .mx_1()
        .app_class("rounded-(--radius)")
        .on_press(move |_: &()| nav::open(NavRequest::bare(name)))
        .style_when(
            move || nav::store().current.get() == name,
            move |s| s.bg(accent_background()),
        )
        .child(
            text(name.label())
                .text_sm()
                .text_color(fg)
                .number_of_lines(1),
        )
        .child(
            // 键位提示（桌面侧栏带；抽屉不渲染——决策 6 移动档快捷键退役）。
            // 辅助小字下限 = 字阶 CAPTION 12（规约阅读文字 ≥ 12pt，10pt 裸值退役）。
            text(move || {
                if surface == NavSurface::Desktop {
                    hint_of(name).unwrap_or_default()
                } else {
                    String::new()
                }
            })
            .text_xs()
            .text_color(muted),
        );
    if surface == NavSurface::Desktop {
        attach_sort_menu(name, item)
    } else {
        item
    }
}

/// 导航抽屉面板（移动档）：品牌行 + 固定项 + 三组（组标题行含「更多」链接）。
/// 与桌面侧栏同一份导航状态与出厂定义（由 [`crate::adaptive::nav_drawer`]
/// 拼装），差异仅在呈现面：无键位提示、无右键排序菜单（[NavSurface::Drawer]
/// ——决策 6/10）。导航即关闭由抽屉层收口（落位驱使收起，本面板零变更）。
pub(crate) fn drawer_panel() -> View {
    let nav = nav::store();
    let fg = crate::theme::color("foreground");
    let muted = crate::theme::color("muted-foreground");
    let no_hint = |_: ViewName| -> Option<String> { None };
    let brand = brand_row();
    let fixed_item = |name: ViewName| nav_item(name, no_hint, fg, muted, NavSurface::Drawer);
    let group_rows = nav_model::GROUPS
        .iter()
        .copied()
        .map(|gid| {
            let header = group_header(nav, gid, fg, muted);
            let members = view().flex_col().child(each(
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
                    nav_item(item.get(), no_hint, fg, muted, NavSurface::Drawer)
                },
            ));
            view().flex_col().gap_1().child(header).child(members)
        })
        .collect::<Vec<_>>();
    view()
        .flex_col()
        .child(brand)
        .child(fixed_item(ViewName::Dashboard).mt_2())
        .children(group_rows)
        .child(fixed_item(ViewName::Ai).mt_2())
        .child(fixed_item(ViewName::Settings))
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
            // 组名 overline：辅助小字下限 = 字阶 CAPTION 12。
            text(gid.label())
                .text_xs()
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
                        .app_class("rounded-(--radius)")
                        .on_press(move |_: &()| nav::open(NavRequest::bare(gid.more_view())))
                        .child(text("更多").text_xs().text_color(fg))
                        .child(text("›").text_xs().text_color(muted))
                },
            )
            .otherwise(view),
        )
}

/// 当前视图高亮底色（弱化面，accent 令牌经 [`crate::theme::color`] 单点）。
fn accent_background() -> AdaptiveColor {
    crate::theme::color("accent")
}
