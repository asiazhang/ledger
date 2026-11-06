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

/// 经域层公开写入口在同一账户上造一列带指定中文备注的支出（搜索断言的
/// 原文命中造数面：备注各不相同、日期递增；账户名固定「测试现金」不与
/// 搜索关键词相交）。
fn seed_expense_notes(dir: &Path, notes: &[&str]) {
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
    for (index, note) in notes.iter().enumerate() {
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
                note: Some((*note).to_owned()),
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

/// 搜索命中（issue #2023 AC1）：中文关键词经服务端统一模糊搜索命中——原文
/// 连续子串语义（「火锅」命中「元旦火锅聚餐」中部），与旧壳同一域函数同结果；
/// 总数随搜索语义变化（共 2 条 → 命中 1 条），命中行在屏、未命中行离屏。
#[test]
fn app_window_searches_by_chinese_keyword_with_raw_text_hits() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search");
    seed_expense_notes(&dir, &["元旦火锅聚餐", "超市买菜"]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    // 列表态基线：共 2 条，两行都在屏（date DESC：01-02 在前）。
    app.wait_for("text:共 2 条").unwrap();
    app.wait_for("text:2026-01-02").unwrap();
    app.wait_for("text:2026-01-01").unwrap();

    // 输入中文关键词 → 服务端搜索：总数切换「命中 1 条」，命中行（01-01）
    // 在屏、未命中行（01-02）离屏。
    app.type_text("id:tx-search", "火锅").unwrap();
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("text:2026-01-01").unwrap();
    app.wait_for_gone("text:2026-01-02", DEFAULT_WAIT).unwrap();
}

/// 搜索无结果空态（issue #2023 AC3）：无命中关键词呈现「无匹配结果」（与旧壳
/// search.noResults 同文案）与「命中 0 条」（旧壳搜索页 searched 即显示
/// hitCount 同形），表格不残留列表行。
#[test]
fn app_window_shows_empty_state_when_search_has_no_hits() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-empty");
    seed_expense_notes(&dir, &["元旦火锅聚餐", "超市买菜"]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 2 条").unwrap();
    app.type_text("id:tx-search", "不存在的关键词").unwrap();
    app.wait_for("text:无匹配结果").unwrap();
    app.wait_for("text:命中 0 条").unwrap();
    app.wait_for_gone("text:2026-01-01", DEFAULT_WAIT).unwrap();
    app.wait_for_gone("text:2026-01-02", DEFAULT_WAIT).unwrap();
}

/// 清空恢复列表（issue #2023）：搜索命中后清空输入框 → 回列表态——总数回
/// 「共 2 条」、全部行回屏、搜索态文案离屏（搜索态与列表态双向可逆，与旧壳
/// 空查询回占位同语义）。
#[test]
fn app_window_restores_list_when_search_cleared() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-clear");
    seed_expense_notes(&dir, &["元旦火锅聚餐", "超市买菜"]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 2 条").unwrap();
    app.type_text("id:tx-search", "火锅").unwrap();
    app.wait_for("text:命中 1 条").unwrap();

    app.clear("id:tx-search").unwrap();
    app.wait_for("text:共 2 条").unwrap();
    app.wait_for("text:2026-01-02").unwrap();
    app.wait_for("text:2026-01-01").unwrap();
    app.wait_for_gone("text:命中 1 条", DEFAULT_WAIT).unwrap();
}

/// 搜索态分页与总数（issue #2023）：搜索态沿用服务端 offset 分页语义（与旧壳
/// 搜索页同形）——150 笔同备注命中总数 150、分页条两页；回车立即搜索（不等
/// 防抖，旧壳 onEnter 同语义）后点击页码 2 翻末页（最旧 50 笔），首页最新行
/// 离屏；搜索态总数与列表态同值不同词（命中 150 条）。
#[test]
fn app_window_paginates_search_hits_after_enter_search() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-paged");
    seed_expense_notes(&dir, &["批量造数"; 150]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 150 条").unwrap();
    app.type_text("id:tx-search", "批量造数").unwrap();
    app.submit("id:tx-search").unwrap();
    app.wait_for("text:命中 150 条").unwrap();

    // 搜索态翻页：点击页码 2 → 末页（最旧 50 笔，2026-01-01..2026-02-19），
    // 首页最新行不再在屏（断言对准视口内首行，与列表态翻页同口径）。
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
