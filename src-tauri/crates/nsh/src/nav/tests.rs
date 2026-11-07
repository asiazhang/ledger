//! 导航接缝探针测试（lib 单测；headless 引擎内驱动下钻接缝——引擎线程契约
//! 下，[`crate::nav::open`] 的 focus 意图投递没有真实 UI 生产面（来源列跳转
//! 随交易列表筛选票接线），探针组件在引擎线程内经**真实生产路径**
//! （[`crate::nav_model::resolve_source_jump`] → [`crate::nav::open`]）投递，
//! 消费面挂**真实占位组件**（挂载读一次 + 回显），断言用户可见文本。
//!
//! 「删除即红」负向条目：删 [`crate::nav::open`] 内的意图落位（`intent` 写）
//! → 探针红（无回显）；删占位组件的 [`crate::nav::take_intent`] 消费 → 探针
//! 红（回显不出现）；删容器页签落位 → 页签切换探针红。

use std::sync::Mutex;

use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Environment, Idiom, Os, Root};
use nsh_testing::Headless;

use crate::nav::{self, NavStore};
use crate::nav_model::{self, GroupId, NavRequest, SourceKind, ViewName};
use crate::views;

const WAIT: std::time::Duration = std::time::Duration::from_secs(5);

static SERIAL: Mutex<()> = Mutex::new(());

/// 探针根组件：引擎线程内走真实导航生产路径，渲染落位状态与真实占位组件。
/// 「换到实物资产页签」按钮产生容器页签意图（引擎线程真实点击路径的等价
/// 形态，用于验证读一次语义在页签切换重挂后不重放）。
#[component]
fn SeamProbeApp() -> impl IntoElement {
    let nav = nav::store();
    // 真实生产路径：来源跳转目标计算（保单，收纳态）→ 导航接缝。
    let facts = nav_model::NavFacts::factory();
    nav::open(nav_model::resolve_source_jump(
        SourceKind::Policy,
        "p-9",
        |v| facts.is_contained(v),
        |v| facts.is_closed(v),
    ));
    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            // 落位状态呈现（容器页签意图经守卫落位的用户可见凭据）。
            text(move || current_text(nav))
                .text_size(px(14.0))
                .test_id("probe-current"),
        )
        .child(
            // 资产组容器（真实容器壳）：保单页签 + 实物资产页签（出厂种子）。
            views::group_more_view(GroupId::Assets),
        )
        .child(
            view()
                .test_id("probe-switch")
                .on_press(move |_: &()| {
                    // 容器页签意图（裸容器落位到实物资产页签——无下钻意图）。
                    nav::open(NavRequest::container(
                        GroupId::Assets.more_view(),
                        Some(ViewName::PhysicalAssets),
                    ));
                })
                .child(text("换到实物资产页签")),
        )
        .child(
            view()
                .test_id("probe-back")
                .on_press(move |_: &()| {
                    // 切回保单页签（重挂保单占位——读一次语义下不得重放意图）。
                    nav::open(NavRequest::container(
                        GroupId::Assets.more_view(),
                        Some(ViewName::Policies),
                    ));
                })
                .child(text("切回保单页签")),
        )
}

/// 当前落位与页签会话槽的可读文本（断言容器页签落位的凭据）。
fn current_text(nav: NavStore) -> String {
    format!(
        "落位:{} 页签:{}",
        nav.current.get().label(),
        nav.container_tab
            .get()
            .map(|v| v.label())
            .unwrap_or("默认首签")
    )
}

fn probe_config() -> EngineConfig {
    EngineConfig {
        viewport: nsh_engine::Size::new(nsh_engine::Points(800.0), nsh_engine::Points(600.0)),
        environment: Environment {
            os: Os::Linux,
            idiom: Idiom::Desktop,
            ..Environment::default()
        },
        ui_sets: vec![nsh_base::registry()],
        ..EngineConfig::new(Root::of::<SeamProbeApp>())
    }
}

