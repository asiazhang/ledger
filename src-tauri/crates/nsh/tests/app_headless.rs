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
use ledger_categories::{CategoryInput, create_category};
use ledger_infra::ids::new_uuid;
use ledger_transaction::{
    TransactionFundingInput, TransactionInput, TransactionKind, TransactionPurchaseInput,
};
use nsh_core::platform::ColorScheme;
use nsh_engine::{EngineConfig, Environment, Idiom, Os, Points, Root, Size};
use nsh_testing::Headless;

/// 与 `wait_for` 缺省同一超时（nsh-testing 的 `wait_for_gone` 需显式超时）。
const DEFAULT_WAIT: Duration = Duration::from_secs(5);

/// 串行锁：装配入口（`set_book_dir`）与会话句柄是进程级单例，用例间串行防串扰
/// （与 lib 单测的 SERIAL 同形；两测试二进制是独立进程，互不串扰）。
static SERIAL: Mutex<()> = Mutex::new(());

/// 桌面形态 headless 配置（足够容纳表格与分页条的视口），带设备存储初值
/// （金额隐私开关的轻量设置项面；`None` = 缺省空存储）。
fn desktop_config_with_storage(entries: &[(&str, &str)]) -> EngineConfig {
    EngineConfig {
        storage: Some(nsh_testing::MemoryStorage::new(
            entries
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<Vec<_>>(),
        )),
        ..desktop_config()
    }
}

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

