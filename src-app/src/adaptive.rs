//! 窄窗自适应断点单点（issue #2057 窄窗自适应横切；对表口径 ADR-0088）。
//!
//! **宽度轴唯一事实源**（ADR-0088 决策 1/2 / 旧壳 `packages/window-tier`
//! `useWindowTier` 的本壳重生）：840pt 单断点——≥ 断点为桌面档（既有布局一字
//! 不动），< 断点为移动档（窄窗自适应形态）。本模块只做「宽度信号 → 档位」的
//! 纯映射与移动档导航壳（决策 4：侧栏替换为顶栏 + 导航抽屉），不持任何按档位
//! 的渲染决策（形态断言归消费视图；`when`/响应式读取即订阅，窗口缩放实时换
//! 档，断点切换平滑——形态读闭包都读本模块的响应式查询，旧壳 media listener
//! 的本壳等价物是环境窗口尺寸信号）。
//!
//! **断点消费单点纪律**：各视图一律声明式消费 [`content_width`]（内容区宽度 =
//! 桌面档扣侧栏宽 / 移动档全宽——视图不再各自 `window().width - SIDEBAR_WIDTH`
//! 自建判定，断点口径唯一）；新旧消费面在本票一次性迁齐，`elastic_columns`
//! 等弹性轴字号不再出现第二处侧栏扣减。
//!
//! **移动档导航壳**（ADR-0088 决策 4 / 旧壳 `MobileNavShell` 的本壳重生）：
//! 顶栏（汉堡按钮 + 当前视图名）+ 导航抽屉。抽屉消费同一份导航状态
//! （[`crate::nav`]）与侧栏出厂定义（视图名/组词表/组内序/收纳清单/关闭集合
//! 同源，零改动零双写）；移动档不渲染键位提示、不附右键排序菜单（桌面专属
//! 管理动作不上抽屉，决策 6/10）；**导航即关闭**（旧壳路由 watch 收口——菜单
//! 项、「更多」链接与快捷键统一由落位驱使抽屉收起）；抽屉是弹层：开合上报
//! Overlay Suppression 注册表（ADR-0035 语义，开抽屉期间快捷键抑制照常），
//! ESC / 遮罩点击关闭（导航菜单无表单状态，误触零代价，非模态家族——与点遮
//! 拒关的模态弹窗不同档）。
//!
//! 状态不丢（断点切换平滑的成功判据）：导航状态、筛选会话态、分页页码等全归
//! 进程级信号束挂根组件实例，档位切换只改布局分支不重置任何信号——两侧形态
//! 读同一份数据。测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放
//! 行六件套。

use nsh_core::prelude::*;

use crate::nav;
use crate::overlay;

/// 窄窗断点（pt，旧壳 `WINDOW_TIER_BREAKPOINT_PX` 同值同语义）：≥ 断点桌面档，
/// < 断点移动档（ADR-0088 决策 2 全仓唯一出现点——旧壳 JS 与 CSS 同值，本壳
/// 唯一出现点在此）。
pub(crate) const NARROW_BREAKPOINT: f32 = 840.0;

/// 当前是否窄窗（移动档）。响应式：读闭包内消费即订阅窗口尺寸信号，缩放实时
/// 换档。宽 0 = 窗口面未知（headless 驱动替身在配置 picker 时重发 Environment
/// 携默认零窗——替身缺陷面，运行期 GTK 窗口恒 ≥1pt），落桌面档。
#[inline]
pub(crate) fn is_narrow() -> bool {
    let width = window().width.0;
    width > 0.0 && width < NARROW_BREAKPOINT
}

/// 内容区宽度（pt）：桌面档 = 窗口宽 − 侧栏宽；移动档 = 窗口全宽（侧栏被
/// 顶栏替换，不扣侧栏宽）。弹性列宽等「扣侧栏」算式的唯一出口——视图不自建
/// 判定，断点切换时弹性轴随档位正确重算（各视图同一读闭包，删除本单点回写
/// 去中心扣减即各窗口档位口径漂移）。
pub(crate) fn content_width() -> f32 {
    let width = window().width.0;
    if is_narrow() {
        width
    } else {
        width - crate::sidebar::SIDEBAR_WIDTH
    }
}

/// 移动档导航壳状态（进程级单例，`App` 组件体初始化——信号挂根实例；headless
/// 多场景串行复位）。抽屉开合是壳态：不持久化（旧壳 AppDrawer 局部 ref 同口
/// 径——抽屉态跨启动回归关闭）。
#[derive(Clone, Copy)]
pub(crate) struct MobileShellStore {
    drawer_open: Signal<bool>,
}

/// 进程级导航壳状态槽（锁毒化取回内值：槽位不含不变量，app.rs 槽位同形）。
static MOBILE_SHELL: std::sync::Mutex<Option<MobileShellStore>> = std::sync::Mutex::new(None);

fn shell_slot() -> std::sync::MutexGuard<'static, Option<MobileShellStore>> {
    MOBILE_SHELL.lock().unwrap_or_else(|p| p.into_inner())
}

/// 移动档导航壳状态（惰性初始化；须在引擎线程调用）。
pub(crate) fn store() -> MobileShellStore {
    let mut slot = shell_slot();
    if let Some(s) = *slot {
        return s;
    }
    let s = MobileShellStore {
        drawer_open: signal(false),
    };
    *slot = Some(s);
    s
}

