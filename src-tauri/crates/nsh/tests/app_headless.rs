//! headless 组件测试（issue #2018 交易表格首屏；docs/research/
//! nsh-testing-capabilities.md §2.1 裁决的 UI 自动化接缝）。
//!
//! 进程内起真实视图树（`Root::of::<App>`，无窗口无平台后端），断言只写用户
//! 可见结果：表格呈现真实账本数据（格文本来自装配层）、分页条点击翻页与空
//! 态。装配入口注入测试库路径（生产与测试同一装配路径）；建库走产品
//! 开库入口、造数经域层公开写入口（与 lib 单测同纪律）。域栈引导在组件任务的
//! 后台线程（blocking），呈现晚于步骤返回——一律 `wait_for`（§3.5 通用纪律 4）；
//! 忙碌指示是装载期间的时序性呈现（本地读毫秒级完成、屏幕不可观测），归人工
//! 验收清单（#2024 硬伤清单对表），不进自动化断言面。
//!
//! 桌面形态显式置定（§3.5：headless 缺省手机惯用语），`nsh-base` 组件集显式
//! 注入（`nsh-testing` 缺省 ui_sets 不带 nsh-base；平台宿主生产侧缺省注册的
//! 正是同一 registry）。

// 测试整体豁免（ADR-0060）：集成测试 crate 经 cfg(test) 放行六件套，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use chrono::Datelike;
use ledger_accounts::{AccountInput, AccountType};
use ledger_infra::ids::new_uuid;
use ledger_transaction::{TransactionInput, TransactionKind};
use nsh_engine::{EngineConfig, Environment, Idiom, Os, Points, Root, Size};
use nsh_testing::Headless;

/// 与 `wait_for` 缺省同一超时（nsh-testing 的 `wait_for_gone` 需显式超时）。
const DEFAULT_WAIT: Duration = Duration::from_secs(5);

/// 串行锁：装配入口（`set_book_dir`）与会话句柄是进程级单例，用例间串行防串扰
/// （与 lib 单测的 SERIAL 同形；两测试二进制是独立进程，互不串扰）。
static SERIAL: Mutex<()> = Mutex::new(());

/// 桌面形态 headless 配置（足够容纳表格与分页条的视口）。
fn desktop_config() -> EngineConfig {
    EngineConfig {
        viewport: Size::new(Points(1024.0), Points(720.0)),
        environment: Environment {
            os: Os::Linux,
            idiom: Idiom::Desktop,
            ..Environment::default()
        },
        ui_sets: vec![nsh_base::registry()],
        ..EngineConfig::new(Root::of::<nsh_app::app::App>())
    }
}

/// 测试账本（产品开库入口建库：0 字节文件 = 新装语义）。
fn make_book(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ledger-test-nsh-table-{tag}-{}", new_uuid()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("ledger.db"), b"").unwrap();
    dir
}

/// 造数日期：2026-01-01 起（平年月份表逐日推进，插入序 = 日期升序）。
fn test_date(index: usize) -> String {
    const MONTH_DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut year = 2026;
    let mut month = 0usize;
    let mut rest = index as u32;
    loop {
        let days = MONTH_DAYS[month];
        if rest < days {
            return format!("{year}-{:02}-{:02}", month + 1, rest + 1);
        }
        rest -= days;
        month += 1;
        if month == 12 {
            month = 0;
            year += 1;
        }
    }
}