/// 造一个账户（用例共用；类型与名称随参）。
fn seed_account(db: &ledger_infra::db::DbState, name: &str, kind: AccountType) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_accounts::create_account(
        &conn,
        AccountInput {
            name: name.to_owned(),
            kind,
            currency_code: "CNY".to_owned(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap()
}

/// 造一个现金账户（用例共用）。
fn seed_cash_account(db: &ledger_infra::db::DbState) -> String {
    seed_account(db, "测试现金", AccountType::Cash)
}

/// 经域层公开写入口造一笔交易（用例共用的最小写面）。
fn seed_transaction(
    db: &ledger_infra::db::DbState,
    account: &str,
    kind: TransactionKind,
    amount_cents: i64,
    date: &str,
    category_id: Option<String>,
    note: Option<String>,
) {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_transaction::create(
        &conn,
        TransactionInput {
            kind,
            amount_cents,
            currency_code: "CNY".to_owned(),
            account_id: Some(account.to_owned()),
            to_account_id: None,
            funding_account_id: None,
            funding: Vec::new(),
            purchases: Vec::new(),
            category_id,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note,
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

/// 金额隐私遮蔽接线（issue #2019）：设备轻量设置项 `amount_privacy_enabled`
/// 置真（与旧壳同键同值形态），表格金额呈现恒形掩码 `••••`、明文金额不在屏；
/// 类型标签等其余格照常。删掉设备态读取（`amount_privacy_enabled`）或装配
/// 消费任一环，明文金额回到屏上即红。
#[test]
fn app_window_masks_amounts_when_device_privacy_enabled() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("privacy");
    seed_expense_series(&dir, 1);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config_with_storage(&[(
        "amount_privacy_enabled",
        "true",
    )]))
    .unwrap();

    app.wait_for("text:••••").unwrap();
    let texts = app.screen().texts();
    assert!(
        !texts.contains(&"¥25"),
        "隐私开启后金额明文不在屏：{:?}",
        texts
    );
    assert!(texts.contains(&"支出"), "掩码态其余格照常：{:?}", texts);
}

/// 金额语义色接线（issue #2019）：支出红 / 收入绿逐值镜像旧壳
/// `kindSemanticColor`（packages/theme），亮暗两态随系统外观切换。删掉
/// `render_cell` 接线或色值映射任一环即红。
#[test]
fn app_window_colors_amount_cells_by_kind_semantic() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("colors");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        seed_transaction(
            &db,
            &account,
            TransactionKind::Expense,
            2_500,
            "2026-01-15",
            None,
            None,
        );
        seed_transaction(
            &db,
            &account,
            TransactionKind::Income,
            1_000,
            "2026-01-16",
            None,
            None,
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:¥25").unwrap();
    assert_eq!(
        app.screen().span_style("text:¥25").unwrap().color,
        nsh_core::Color(0xd03050ff),
        "支出金额亮色 = 旧壳 expense light"
    );
    assert_eq!(
        app.screen().span_style("text:¥10").unwrap().color,
        nsh_core::Color(0x18a058ff),
        "收入金额亮色 = 旧壳 income light"
    );

    // 系统切暗色（用户在外观设置切换）：金额色随旧壳暗色变体。
    app.set_environment("dark mode", |env| env.color_scheme = ColorScheme::Dark)
        .unwrap();
    assert_eq!(
        app.screen().span_style("text:¥25").unwrap().color,
        nsh_core::Color(0xe88080ff),
        "支出金额暗色 = 旧壳 expense dark"
    );
    assert_eq!(
        app.screen().span_style("text:¥10").unwrap().color,
        nsh_core::Color(0x63e2b7ff),
        "收入金额暗色 = 旧壳 income dark"
    );
}

/// 分类路径与备注列接线（issue #2019）：父子分类的支出行呈现「父 > 子」路径
/// 与备注原文；无分类无备注行落 `-`。删掉分类读接缝、路径装配或列映射任一环
/// 即红。
#[test]
fn app_window_renders_category_path_and_note_cells() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("category");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        let parent = create_category(
            &conn,
            CategoryInput {
                name: "餐饮".to_owned(),
                kind: "expense".to_owned(),
                parent_id: None,
                icon: None,
            },
        )
        .unwrap();
        let child = create_category(
            &conn,
            CategoryInput {
                name: "午餐".to_owned(),
                kind: "expense".to_owned(),
                parent_id: Some(parent),
                icon: None,
            },
        )
        .unwrap();
        drop(conn);
        let account = seed_cash_account(&db);
        seed_transaction(
            &db,
            &account,
            TransactionKind::Expense,
            2_500,
            "2026-01-15",
            Some(child),
            Some("买咖啡".to_owned()),
        );
        seed_transaction(
            &db,
            &account,
            TransactionKind::Expense,
            1_000,
            "2026-01-16",
            None,
            None,
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:餐饮 > 午餐").unwrap();
    let texts = app.screen().texts();
    for expected in ["分类", "商品 / 备注", "买咖啡", "-"] {
        assert!(
            texts.contains(&expected),
            "表格应呈现「{expected}」：{:?}",
            texts
        );
    }
}

/// 购买项订单展开（issue #2020 / ADR-0138 决策 13/14）：一个两购买项订单在新壳
/// 呈现为两行——分类与商品逐行、订单级列（日期/类型/商户/账户/金额）只在首行
/// 出现一次（DataTable 无纵向合并，首行带值镜像旧壳合并格），出资分解标注与
/// 商户名在首行；「共 N 条」按交易计（2 行仍共 1 条）。删掉行展开装配任一环
/// （展开 / 订单级留空 / 逐行分类与商品）即红。
#[test]
fn app_window_renders_purchase_order_rows_with_count_by_transaction() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("order");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        let food = create_category(
            &conn,
            CategoryInput {
                name: "食品".to_owned(),
                kind: "expense".to_owned(),
                parent_id: None,
                icon: None,
            },
        )
        .unwrap();
        let snack = create_category(
            &conn,
            CategoryInput {
                name: "零食".to_owned(),
                kind: "expense".to_owned(),
                parent_id: Some(food),
                icon: None,
            },
        )
        .unwrap();
        let daily = create_category(
            &conn,
            CategoryInput {
                name: "日用".to_owned(),
                kind: "expense".to_owned(),
                parent_id: None,
                icon: None,
            },
        )
        .unwrap();
        let care = create_category(
            &conn,
            CategoryInput {
                name: "洗护".to_owned(),
                kind: "expense".to_owned(),
                parent_id: Some(daily),
                icon: None,
            },
        )
        .unwrap();
        drop(conn);
        let piggy = seed_account(&db, "小金库", AccountType::Ewallet);
        let credit = seed_account(&db, "余额", AccountType::Credit);
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_transaction::create(
            &conn,
            TransactionInput {
                kind: TransactionKind::Expense,
                amount_cents: 28_160,
                currency_code: "CNY".to_owned(),
                // 出资分解与 account_id 互斥（ADR-0138 决策 2）：分解行不带 account_id。
                account_id: None,
                to_account_id: None,
                funding_account_id: None,
                funding: vec![
                    TransactionFundingInput {
                        account_id: piggy,
                        amount_cents: 2_853,
                        label: None,
                    },
                    TransactionFundingInput {
                        account_id: credit,
                        amount_cents: 25_307,
                        label: None,
                    },
                ],
                purchases: vec![
                    TransactionPurchaseInput {
                        name: "猫粮".to_owned(),
                        quantity: 2,
                        category_id: Some(snack),
                        unit_price_cents: None,
                    },
                    TransactionPurchaseInput {
                        name: "洗衣液".to_owned(),
                        quantity: 1,
                        category_id: Some(care),
                        unit_price_cents: None,
                    },
                ],
                category_id: None,
                merchant_id: None,
                merchant_name: Some("京东".to_owned()),
                policy_id: None,
                refund_of_transaction_id: None,
                note: None,
                source_order_no: Some("JD-1001".to_owned()),
                date: "2026-01-15".to_owned(),
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

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    // 「共 N 条」按交易计：两个购买项行仍共 1 条（决策 14，购买项行不虚增计数）。
    app.wait_for("text:共 1 条").unwrap();
    let texts = app.screen().texts();
    for expected in [
        "京东",
        "小金库 等 2 账户",
        "¥281.6",
        "猫粮 共 2 件",
        "洗衣液 共 1 件",
        "食品 > 零食",
        "日用 > 洗护",
    ] {
        assert!(
            texts.contains(&expected),
            "表格应呈现「{expected}」：{:?}",
            texts
        );
    }
    // 订单级格只在首行出现一次（旧壳合并格「内容只出现一次」的可观察镜像）。
    let count = |needle: &str| texts.iter().filter(|t| ***t == *needle).count();
    assert_eq!(count("支出"), 1, "类型列只在首行：{:?}", texts);
    assert_eq!(count("2026-01-15"), 1, "日期列只在首行：{:?}", texts);
    assert_eq!(count("¥281.6"), 1, "金额列只在首行：{:?}", texts);
    assert_eq!(count("京东"), 1, "商户列只在首行：{:?}", texts);
}

/// 借贷派生标签与双账户标注（issue #2020）：资金账户 → 应收账户的转账呈现
/// 「借出」专属文案与「转出 → 转入」双账户格（旧壳 kindLabel / accountCellView
/// 同口径）。删掉借贷派生或账户格装配任一环即红。
#[test]
fn app_window_renders_lending_label_and_two_account_cell() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("lending");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_account(&db, "测试现金", AccountType::Cash);
        let receivable = seed_account(&db, "应收款", AccountType::Receivable);
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_transaction::create(
            &conn,
            TransactionInput {
                kind: TransactionKind::Transfer,
                amount_cents: 5_000,
                currency_code: "CNY".to_owned(),
                account_id: Some(cash),
                to_account_id: Some(receivable),
                funding_account_id: None,
                funding: Vec::new(),
                purchases: Vec::new(),
                category_id: None,
                merchant_id: None,
                merchant_name: None,
                policy_id: None,
                refund_of_transaction_id: None,
                note: None,
                source_order_no: None,
                date: "2026-01-15".to_owned(),
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

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:借出").unwrap();
    let texts = app.screen().texts();
    assert!(
        texts.contains(&"测试现金 → 应收款"),
        "双账户格「转出 → 转入」：{:?}",
        texts
    );
    assert!(
        !texts.contains(&"转账"),
        "借贷派生不落「转账」泛化文案：{:?}",
        texts
    );
}

/// 来源名与状态（issue #2020 / spec #704 来源列）：保单直挂交易呈现险种名；
/// 软删保单的历史引用另带「已删除」状态标注（在用保单无状态标注）。删掉来源
/// 反查接线的装配消费或状态标注任一环即红。
#[test]
fn app_window_renders_source_name_and_status() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("source");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        let insurer = ledger_policy::create_insurer_by_name(&conn, "众安").unwrap();
        let make_policy = |number: &str, product: &str| ledger_policy::PolicyInput {
            insurer_id: insurer.clone(),
            policy_number: number.to_owned(),
            product_name: product.to_owned(),
            start_date: "2026-01-01".to_owned(),
            end_date: None,
            coverage_amount_cents: None,
            coverage_currency_code: None,
            note: None,
        };
        let deleted =
            ledger_policy::create_policy(&conn, make_policy("P-1", "医疗险"), &mut || {}).unwrap();
        let alive =
            ledger_policy::create_policy(&conn, make_policy("P-2", "重疾险"), &mut || {}).unwrap();
        let make_expense = |date: &str, policy: String| TransactionInput {
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
            policy_id: Some(policy),
            refund_of_transaction_id: None,
            note: None,
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
        };
        ledger_transaction::create(&conn, make_expense("2026-01-15", deleted.clone())).unwrap();
        ledger_transaction::create(&conn, make_expense("2026-01-16", alive)).unwrap();
        // 软删保单：历史引用保留，来源列状态标注「已删除」。
        ledger_policy::delete_policy(&conn, &deleted, &mut || {}).unwrap();
        drop(conn);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:医疗险 已删除").unwrap();
    let texts = app.screen().texts();
    assert!(
        texts.contains(&"重疾险"),
        "在用保单来源只呈现险种名：{:?}",
        texts
    );
    assert!(
        !texts.contains(&"医疗险"),
        "软删保单的状态标注不缺席（无裸名形态）：{:?}",
        texts
    );
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

// ---------------------------------------------------------------------------
// 行右键删除全链路（issue #2022）：右键菜单 → 危险确认 → 域公开写入口直调删除
// → 进程内失效信号驱动列表重读。
// ---------------------------------------------------------------------------

/// 库态核对（不经 UI）：测试与被测应用同进程持有测试库路径，经产品开库入口
/// 另起会话直调域读 API（headless 断言原语无查库能力，research doc §3.3）。
fn book_transaction_dates(dir: &Path) -> Vec<String> {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let filter = ledger_transaction::TransactionListFilter {
        page: Some(1),
        page_size: Some(100),
        ..Default::default()
    };
    ledger_transaction::list_transactions(&conn, &filter)
        .unwrap()
        .items
        .into_iter()
        .map(|tx| tx.date)
        .collect()
}

/// 删除全旅程（issue #2022 AC1）：行右键菜单选「删除」→ 危险确认（error 级，
/// ADR-0078）→ 确认后交易删除生效且列表自动刷新（无需手动刷新）；总数减一、
/// 库中该笔消失。异步纪律：域写与重读在后台线程，一律 wait_for（§3.5）。
#[test]
fn row_context_menu_delete_confirms_then_refreshes_list() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("delete");
    seed_expense_notes(&dir, &["要删除的一笔", "要留下的一笔"]);

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 2 条").unwrap();

    // 行右键 → 上下文菜单呈现（zh-CN 文案与旧壳 transactions.menu.delete 同词表）。
    app.right_click("text:2026-01-01").unwrap();
    let menu = app.menu().expect("右键应打开上下文菜单");
    assert_eq!(
        menu.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        vec!["删除"],
        "行菜单项闭集（立桩只接删除）：{menu:?}"
    );
    app.choose_menu_item("删除").unwrap();

    // 危险确认对话框（error 级形态）：标题与说明段呈现，确认前不产生写入。
    app.wait_for("text:删除交易").unwrap();
    app.wait_for("text:删除后不可恢复。").unwrap();
    {
        let dates = book_transaction_dates(&dir);
        assert_eq!(dates.len(), 2, "确认对话框开着时不产生写入：{dates:?}");
    }

    // 确认 → 后台删除 → 失效信号驱动列表重读：行离屏、总数减一（删「要删除的
    // 一笔」（2026-01-01），留下的 2026-01-02 仍在屏）。删掉「失效信号→列表
    // 重读」接线（signals 广播点）本 wait_for_gone 超时红（库中已删而列表仍旧）。
    app.press("id:danger-confirm").unwrap();
    app.wait_for_gone("text:2026-01-01", DEFAULT_WAIT).unwrap();
    app.wait_for("text:共 1 条").unwrap();
    assert!(
        app.shows("2026-01-02"),
        "未删除的行留在刷新后的列表：{:?}",
        app.screen().texts()
    );

    // 库态核对（不经 UI）：被删笔软删生效，剩一笔。
    let dates = book_transaction_dates(&dir);
    assert_eq!(
        dates,
        vec!["2026-01-02".to_owned()],
        "库中剩一笔：{dates:?}"
    );
}

/// 取消路径（issue #2022 AC1）：确认对话框点「取消」不产生写入——行仍在屏、
/// 库中仍在（域读 API 核对）。
#[test]
fn row_context_menu_delete_cancel_writes_nothing() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("delete-cancel");
    seed_expense_notes(&dir, &["要删除的一笔", "要留下的一笔"]);

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 2 条").unwrap();
    app.right_click("text:2026-01-01").unwrap();
    app.choose_menu_item("删除").unwrap();
    app.wait_for("text:删除交易").unwrap();

    app.press("id:danger-cancel").unwrap();
    app.wait_for_gone("text:删除交易", DEFAULT_WAIT).unwrap();
    app.settle();
    assert!(
        app.shows("2026-01-01"),
        "取消后行仍在屏：{:?}",
        app.screen().texts()
    );
    assert!(
        app.shows("共 2 条"),
        "取消后总数不变：{:?}",
        app.screen().texts()
    );

    let dates = book_transaction_dates(&dir);
    assert_eq!(dates.len(), 2, "取消路径不产生写入：{dates:?}");
}

/// ESC 路径（issue #2022 AC1 / 用户故事 11）：ESC 按弹层关闭语义关闭确认对话
/// 框，不产生写入。
#[test]
fn row_context_menu_delete_escape_writes_nothing() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("delete-esc");
    seed_expense_notes(&dir, &["要删除的一笔", "要留下的一笔"]);

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 2 条").unwrap();
    app.right_click("text:2026-01-01").unwrap();
    app.choose_menu_item("删除").unwrap();
    app.wait_for("text:删除交易").unwrap();

    app.press_key("escape").unwrap();
    app.wait_for_gone("text:删除交易", DEFAULT_WAIT).unwrap();
    app.settle();
    assert!(
        app.shows("2026-01-01"),
        "ESC 后行仍在屏：{:?}",
        app.screen().texts()
    );

    let dates = book_transaction_dates(&dir);
    assert_eq!(dates.len(), 2, "ESC 路径不产生写入：{dates:?}");
}

/// 抑制接线——危险确认对话框（issue #2022 AC3，弹层闸门的第三个消费面）：
/// 对话框开着按 `[` 不翻页（页码是用户可见的行集结果），取消关窗后同键翻页
/// 生效。删掉 danger 对话框的抑制上报或 keymap 的闸门检查任一环本测红。
#[test]
fn suppression_gate_blocks_pagination_key_while_danger_dialog_open() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("suppress-danger");
    seed_expense_series(&dir, 150);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 150 条").unwrap();
    app.press_key("]").unwrap();
    app.wait_for("text:2026-02-19").unwrap();

    // 右键行 → 菜单 → 删除 → 危险确认对话框开（注册抑制的弹层）。
    app.right_click("text:2026-02-19").unwrap();
    app.choose_menu_item("删除").unwrap();
    app.wait_for("text:删除交易").unwrap();

    // 对话框开着 → `[` 不派发（第 2 页行集原样）。
    app.press_key("[").unwrap();
    app.settle();
    assert!(
        app.shows("2026-02-19") && !app.shows("2026-05-30"),
        "危险确认对话框开着时 `[` 不得翻页：{:?}",
        app.screen().texts()
    );

    // 取消关窗 → 同键翻页生效（第 1 页行集换血）。
    app.press("id:danger-cancel").unwrap();
    app.wait_for_gone("text:删除交易", DEFAULT_WAIT).unwrap();
    app.press_key("[").unwrap();
    app.wait_for("text:2026-05-30").unwrap();
    app.wait_for_gone("text:2026-02-19", DEFAULT_WAIT).unwrap();
}

/// 删除按交易计（issue #2022 × ADR-0138）：购买项订单逐项成行，任一购买项
/// 行右键删除的整笔交易——续行行文本离屏、订单级行离屏、库中该笔消失，
/// 留底支出不受影响（行 id 同为交易本体 id 的固定目标语义）。
#[test]
fn purchase_item_row_right_click_deletes_whole_transaction() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("delete-order");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
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
                amount_cents: 3_000,
                currency_code: "CNY".to_owned(),
                account_id: Some(account.clone()),
                to_account_id: None,
                funding_account_id: None,
                funding: Vec::new(),
                purchases: vec![
                    TransactionPurchaseInput {
                        name: "猫粮".to_owned(),
                        quantity: 2,
                        category_id: None,
                        unit_price_cents: None,
                    },
                    TransactionPurchaseInput {
                        name: "洗衣液".to_owned(),
                        quantity: 1,
                        category_id: None,
                        unit_price_cents: None,
                    },
                ],
                category_id: None,
                merchant_id: None,
                merchant_name: None,
                policy_id: None,
                refund_of_transaction_id: None,
                note: None,
                source_order_no: None,
                date: "2026-01-01".to_owned(),
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
        drop(conn);
        seed_transaction(
            &db,
            &account,
            TransactionKind::Expense,
            2_500,
            "2026-01-02",
            None,
            None,
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();

    // 「共 N 条」按交易计：订单 1 笔 + 留底 1 笔（购买项行不虚增计数）。
    app.wait_for("text:共 2 条").unwrap();

    // 右键购买项**续行**（非首行）→ 菜单 → 删除 → 确认。
    app.right_click("text:洗衣液 共 1 件").unwrap();
    app.choose_menu_item("删除").unwrap();
    app.wait_for("text:删除交易").unwrap();
    app.press("id:danger-confirm").unwrap();

    // 整笔离屏：订单级行与购买项行全消失，计数按交易减一。
    app.wait_for_gone("text:洗衣液 共 1 件", DEFAULT_WAIT)
        .unwrap();
    app.wait_for_gone("text:猫粮 共 2 件", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    assert!(
        app.shows("2026-01-02"),
        "留底支出不受影响：{:?}",
        app.screen().texts()
    );

    // 库态核对（不经 UI）：被删整笔软删生效，剩留底一笔。
    let dates = book_transaction_dates(&dir);
    assert_eq!(
        dates,
        vec!["2026-01-02".to_owned()],
        "库中剩一笔：{dates:?}"
    );
}

// ---------------------------------------------------------------------------
// 立桩验收自动化件（issue #2024）：① headless 冒烟首件（AC3，#2016 裁决
// §3.4 成立）；② ADR-0062 确定性性能数据集量级的计数型验证（AC2 可自动化
// 半边）；③ 搜索 × 期间未定义组合的实际行为观察钉（AC1 备料）。
// ---------------------------------------------------------------------------

/// 冒烟首件（issue #2024 AC3 / research doc §3.4）：一条最小复合旅程——起根
/// → 就绪 → 首屏计数（「共 N 条」）→ 翻页键可用。既有用例逐功能聚焦（首屏
/// 用例只看 1 笔首屏、翻页用例带边界幂等细节），本测是裁决的「UI 冒烟自动
/// 化首件」：以单条旅程证明应用能起、首屏数据可见、翻页链路活着；断言面
/// 刻意最小，功能级细节归各专项用例，不做第二遍。
#[test]
fn app_smoke_boots_shows_first_screen_count_and_pages_by_key() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("smoke");
    seed_expense_series(&dir, 150);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    // 就绪 + 首屏计数（100 行/页，150 笔 = 两页）。
    app.wait_for("text:共 150 条").unwrap();
    app.wait_for("text:2026-05-30").unwrap();

    // 翻页键可用：`]` 翻末页，行集换血（与分页条同一读路径）。
    app.press_key("]").unwrap();
    app.wait_for("text:2026-02-19").unwrap();
    app.wait_for_gone("text:2026-05-30", DEFAULT_WAIT).unwrap();
}

/// ADR-0062 确定性性能数据集生成器的 bin 路径（根包 tauri-app 的
/// `src/bin/ledger-perf`，`cargo test --workspace` / scripts/test.sh 泳道随
/// 构建一并产出）。跨包集成测试取不到 `CARGO_BIN_EXE_*`，按 workspace target
/// 目录约定定位（manifest 相对，编译期注入；执行器等价性守门禁测试运行期读
/// CARGO_* 环境变量，scripts/test-exec.ts 守门规则④）。
fn ledger_perf_bin() -> std::path::PathBuf {
    let exe =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/ledger-perf");
    assert!(
        exe.is_file(),
        "ledger-perf 二进制未构建（{}）：本测经 ADR-0062 生成器子进程造库，\
         先 `cargo build --bin ledger-perf -p tauri-app`（scripts/test.sh 泳道自带）",
        exe.display()
    );
    exe
}

/// 经 ADR-0062 生成器现场造一本确定性数据集库（复用唯一生成器，不另立第二
/// 套造数——ADR-0062 决策 1/4）：`--transactions` 缩量适配测试泳道耗时（同
/// 参数必出同库，账户/分类/商户/转账/退款/软删/购买项/投资/计划全画像都在），
/// 输出落产品形态目录（ledger.db + sync.db，与 nsh-app boot 的开库形态一致）。
/// 返回账本目录。
fn generate_perf_book(transactions: u64) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ledger-test-nsh-perf-{}-{}",
        transactions,
        new_uuid()
    ));
    fs::create_dir_all(&dir).unwrap();
    let out = dir.join("ledger.db");
    let status = std::process::Command::new(ledger_perf_bin())
        .args([
            "generate",
            "--transactions",
            &transactions.to_string(),
            "--out",
            &out.to_string_lossy(),
        ])
        .status()
        .unwrap();
    assert!(
        status.success(),
        "ledger-perf generate 失败（--transactions {transactions}）"
    );
    dir
}