/// 下钻接缝全链（生产路径投递 → 守卫落位容器页签 → 意图投递 → 真实占位
/// 挂载消费回显 → 页签切换重挂不重放）。删除任一接线即红（见模块注释）。
#[test]
fn seam_delivers_focus_once_and_container_tab_intents_land() {
    let _guard = SERIAL.lock().unwrap();
    crate::app::reset_book_assembly();
    let mut app = Headless::start(probe_config()).unwrap();

    // 落位资产组容器、页签意图 = 保单（收纳分流守卫的落位凭据）。
    app.wait_for("id:probe-current").unwrap();
    app.wait_for("text:落位:资产 · 更多 页签:保单").unwrap();
    // 意图投递：真实占位组件挂载消费，回显定位目标（用户可见凭据）。
    app.wait_for("id:focus-target").unwrap();
    app.wait_for("text:已记录定位目标 p-9").unwrap();

    // 页签切换（无意图落位）：实物资产占位重挂——不重放上一意图。
    app.press("id:probe-switch").unwrap();
    app.wait_for("text:落位:资产 · 更多 页签:实物资产").unwrap();
    app.wait_for("text:实物资产").unwrap();
    app.wait_for_gone("id:focus-target", WAIT).unwrap();

    // 切回保单页签（重挂占位）：读一次语义——意图已耗，不重放。
    app.press("id:probe-back").unwrap();
    app.wait_for("text:落位:资产 · 更多 页签:保单").unwrap();
    app.settle();
    assert!(
        !app.shows("已记录定位目标"),
        "读一次语义：意图消费后页签切换重挂不得重放：{:?}",
        app.screen().texts()
    );
}

/// 接缝守卫裁决探针（引擎线程内经 [`crate::nav::open`] 走守卫，落位文本
/// 逐段呈现）：关闭态裸访问回出厂落点、引用（focus 在场）放行自有路由、
/// 容器页签指向已关闭功能改道自有路由且引用不落空。「删除即红」负向条目：
/// 拆 [`crate::nav::open`] 内的 `resolve_landing` 守卫裁决 → 本测红（关闭态
/// 裸访问落进功能视图、改道不发生）。
#[component]
fn GuardProbeApp() -> impl IntoElement {
    let nav = nav::store();
    // 种子关闭集合：保单、商户（存储读路径经 nav::store 初始化）。
    let land_bare = signal(String::new());
    let land_ref = signal(String::new());
    let land_tab_open = signal(String::new());
    let land_tab_closed = signal(String::new());
    // ① 关闭态裸访问：回出厂落点（骨架期 = 交易列表）。
    nav::open(NavRequest::bare(ViewName::Policies));
    land_bare.set(nav.current.get().label().to_owned());
    // ② 关闭态引用（focus 在场）：放行自有路由。
    nav::open(NavRequest::focus(ViewName::Policies, "p-1"));
    land_ref.set(nav.current.get().label().to_owned());
    // ③ 容器页签指向开放功能：落容器 + 页签意图。
    nav::open(NavRequest::container(
        GroupId::Bookkeeping.more_view(),
        Some(ViewName::Scheduled),
    ));
    land_tab_open.set(format!(
        "{}·{:?}",
        nav.current.get().label(),
        nav.container_tab.get().map(|v| v.as_str())
    ));
    // ④ 容器页签指向已关闭功能（商户）带 focus：改道自有路由且放行。
    nav::open(NavRequest::container_tab_focus(
        GroupId::Bookkeeping.more_view(),
        ViewName::Merchants,
        "m-1",
    ));
    land_tab_closed.set(nav.current.get().label().to_owned());
    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(text(move || land_bare.get()).test_id("land-bare"))
        .child(text(move || land_ref.get()).test_id("land-ref"))
        .child(text(move || land_tab_open.get()).test_id("land-tab-open"))
        .child(text(move || land_tab_closed.get()).test_id("land-tab-closed"))
}

#[test]
fn seam_guard_decides_closed_bare_reference_and_container_tab_landing() {
    let _guard = SERIAL.lock().unwrap();
    crate::app::reset_book_assembly();
    let config = EngineConfig {
        storage: Some(nsh_testing::MemoryStorage::new(vec![(
            "view_state:closed_features".to_owned(),
            "[\"policies\",\"merchants\"]".to_owned(),
        )])),
        ..EngineConfig::new(Root::of::<GuardProbeApp>())
    };
    let mut app = Headless::start(config).unwrap();
    // ① 关闭态裸访问 → 概览兜底（旧壳 featureRouteGuard 重定向 dashboard 同语义）。
    app.wait_for("text:概览").unwrap();
    app.wait_for("text:保单").unwrap();
    app.wait_for("text:记账 · 更多·Some(\"scheduled\")")
        .unwrap();
    app.wait_for("text:商户").unwrap();
}