/// 经域层公开写入口在同一账户上造一列现金支出（每笔 ¥25，日期递增）。
fn seed_expense_series(dir: &Path, count: usize) {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let account = ledger_accounts::create_account(
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
    for index in 0..count {
        ledger_transaction::create(
            &conn,
            TransactionInput {
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                currency_code: "CNY".to_owned(),
                account_id: Some(account.clone()),
                to_account_id: None,
                funding_account_id: None,
                funding: Vec::new(),
                purchases: Vec::new(),
                category_id: None,
                merchant_id: None,
                merchant_name: None,
                policy_id: None,
                refund_of_transaction_id: None,
                note: Some(format!("造数第 {index} 笔")),
                source_order_no: None,
                date: test_date(index),
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
}

/// 在指定日期造一笔指定金额的现金支出（期间维度读路径的造数入口，
/// 与 lib 单测同纪律：建库走产品开库入口，造数经域公开写入口）。
fn seed_expense_on_date(dir: &Path, date: &str, amount_cents: i64) {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let account = ledger_accounts::create_account(
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
    ledger_transaction::create(
        &conn,
        TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents,
            currency_code: "CNY".to_owned(),
            account_id: Some(account),
            to_account_id: None,
            funding_account_id: None,
            funding: Vec::new(),
            purchases: Vec::new(),
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: Some(format!("支出 {date}")),
            source_order_no: None,
            date: date.to_owned(),
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

/// 测试侧「今天」（本地时区，与被测应用的 now() 同源同口径）。
fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// 上月中旬日（15 日，恒在上个自然月内，与「今天」无关的稳定锚点）。
fn prev_month_mid() -> chrono::NaiveDate {
    let t = today();
    let first_of_prev = chrono::NaiveDate::from_ymd_opt(t.year(), t.month(), 1)
        .unwrap()
        .checked_sub_days(chrono::Days::new(1))
        .unwrap();
    first_of_prev
        .with_day(1)
        .unwrap()
        .checked_add_days(chrono::Days::new(14))
        .unwrap()
}

/// 期间标签选择器（旧壳 quickTimeRange.periodLabel.month 同词表）。
fn month_label(year: i32, month: u32) -> String {
    format!("text:{year}年{month}月")
}

/// 表格首屏（issue #2018 AC1）：就绪窗口呈现真实账本数据——列名、格文本
/// （类型标签 / 金额 / 日期为装配层产物）与总数齐备。删掉接线（装配注入 /
/// 引导 / 首页读 / 表格渲染任一环）即红。
#[test]
fn app_window_renders_transaction_table_of_seeded_book() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("one");
    seed_expense_series(&dir, 1);

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 1 条").unwrap();
    let texts = app.screen().texts();
    for expected in ["日期", "类型", "金额", "2026-01-01", "支出", "¥25"] {
        assert!(
            texts.contains(&expected),
            "表格应呈现「{expected}」：{:?}",
            app.screen().texts()
        );
    }
}

/// 空库空态（issue #2018 AC1 三态之一）：空账本呈现「暂无数据」，不呈现总数
/// 与分页条。
#[test]
fn app_window_shows_empty_state_for_empty_book() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("empty");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:暂无数据").unwrap();
    let texts = app.screen().texts();
    assert!(
        !texts.iter().any(|t| t.starts_with("共 ")),
        "空库不呈现总数：{:?}",
        texts
    );
}

/// 分页条点击翻页（issue #2018 AC1）：150 笔两页——首页含最新行，点击页码
/// 「2」翻页后呈现末页行集且首页最新行不再在屏。
#[test]
fn app_window_flips_pages_by_pagination_bar_click() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("paged");
    seed_expense_series(&dir, 150);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    // 首页（最新 100 笔）：总数与首页首行（第 150 笔，2026-05-30）在屏。
    app.wait_for("text:共 150 条").unwrap();
    app.wait_for("text:2026-05-30").unwrap();

    // 点击页码 2 → 末页行集替换（最旧 50 笔，2026-01-01..2026-02-19），首页
    // 最新行不再在屏。断言对准视口内的首行（表格按视口虚拟化，视口外的行不
    // 进视图树）；忙碌指示（loadingView 文案）是装载期间的时序性呈现——headless
    // 本地读毫秒级完成、屏幕不可观测，归人工验收清单（#2024 硬伤清单对表）。
    app.press("role:button[2]").unwrap();
    app.wait_for("text:2026-02-19").unwrap();
    app.wait_for_gone("text:2026-05-30", DEFAULT_WAIT).unwrap();
}

/// 上一页按钮（zh-CN 文案，issue #2018）：末页点「上一页」回到首页，首页
/// 最新行回到屏上（上一页/下一页的可见文案即其无障碍标签）。
#[test]
fn app_window_steps_back_by_previous_button() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("back");
    seed_expense_series(&dir, 150);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 150 条").unwrap();
    app.press("role:button[2]").unwrap();
    app.wait_for("text:2026-02-19").unwrap();

    // 「上一页」（zh-CN 可见文案即无障碍标签）回到首页，首页最新行回到屏。
    app.press("role:button[上一页]").unwrap();
    app.wait_for("text:2026-05-30").unwrap();
    app.wait_for_gone("text:2026-02-19", DEFAULT_WAIT).unwrap();
}

/// 翻页快捷键（issue #2021 / ADR-0140）：`]` 下一页 / `[` 上一页，语义等同
/// 点击分页条（同一读路径、同一行集替换）；首页 `[` 越界幂等无操作。断言
/// 对准用户可见结果（行集内容），删除 keymap 接线即红。
#[test]
fn pagination_shortcut_keys_flip_pages_and_stay_idempotent_at_ends() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("pg-keys");
    seed_expense_series(&dir, 150);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 150 条").unwrap();
    app.wait_for("text:2026-05-30").unwrap();

    // `]` 下一页 → 末页行集（与分页条点击同一读路径，同 #2018 点击用例断言）。
    app.press_key("]").unwrap();
    app.wait_for("text:2026-02-19").unwrap();
    app.wait_for_gone("text:2026-05-30", DEFAULT_WAIT).unwrap();

    // `[` 上一页 → 回首页。
    app.press_key("[").unwrap();
    app.wait_for("text:2026-05-30").unwrap();
    app.wait_for_gone("text:2026-02-19", DEFAULT_WAIT).unwrap();

    // 首页 `[` 越界：幂等无操作（首/末页不提示不抖动，ADR-0140 决策 2）。
    app.press_key("[").unwrap();
    app.settle();
    assert!(
        app.shows("2026-05-30") && !app.shows("2026-02-19"),
        "首页 `[` 应原地不动：{:?}",
        app.screen().texts()
    );
}

/// 周期步进快捷键与步进器点击（issue #2021 / ADR-0141）：`,` / `.` 语义等同
/// 点击步进器（同一 [`nsh_app::period`] 决策单点），钳制于数据期间边界——
/// 当前期间即最晚期间时 `.` 幂等，最早期间时第二次 `,` 幂等；换期间同时换
/// 行集（期间限定读路径）。数据期间边界与标签相对真实「今天」派生（测试与
/// 被测应用同源取钟，与具体日期无关）。
#[test]
fn period_shortcut_keys_and_stepper_click_step_and_clamp_at_boundary() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("period-keys");
    seed_expense_on_date(&dir, &prev_month_mid().to_string(), 8_800);
    seed_expense_on_date(&dir, &today().to_string(), 2_500);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 2 条").unwrap();

    // 经期间选择弹层进入「当月」：标签 = 本月，行集只剩本月一笔（¥88 不在屏）。
    let t = today();
    app.press("id:period-label").unwrap();
    app.wait_for("id:period-chip-month").unwrap();
    app.press("id:period-chip-month").unwrap();
    app.wait_for_gone("id:period-chip-month", DEFAULT_WAIT)
        .unwrap();
    app.wait_for(&month_label(t.year(), t.month())).unwrap();
    app.wait_for("text:¥25").unwrap();
    assert!(!app.shows("¥88"), "当月行集不含上月支出");

    // `.`：当前期间即边界最晚（数据不晚于今天）→ 幂等无操作。
    app.press_key(".").unwrap();
    app.settle();
    assert!(
        app.shows(&format!("{}年{}月", t.year(), t.month())),
        "边界末端 `.` 应原地不动"
    );

    // `,`：步进到上月（行集换血 → ¥88 在屏、¥25 离屏）。
    app.press_key(",").unwrap();
    let p = prev_month_mid();
    app.wait_for(&month_label(p.year(), p.month())).unwrap();
    app.wait_for("text:¥88").unwrap();
    app.wait_for_gone("text:¥25", DEFAULT_WAIT).unwrap();

    // 第二次 `,`：最早期间即边界 → 幂等无操作。
    app.press_key(",").unwrap();
    app.settle();
    assert!(
        app.shows(&format!("{}年{}月", p.year(), p.month())),
        "边界始端第二次 `,` 应原地不动"
    );

    // 步进器点击（「下一个周期」，zh-CN 无障碍标签）与 `.` 同一出口：回到当月。
    app.press("role:button[下一个周期]").unwrap();
    app.wait_for(&month_label(t.year(), t.month())).unwrap();
    app.wait_for("text:¥25").unwrap();
    app.wait_for_gone("text:¥88", DEFAULT_WAIT).unwrap();
}

/// 抑制闸门——翻页族（issue #2021 / ADR-0035 原生壳重建）：注册抑制的弹层
/// （期间选择弹层）开着时 `[` 不派发（页码不动），关闭后同键翻页生效。
/// 「删除即红」负向条目：删掉 keymap 处理器中的 `overlay::has_open_overlay`
/// 检查 → 本测红（弹层下按键把页翻了）。断言对准用户可见结果（行集），
/// 经 settle 给错误路径落地窗口（红态下翻页在毫秒级完成），不写可点性断言。
#[test]
fn suppression_gate_blocks_pagination_key_while_period_popup_open() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("suppress-pg");
    seed_expense_series(&dir, 150);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 150 条").unwrap();
    app.press_key("]").unwrap();
    app.wait_for("text:2026-02-19").unwrap();

    // 打开注册抑制的弹层 → `[` 不派发（第 2 页行集原样）。
    app.press("id:period-label").unwrap();
    app.wait_for("id:period-chip-all").unwrap();
    app.press_key("[").unwrap();
    app.settle();
    assert!(
        app.shows("2026-02-19") && !app.shows("2026-05-30"),
        "弹层开着时 `[` 不得翻页：{:?}",
        app.screen().texts()
    );

    // 关弹层（ESC 走弹层关闭语义）→ 同键翻页生效。
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:period-chip-all", DEFAULT_WAIT)
        .unwrap();
    app.press_key("[").unwrap();
    app.wait_for("text:2026-05-30").unwrap();
    app.wait_for_gone("text:2026-02-19", DEFAULT_WAIT).unwrap();
}