/// headless 复位（[`crate::app::reset_book_assembly`] 消费）：壳态随导航/筛选
/// 同批复位。
#[doc(hidden)]
pub(crate) fn reset_for_tests() {
    *shell_slot() = None;
}

impl MobileShellStore {
    /// 抽屉是否开着（响应式）。
    pub(crate) fn drawer_open(&self) -> bool {
        self.drawer_open.get()
    }

    /// 打开抽屉（顶栏汉堡）。
    pub(crate) fn open_drawer(&self) {
        self.drawer_open.set(true);
    }

    /// 关闭抽屉（ESC / 遮罩点击 / 导航落位驱使；幂等）。
    pub(crate) fn close_drawer(&self) {
        self.drawer_open.set(false);
    }
}

/// 响应式统计行（旧壳多卡行窄窗收合的共用形态，ADR-0088 决策 11 旧壳
/// `NGrid` 桌面 cols=2/3 → 移动档 cols=1 的同构）：桌面档横向一行（归 `gap`
/// 间距），窄窗移动档纵向堆叠（间距 12pt）。子项构造器两档共用同一份——
/// 堆叠只改轴向不改内容与口径（断点切换平滑，子项状态零漂移）。
pub(crate) fn responsive_row(
    wide_gap: f32,
    items: impl Fn() -> Vec<AnyElement> + Copy + 'static,
) -> impl IntoElement {
    when(is_narrow, move || {
        view()
            .flex_col()
            .gap(px(crate::design::spacing::GROUP))
            .children(items())
    })
    .otherwise(move || view().flex_row().gap(px(wide_gap)).children(items()))
}

/// 顶栏（移动档应用栏）：汉堡按钮（开抽屉）+ 当前视图名。当前视图名随落位响
/// 应式（含组「更多」容器的落位名，`ViewName::label` 词表单点）；汉堡行面抬
/// 满触控下限（48pt——ADR-0088 决策 11 验收基线，旧壳 mobile-hamburger 同规格）。
pub(crate) fn top_bar(fg: AdaptiveColor, muted: AdaptiveColor) -> View {
    let shell = store();
    view()
        .test_id("nav-topbar")
        .flex_row()
        .items_center()
        .gap_1()
        .app_class("border-b border-border bg-background")
        .child(
            // 汉堡按钮：触控目标 ≥48pt（px_3 py_3 + 文本 18pt ≈ 48pt 行面，
            // 决策 11 基线；字阶 LEADING 档）；test_id 落在文本上（headless
            // id: 锚点命中内层可按压件，先例期间芯片同形）。
            view()
                .test_id("nav-topbar-open")
                .flex_row()
                .items_center()
                .justify_center()
                .px_3()
                .py_3()
                .app_class("rounded-(--radius)")
                .on_press(move |_: &PressEvent| shell.open_drawer())
                .child(
                    text("☰")
                        .text_lg()
                        .text_color(fg)
                        .test_id("nav-topbar-open"),
                ),
        )
        .child(
            text(move || nav::store().current.get().label().to_owned())
                .text_sm()
                .text_color(muted)
                .test_id("nav-topbar-title"),
        )
}

/// 导航抽屉（移动档导航壳的抽屉态呈现）：左栏面板（侧栏内容同源：品牌行 +
/// 固定项 + 三组，无键位提示、无右键排序菜单）+ 遮罩。开合上报抑制注册表；
/// ESC / 遮罩点击关闭；**导航即关闭**——落位（视图/容器页签变化）驱使收起，
/// 旧壳 drawerShow 随 route watch 收口的同语义。
///
/// 弹层挂载：常驻移动档分支（`App` 窄窗时挂载本组件），呈现以抽屉开态受控。
pub(crate) fn nav_drawer() -> impl IntoElement {
    let shell = store();
    // 开/关上报（抑制注册表单一通道，modal.rs 同形：effect 追踪受控开合，全
    // 路径覆盖，组件卸载 Drop 自动撤销）。
    let token = overlay::OverlayToken::new();
    effect(move || {
        let shown = shell.drawer_open();
        token.report(shown);
    });
    // 导航即关闭：落位与容器页签变化（含快捷键在抽屉开着时的换位）统一收
    // 口（旧壳 watch route fullPath 同语义）；同值幂等（信号等值不通知）。
    effect(move || {
        let nav = nav::store();
        let _landed = nav.current.get();
        let _tab = nav.container_tab.get();
        shell.close_drawer();
    });
    when(
        move || shell.drawer_open(),
        move || {
            overlay()
                .inset_0()
                .flex_row()
                .app_class("bg-black/30")
                .on_cancel(move || shell.close_drawer())
                .child(
                    // 抽屉面板（前层，左）：宽与桌面侧栏同宽（同一宽度常量——
                    // 面板即侧栏的另一壳层渲染，旧壳 ADR-0088 决策 4 原话）。
                    // 内容 = 侧栏同源导航面（ [`crate::sidebar::drawer_panel`]：
                    // 无键位提示、无右键排序菜单，决策 6/10）。
                    view()
                        .h_full()
                        .w(px(crate::sidebar::SIDEBAR_WIDTH))
                        .flex_col()
                        .app_class("bg-background border-r border-border")
                        .child(crate::sidebar::drawer_panel()),
                )
                .child(
                    // 遮罩（后层余幅）：点击即关（旧壳 AppDrawer mask-closable——
                    // 导航抽屉非模态家族，遮罩点击误触零代价）。
                    view()
                        .flex_1()
                        .h_full()
                        .on_press(move |_: &PressEvent| shell.close_drawer()),
                )
        },
    )
}
