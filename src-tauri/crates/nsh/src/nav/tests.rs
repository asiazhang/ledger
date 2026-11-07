//! 导航接缝探针测试（lib 单测；headless 引擎内驱动下钻接缝——引擎线程契约
//! 下，[`crate::nav::open`] 的 focus 意图投递没有真实 UI 生产面（来源列跳转
//! 随交易列表筛选票接线），探针组件在引擎线程内经**真实生产路径**
//! （[`crate::nav_model::resolve_source_jump`] → [`crate::nav::open`]）投递，
//! 消费面挂**真实占位组件**（挂载读一次 + 回显），断言用户可见文本。
//!
//! 「删除即红」负向条目：删 [`crate::nav::open`] 内的意图落位（`intent` 写）
//! → 探针红（无回显）；删占位组件的 [`crate::nav::take_intent`] 消费 → 探针
//! 红（回显不出现）；删容器页签落位 → 页签切换探针红。

use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Environment, Idiom, Os, Root};
use nsh_testing::Headless;

use std::sync::Mutex;

use crate::app::test_assembly_guard;
use crate::nav::{self, NavStore};
use crate::nav_model::{self, GroupId, NavRequest, SourceKind, ViewName};
use crate::views;

const WAIT: std::time::Duration = std::time::Duration::from_secs(5);

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
    let _guard = test_assembly_guard();
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
    let _guard = test_assembly_guard();
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

// ---------------------------------------------------------------------------
// 交易筛选下钻落位（issue #2037，骨架导航接缝的第一个真实消费方）。
// ---------------------------------------------------------------------------

/// 落位载荷槽（测试注入：账户 id 由造数面生成，探针组件体经它取载荷）。
static DRILL_ACCOUNT: Mutex<Option<String>> = Mutex::new(None);

/// 真实交易视图消费探针：引擎线程内经**真实生产路径**（[`nav::open`] 携交易
/// 筛选载荷）投递，落位交易列表挂**真实交易视图**（挂载消费 [`nav::take_intent`]
/// → 全量替换筛选态 → 行集按落位筛选呈现）。
#[component]
fn TxFilterDrillProbeApp() -> impl IntoElement {
    // 会话就位（阻塞 SQLite IO 直呼组件体——测试探针，量产路径在 App 启动
    // 竖井的 blocking 任务里；boot 成功先于交易视图装载读会话句柄）。
    assert_eq!(crate::app::run_startup(), crate::app::StartupView::Ready);
    let account = DRILL_ACCOUNT
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .unwrap_or_default();
    nav::open(NavRequest::bare(ViewName::Transactions).with_tx_filters(
        crate::tx_filter::DrillFilters {
            account: (!account.is_empty()).then_some(account),
            ..crate::tx_filter::DrillFilters::default()
        },
    ));
    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(crate::app::transactions_view())
}

/// 下钻筛选落位全链：载荷投递 → 守卫落位交易列表 → 意图投递 → 真实视图挂载
/// 消费（筛选会话态全量替换）→ 行集按落位筛选呈现 + 清除按钮激活。「删除即
/// 红」负向条目：删 [`nav::open`] 的 tx_filters 意图投递（intent 写）或删交易
/// 视图挂载消费（[`nav::take_intent`]）→ 落位筛选不生效，行集回全量即红。
#[test]
fn seam_drill_filters_land_on_real_transactions_view() {
    use ledger_accounts::{AccountInput, AccountType};
    use ledger_transaction::{TransactionInput, TransactionKind};

    let _guard = test_assembly_guard();
    crate::app::reset_book_assembly();

    // 造数（两账户各一笔支出；账户 id 经载荷槽注入探针）。账本目录落唯一
    // 暂存目录（建库走产品开库入口，tests.rs 造数纪律同形）。
    let dir = std::env::temp_dir().join(format!(
        "nsh-nav-drill-test-{}",
        ledger_infra::ids::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ledger.db"), b"").unwrap();
    let db = crate::boot::boot_ledger(&dir).unwrap();
    let (cash, bank) = {
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        let cash = ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "测试现金".to_owned(),
                kind: AccountType::Cash,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
        let bank = ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "银行卡".to_owned(),
                kind: AccountType::Bank,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
        for (account, amount, note) in [(&cash, 2_500i64, "现金笔"), (&bank, 6_600, "银行卡笔")]
        {
            ledger_transaction::create(
                &conn,
                TransactionInput {
                    kind: TransactionKind::Expense,
                    amount_cents: amount,
                    currency_code: "CNY".to_owned(),
                    account_id: Some(account.to_owned()),
                    to_account_id: None,
                    funding_account_id: None,
                    funding: Vec::new(),
                    purchases: Vec::new(),
                    category_id: None,
                    merchant_id: None,
                    merchant_name: None,
                    policy_id: None,
                    refund_of_transaction_id: None,
                    note: Some(note.to_owned()),
                    source_order_no: None,
                    date: "2026-01-05".to_owned(),
                    instrument_id: None,
                    quantity: None,
                    price_cents: None,
                    fee_cents: None,
                    to_instrument_id: None,
                    to_quantity: None,
                    out_amount_cents: None,
                    in_amount_cents: None,
                    origin: None,
                    fx_rate: None,
                    idempotency_key: None,
                },
            )
            .unwrap();
        }
        (cash, bank)
    };
    *DRILL_ACCOUNT.lock().unwrap_or_else(|p| p.into_inner()) = Some(bank.clone());

    // 会话目录注入（会话就位在探针组件体内执行——boot 与信号单例都必须在
    // 引擎线程；交易视图装载读会话句柄，boot 成功先于视图呈现）。
    crate::app::set_book_dir(dir.clone());

    let config = EngineConfig {
        viewport: nsh_engine::Size::new(nsh_engine::Points(1024.0), nsh_engine::Points(720.0)),
        environment: Environment {
            os: Os::Linux,
            idiom: Idiom::Desktop,
            ..Environment::default()
        },
        ui_sets: vec![nsh_base::registry()],
        ..EngineConfig::new(Root::of::<TxFilterDrillProbeApp>())
    };
    let mut app = Headless::start(config).unwrap();

    // 落位筛选生效的行集凭据：只剩银行卡一笔；现金笔离屏。
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:¥66").unwrap();
    assert!(
        !app.shows("¥25"),
        "落位筛选（涉及账户）应过滤行集：{:?}",
        app.screen().texts()
    );
    // 落位筛选的呈现凭据：账户触发件回显选中项、清除筛选激活（无障碍名锚点
    // 命中按钮文本）。
    app.wait_for("text:银行卡").unwrap();
    assert!(
        app.screen().enabled("id:tx-filter-clear").unwrap(),
        "落位筛选在场 → 清除筛选激活"
    );

    let _ = (cash, bank);
}