/// 生成库的计数事实（测试侧期望值的独立真源）。
struct PerfBookFacts {
    /// 未删除交易总数（「共 N 条」应呈的 N）。
    total: i64,
    /// 页数（`assembly::total_pages` 口径 = total 对页径向上取整；PAGE_SIZE
    /// 住 crate 私有装配层，字面量与该单点同步维护）。
    pages: u32,
    /// 首页首行日期（数据集最新一笔，date DESC 首位）。
    newest_date: String,
    /// 第 2 页首行日期（翻页键一页后的落点）。
    second_page_first_date: String,
    /// 末页首行日期（数据集最旧一笔）。
    oldest_date: String,
    /// 首页末行日期（首页滚动到底的落点）。
    first_page_last_date: String,
}

/// 经域读路径取计数事实：与被测应用同一函数同一过滤
/// （`boot::transaction_page`，None 区间 = 「全部」默认态）——断言「UI == 域
/// 读」而非幻数（确定性数据集的软删数随参数变化，钉死值会在参数调整时误红）。
fn perf_book_facts(dir: &Path) -> PerfBookFacts {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let read_page = |no: u32| {
        let result = nsh_app::boot::transaction_page(&db, no, None).unwrap();
        assert!(
            !result.items.is_empty(),
            "第 {no} 页应有行集（数据集量级下不应有空页）"
        );
        result
    };
    let first = read_page(1);
    // 100 = 装配层 PAGE_SIZE（100 行/页，#2018 起翻倍验证原生余量）。
    let pages = (first.total.max(0) as u64).div_ceil(100) as u32;
    let second = read_page(2);
    let last = read_page(pages);
    PerfBookFacts {
        total: first.total,
        pages,
        newest_date: first.items[0].date.clone(),
        second_page_first_date: second.items[0].date.clone(),
        oldest_date: last.items[0].date.clone(),
        first_page_last_date: first.items.last().unwrap().date.clone(),
    }
}