/// 抑制闸门——周期步进族（issue #2021 / ADR-0141）：同一弹层、同一闸门，
/// `,` 不派发（期间标签不动），关闭后同键步进生效；删除任一族处理器中的
/// 闸门检查 → 对应测试红。
#[test]
fn suppression_gate_blocks_period_key_while_period_popup_open() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("suppress-period");
    seed_expense_on_date(&dir, &prev_month_mid().to_string(), 8_800);
    seed_expense_on_date(&dir, &today().to_string(), 2_500);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 2 条").unwrap();

    // 进入「当月」（有游标、有可步进空间——闸门拦截才是可观察的差异）。
    let t = today();
    app.press("id:period-label").unwrap();
    app.wait_for("id:period-chip-month").unwrap();
    app.press("id:period-chip-month").unwrap();
    app.wait_for_gone("id:period-chip-month", DEFAULT_WAIT)
        .unwrap();
    app.wait_for(&month_label(t.year(), t.month())).unwrap();

    // 弹层开着 → `,` 不派发（标签不动、行集不动）。
    app.press("id:period-label").unwrap();
    app.wait_for("id:period-chip-all").unwrap();
    app.press_key(",").unwrap();
    app.settle();
    assert!(
        app.shows(&format!("{}年{}月", t.year(), t.month())),
        "弹层开着时 `,` 不得步进：{:?}",
        app.screen().texts()
    );

    // 关弹层 → 同键步进生效（上月标签 + 行集换血）。
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:period-chip-all", DEFAULT_WAIT)
        .unwrap();
    app.press_key(",").unwrap();
    let p = prev_month_mid();
    app.wait_for(&month_label(p.year(), p.month())).unwrap();
    app.wait_for("text:¥88").unwrap();
}