/// ADR-0062 数据集量级下的计数型验证（issue #2024 AC2 可自动化半边）：经唯
/// 一生成器造库 → NSH 起根 → 总数 / 页数 / 翻页落点 / 滚动落点与域读口径
/// 逐项一致。「Counts are deterministic; timings are not」（research doc
/// §1.5）：只断计数与落点，无任何时间断言；体感迟滞与滚动流畅度归真实窗口
/// 人工对表（docs/research/nsh-stake-acceptance-checklist.md）。生成器缩量
/// 到 2000 笔（约 20 页）适配测试泳道；50 万笔全量级的人工复跑命令见对表
/// 手册（同一生成器，只换 --transactions）。
#[test]
fn perf_dataset_book_counts_match_domain_read_across_pages_and_scroll() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = generate_perf_book(2_000);
    let facts = perf_book_facts(&dir);
    assert!(
        facts.pages >= 3,
        "数据集量级应至少三页（缩量参数失配时先查生成器）：total={} pages={}",
        facts.total,
        facts.pages
    );

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    // 总数一致（UI == 域读，非幻数）；首页首行 = 最新一笔落进视口。
    let total_text = format!("共 {} 条", facts.total);
    let total_selector = format!("text:{total_text}");
    let newest = format!("text:{}", facts.newest_date);
    app.wait_for(&total_selector).unwrap();
    app.wait_for(&newest).unwrap();

    // 翻页键一页 → 第 2 页首行落点一致，首页最新行离屏（offset 分页换血）。
    app.press_key("]").unwrap();
    let second = format!("text:{}", facts.second_page_first_date);
    app.wait_for(&second).unwrap();
    app.wait_for_gone(&newest, DEFAULT_WAIT).unwrap();

    // 页数一致：末页页码按钮存在且可点（页码按钮闭集即 total_pages 的用户
    // 可见投影——按钮不存在时 press 即红），末页首行 = 最旧一笔。
    let last_button = format!("role:button[{}]", facts.pages);
    app.press(&last_button).unwrap();
    let oldest = format!("text:{}", facts.oldest_date);
    app.wait_for(&oldest).unwrap();
    app.wait_for_gone(&second, DEFAULT_WAIT).unwrap();
    assert!(
        app.shows(&total_text),
        "翻页后总数不变：{:?}",
        app.screen().texts()
    );

    // 回首页滚动到底 → 首页末行（第 100 笔）可见、首行离屏。距离给足
    // （headless scroll 钳到滚动终点）；虚拟化大表在数据集量级下撑得起
    // 滚动本身就是计数面事实（滚动距离与行集体量一致才能到底）。
    app.press("role:button[1]").unwrap();
    app.wait_for(&newest).unwrap();
    app.scroll("id:tx-table", Points(1_000_000.0)).unwrap();
    let first_page_last = format!("text:{}", facts.first_page_last_date);
    app.wait_for(&first_page_last).unwrap();
    app.wait_for_gone(&newest, DEFAULT_WAIT).unwrap();
    assert!(
        app.shows(&total_text),
        "滚动后总数不变：{:?}",
        app.screen().texts()
    );
}

/// 搜索 × 期间的未定义组合——实际行为观察钉（issue #2024 AC1 备料）：集成
/// 分支合并时报告了两处未定义组合（搜索期间按 `,` / `.` 步进、搜索期间期间
/// 标签显示），本测把 headless 可复现的**现状行为**记录下来，不设计新语义；
/// #2015 对表以本测与真实窗口人工复核为凭。后续票若对组合语义作出裁决，
/// 应有意更新本测（它翻红 = 语义有意变化，不是回归）。
///
/// 记录到的现状（`[` `]` `,` `.` 皆 When::Auto 裸键，可编辑焦点闸与 Overlay
/// 抑制闸都先于派发）：
/// - 组合②（标签显示）：期间标签在搜索态照常显示且不重置——搜索只换行集
///   与计数词，不动期间快照；
/// - 期间过滤对搜索命中面**不生效**（`boot::transaction_search` 无区间参数，
///   `load` 按非空查询走搜索路径）：当月期间下搜索，上月备注照样命中；
/// - 组合①（搜索期间按 `,` / `.`）分焦点两相：焦点在搜索框（输入后默认态）
///   时，键被输入框吃掉（可编辑焦点闸不派发）——查询追加字符触发重搜，
///   期间步进不发生；焦点不在输入框（回车提交后 BlurAndSubmit 失焦——单行
///   输入回车 unfocus 是引擎缺省）时，键照常派发——期间快照步进、标签随之
///   变化，行集与命中数原样（搜索重读不携期间，唯一可观察变化是标签）；
///   逻辑点击不转移键盘焦点（headless press 是逻辑派发非物理命中），点击
///   步进器在搜索态同样只换标签不动行集（点击路径孪生）。
#[test]
fn search_and_period_combo_keys_pins_actual_behavior() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-period");
    seed_expense_on_date(&dir, &prev_month_mid().to_string(), 8_800);
    seed_expense_on_date(&dir, &today().to_string(), 2_500);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    app.wait_for("text:共 2 条").unwrap();

    // ---- 相 A：焦点在搜索框（输入后默认态，无游标）。----
    app.type_text("id:tx-search", "支出").unwrap();
    app.wait_for("text:命中 2 条").unwrap();
    // 组合②：期间标签（「选择期间」召唤态）照常显示且不重置。
    assert!(
        app.shows("选择期间"),
        "搜索期间期间标签应照常显示（现状 = 不重置）：{:?}",
        app.screen().texts()
    );
    // 组合①焦点在框：`,` 不派发（可编辑焦点闸）——字符落入查询触发重搜，
    // 期间标签与期间快照不动。
    app.press_key(",").unwrap();
    app.wait_for("text:命中 0 条").unwrap();
    assert_eq!(
        app.screen().input_text("id:tx-search").unwrap(),
        Some("支出,"),
        "焦点在搜索框时 `,` 应落进输入框（快捷键不派发）"
    );
    assert!(
        app.shows("选择期间"),
        "`,` 落框不改变期间标签：{:?}",
        app.screen().texts()
    );
    // 清空回列表态（全部）：后续在列表态重新进入当月，再入搜索态。
    app.clear("id:tx-search").unwrap();
    app.wait_for("text:共 2 条").unwrap();

    // ---- 相 B：游标当月 + 搜索态；焦点移出走回车提交失焦（下方 submit）。----
    let t = today();
    app.press("id:period-label").unwrap();
    app.wait_for("id:period-chip-month").unwrap();
    app.press("id:period-chip-month").unwrap();
    app.wait_for_gone("id:period-chip-month", DEFAULT_WAIT)
        .unwrap();
    app.wait_for(&month_label(t.year(), t.month())).unwrap();
    app.wait_for("text:共 1 条").unwrap();

    app.type_text("id:tx-search", "支出").unwrap();
    app.wait_for("text:命中 2 条").unwrap();
    // 期间过滤对搜索不生效：当月期间下，上月备注照样命中。
    app.wait_for("text:¥88").unwrap();
    app.wait_for("text:¥25").unwrap();
    // 组合②：当月标签照常显示，不因搜索重置。
    assert!(
        app.shows(&format!("{}年{}月", t.year(), t.month())),
        "搜索期间期间标签应照常显示（现状 = 不重置）：{:?}",
        app.screen().texts()
    );

    // 点击步进器 ‹（搜索态下步进只换标签不动行集——点击路径孪生；headless
    // 逻辑点击不转移键盘焦点，失焦走下一步的回车提交）。
    let p = prev_month_mid();
    app.press("role:button[上一个周期]").unwrap();
    app.wait_for(&month_label(p.year(), p.month())).unwrap();
    app.wait_for("text:命中 2 条").unwrap();
    app.wait_for("text:¥88").unwrap();
    app.wait_for("text:¥25").unwrap();

    // 回车提交（BlurAndSubmit：单行输入回车 unfocus，真实用户「回车搜索后
    // 焦点离开输入框」的同形）→ 同结果重搜，焦点离开输入框。
    app.submit("id:tx-search").unwrap();
    app.wait_for("text:命中 2 条").unwrap();

    // 组合①焦点在框外：`,` / `.` 照常派发——标签步进，行集与命中数原样。
    app.press_key(".").unwrap();
    app.wait_for(&month_label(t.year(), t.month())).unwrap();
    app.wait_for("text:命中 2 条").unwrap();
    app.wait_for("text:¥88").unwrap();
    app.wait_for("text:¥25").unwrap();

    app.press_key(",").unwrap();
    app.wait_for(&month_label(p.year(), p.month())).unwrap();
    app.wait_for("text:命中 2 条").unwrap();
    app.wait_for("text:¥88").unwrap();
    app.wait_for("text:¥25").unwrap();
    // 失焦后查询原样（`,` / `.` 不再落框——派发面与编辑面互斥的镜像）。
    assert_eq!(
        app.screen().input_text("id:tx-search").unwrap(),
        Some("支出"),
        "焦点在框外时 `,` / `.` 不落输入框"
    );
}
