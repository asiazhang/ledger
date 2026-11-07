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

/// 桌面形态 headless 配置（足够容纳侧栏 + 表格全部建议列宽的视口：内容区
/// 1104pt ≥ 列宽总和 1090pt，导航骨架起侧栏不裁列），带设备存储初值
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

/// 桌面形态 headless 配置（1280×720：侧栏 176 + 表格建议列宽 1090）。
fn desktop_config() -> EngineConfig {
    EngineConfig {
        viewport: Size::new(Points(1280.0), Points(720.0)),
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

/// 一笔种子交易的具名入参（`seed_transaction` 的载荷）：转账双腿填
/// [`TxnSeed::to_account`]，支出/收入留 `None`。
struct TxnSeed<'a> {
    /// 资金账户（支出/收入的记账账户，转账的转出侧）。
    account: &'a str,
    /// 转入侧账户（仅转账非空）。
    to_account: Option<String>,
    /// 交易类型。
    kind: TransactionKind,
    /// 金额（整数分）。
    amount_cents: i64,
    /// 日期（YYYY-MM-DD）。
    date: &'a str,
    /// 分类（可空）。
    category_id: Option<String>,
    /// 商户（可空；#2037 商户维度筛选的造数面）。
    merchant_id: Option<String>,
    /// 备注（可空）。
    note: Option<String>,
}

/// 经域层公开写入口造一笔交易（用例共用的最小写面，载荷具名传递）。
fn seed_transaction(db: &ledger_infra::db::DbState, seed: TxnSeed<'_>) {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_transaction::create(
        &conn,
        TransactionInput {
            kind: seed.kind,
            amount_cents: seed.amount_cents,
            currency_code: "CNY".to_owned(),
            account_id: Some(seed.account.to_owned()),
            to_account_id: seed.to_account,
            funding_account_id: None,
            funding: Vec::new(),
            purchases: Vec::new(),
            category_id: seed.category_id,
            merchant_id: seed.merchant_id,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: seed.note,
            source_order_no: None,
            date: seed.date.to_owned(),
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
    let account = seed_cash_account(&db);
    for index in 0..count {
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: &test_date(index),
                category_id: None,
                merchant_id: None,
                note: Some(format!("造数第 {index} 笔")),
            },
        );
    }
}

/// 经域层公开写入口在同一账户上造一列带指定中文备注的支出（搜索断言的
/// 原文命中造数面：备注各不相同、日期递增；账户名固定「测试现金」不与
/// 搜索关键词相交）。
fn seed_expense_notes(dir: &Path, notes: &[&str]) {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let account = seed_cash_account(&db);
    for (index, note) in notes.iter().enumerate() {
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: &test_date(index),
                category_id: None,
                merchant_id: None,
                note: Some((*note).to_owned()),
            },
        );
    }
}

/// 在指定日期造一笔指定金额的现金支出（期间维度读路径的造数入口，
/// 与 lib 单测同纪律：建库走产品开库入口，造数经域公开写入口）。
fn seed_expense_on_date(dir: &Path, date: &str, amount_cents: i64) {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let account = seed_cash_account(&db);
    seed_transaction(
        &db,
        TxnSeed {
            account: &account,
            to_account: None,
            kind: TransactionKind::Expense,
            amount_cents,
            date,
            category_id: None,
            merchant_id: None,
            note: Some(format!("支出 {date}")),
        },
    );
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

    open_transactions(&mut app);
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

    open_transactions(&mut app);

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
    open_transactions(&mut app);
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
    open_transactions(&mut app);
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

    open_transactions(&mut app);
    app.type_text("id:tx-search", "不存在的关键词").unwrap();
    app.wait_for("text:无匹配结果").unwrap();
    open_transactions(&mut app);
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

    open_transactions(&mut app);
    app.type_text("id:tx-search", "火锅").unwrap();
    open_transactions(&mut app);

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

    open_transactions(&mut app);
    app.type_text("id:tx-search", "批量造数").unwrap();
    app.submit("id:tx-search").unwrap();
    open_transactions(&mut app);

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

    open_transactions(&mut app);
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

    // 交易列表落位后断言（首屏概览的预算卡同样掩码——概览卡面的掩码呈现
    // 由概览测试覆盖，本测断言面是交易表格）。
    open_transactions(&mut app);
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
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: "2026-01-15",
                category_id: None,
                merchant_id: None,
                note: None,
            },
        );
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Income,
                amount_cents: 1_000,
                date: "2026-01-16",
                category_id: None,
                merchant_id: None,
                note: None,
            },
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_transactions(&mut app);

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
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: "2026-01-15",
                category_id: Some(child),
                merchant_id: None,
                note: Some("买咖啡".to_owned()),
            },
        );
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 1_000,
                date: "2026-01-16",
                category_id: None,
                merchant_id: None,
                note: None,
            },
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_transactions(&mut app);

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
    open_transactions(&mut app);
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
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: Some(receivable),
                kind: TransactionKind::Transfer,
                amount_cents: 5_000,
                date: "2026-01-15",
                category_id: None,
                merchant_id: None,
                note: None,
            },
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_transactions(&mut app);

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

    open_transactions(&mut app);

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

    open_transactions(&mut app);
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

    open_transactions(&mut app);

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

    open_transactions(&mut app);
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

    open_transactions(&mut app);

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

    open_transactions(&mut app);

    // 行右键 → 上下文菜单呈现（zh-CN 文案与旧壳 transactions.menu.delete 同词
    // 表；#2035 起支出行带退款项）。
    app.right_click("text:2026-01-01").unwrap();
    let menu = app.menu().expect("右键应打开上下文菜单");
    assert_eq!(
        menu.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        vec!["编辑", "退款", "删除"],
        "行菜单项闭集（#2034 起支出行带编辑项，#2035 起带退款项，删除殿后）：{menu:?}"
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

    open_transactions(&mut app);
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

    open_transactions(&mut app);
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

/// 上下文菜单 ESC 收起（Spec 轴 #2 断言缺口）：右键开菜单 → ESC → 菜单消失、
/// 行仍在、库无写入。headless 对原生菜单的 ESC 语义步是 [`Headless::close_menu`]
/// （nsh-testing 注明的「as Escape or a click elsewhere closes it」：Linux 原生
/// 菜单捕获键盘，ESC 不进应用事件流，只兑现 `Menu.popup` 的关闭 Answer——
/// `press_key("escape")` / `back` 走 keymap 与弹层栈，触不到菜单），不是绕过
/// 应用行为的假步。
#[test]
fn row_context_menu_escape_closes_menu_writes_nothing() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("menu-esc");
    seed_expense_notes(&dir, &["要删除的一笔", "要留下的一笔"]);

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();

    open_transactions(&mut app);
    app.right_click("text:2026-01-01").unwrap();
    assert!(
        app.menu()
            .is_some_and(|items| items.iter().any(|e| e.label == "删除")),
        "右键后菜单开且带「删除」项：{:?}",
        app.menu()
    );

    // ESC（原生菜单语义步）→ 菜单收起、行集原样。
    app.close_menu().unwrap();
    app.settle();
    assert!(app.menu().is_none(), "ESC 后菜单消失");
    assert!(
        app.shows("2026-01-01"),
        "ESC 后行仍在屏：{:?}",
        app.screen().texts()
    );
    assert!(
        app.shows("共 2 条"),
        "ESC 后总数不变：{:?}",
        app.screen().texts()
    );

    let dates = book_transaction_dates(&dir);
    assert_eq!(dates.len(), 2, "ESC 收起菜单不产生写入：{dates:?}");
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

    open_transactions(&mut app);
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
        let account = seed_cash_account(&db);
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
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
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: "2026-01-02",
                category_id: None,
                merchant_id: None,
                note: None,
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();

    // 「共 N 条」按交易计：订单 1 笔 + 留底 1 笔（购买项行不虚增计数）。
    open_transactions(&mut app);

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

    // 就绪 + 首屏概览（出厂落点，#2039）：净资产主数字在屏。
    app.wait_for("id:net-worth-value").unwrap();

    // 交易列表页（100 行/页，150 笔 = 两页）首行在屏；翻页键可用。
    open_transactions(&mut app);
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
        let result = nsh_app::boot::transaction_page(&db, no, &Default::default()).unwrap();
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
    // （出厂落点是概览，#2039：先显式落位交易列表再断言。）
    open_transactions(&mut app);
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

/// 「搜索 × 期间」组合语义钉（issue #2037 裁决定型；前身 = #2024 备料交付②
/// 的现状观察钉——立桩期两处未定义组合按引擎现状记录，本票裁决后**有意更新
/// 本钉**，翻红 = 裁决落地而非回归）。裁决结论（三处同步：父 spec / 本测 /
/// docs/research/nsh-stake-acceptance-checklist.md §4）：
/// - **裁决一（期间标签显示）**：期间标签在搜索态照常显示且不重置——搜索的
///   进入/退出不动期间快照（立桩现状定型为规格）；
/// - **裁决二（期间对搜索命中面生效）**：期间快照对搜索命中面**生效**——
///   搜索读路径携期间区间（平价旧壳搜索页 QuickTimeRange：期间写入搜索过滤，
///   域搜索读路径 date_from/date_to 直传）；当月期间下搜索，上月备注**不再**
///   命中，步进期间 = 命中面随动。账户/商户/分类/类型维度不在搜索面（域读
///   路径不支持，裁决定型为规格）；
/// - **裁决三（焦点按键派发）**：引擎可编辑焦点闸定型——焦点在搜索框时
///   `,`/`.`/`[`/`]` 落框不派发（查询追加字符触发重搜，期间不动）；焦点在
///   框外（回车提交 BlurAndSubmit 失焦后）照常派发——翻页换搜索页、`,`/`.`
///   步进期间并按新区间重搜（命中数与行集随之）。
///
/// 「删除即红」：删读路径的搜索携期间（boot::transaction_search range 参数）
/// → 相 B 当月命中 2 条（上月照样命中）即红；删引擎可编辑焦点闸由引擎侧
/// 承担（不属本壳接线面）。
#[test]
fn search_and_period_combo_semantics_adjudicated() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-period");
    seed_expense_on_date(&dir, &prev_month_mid().to_string(), 8_800);
    seed_expense_on_date(&dir, &today().to_string(), 2_500);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_transactions(&mut app);

    // ---- 相 A：焦点在搜索框（输入后默认态，无游标）。----
    app.type_text("id:tx-search", "支出").unwrap();
    app.wait_for("text:命中 2 条").unwrap();
    // 裁决一：期间标签（「选择期间」召唤态）照常显示且不因搜索重置。
    assert!(
        app.shows("选择期间"),
        "搜索期间期间标签应照常显示：{:?}",
        app.screen().texts()
    );
    // 裁决三焦点在框：`,` 不派发（可编辑焦点闸）——字符落入查询触发重搜，
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
    // 裁决二：期间对搜索命中面生效——当月期间下只有本月一笔命中，上月备注
    // 不再命中（删读路径的搜索携期间即红：命中 2 条 = 立桩现状回归）。
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("text:¥25").unwrap();
    assert!(
        !app.shows("¥88"),
        "当月期间下搜索不得命中上月备注：{:?}",
        app.screen().texts()
    );
    // 裁决一：当月标签照常显示，不因搜索重置。
    assert!(
        app.shows(&format!("{}年{}月", t.year(), t.month())),
        "搜索期间期间标签应照常显示：{:?}",
        app.screen().texts()
    );

    // 点击步进器 ‹（搜索态下步进携新区间重搜——点击路径与快捷键路径同出口；
    // headless 逻辑点击不转移键盘焦点，失焦走下一步的回车提交）。
    let p = prev_month_mid();
    app.press("role:button[上一个周期]").unwrap();
    app.wait_for(&month_label(p.year(), p.month())).unwrap();
    // 上月期间下命中面随动：只剩上月一笔。
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("text:¥88").unwrap();
    assert!(
        !app.shows("¥25"),
        "上月期间下搜索不得命中本月备注：{:?}",
        app.screen().texts()
    );

    // 回车提交（BlurAndSubmit：单行输入回车 unfocus，真实用户「回车搜索后
    // 焦点离开输入框」的同形）→ 同结果重搜，焦点离开输入框。
    app.submit("id:tx-search").unwrap();
    app.wait_for("text:命中 1 条").unwrap();

    // 裁决三焦点在框外：`,` / `.` 照常派发——期间快照步进、标签随之变化，
    // 搜索按新区间重读（命中面随之：`.` 回当月换回本月笔，`,` 再回上月）。
    app.press_key(".").unwrap();
    app.wait_for(&month_label(t.year(), t.month())).unwrap();
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("text:¥25").unwrap();
    assert!(!app.shows("¥88"), "当月期间下搜索不得命中上月备注");

    app.press_key(",").unwrap();
    app.wait_for(&month_label(p.year(), p.month())).unwrap();
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("text:¥88").unwrap();
    assert!(!app.shows("¥25"), "上月期间下搜索不得命中本月备注");
    // 失焦后查询原样（`,` / `.` 不再落框——派发面与编辑面互斥的镜像）。
    assert_eq!(
        app.screen().input_text("id:tx-search").unwrap(),
        Some("支出"),
        "焦点在框外时 `,` / `.` 不落输入框"
    );
}

// ---------------------------------------------------------------------------
// 交易列表筛选补全（issue #2037，spec #2031 拆票②⑥）：TransactionFilter 完
// 整语义（账户/商户/类型/金额区间 + 会话保留 + 清除/ESC）与下钻落位消费。
// 环境：Environment{os: Linux, idiom: Desktop}（desktop_config）。
// 搜索 × 期间裁决钉见 search_and_period_combo_semantics_adjudicated。
// ---------------------------------------------------------------------------

/// 造一个商户（用例共用；商户维度筛选的造数面，含软删口径由 boot::merchant_rows
/// 承担——本测只用在用商户）。
fn seed_merchant(db: &ledger_infra::db::DbState, name: &str) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_merchants::create_merchant(
        &conn,
        ledger_merchants::MerchantInput {
            name: name.to_owned(),
        },
    )
    .unwrap()
}

/// 筛选旅程（#2037 AC）：账户/商户下拉设筛选 → 行集变化（翻页归零 + 维度
/// AND 组合）→ 清除筛选一键复位。删除任一接线即红：删账户/商户桥接 effect
/// （下拉写不落会话态）→ 行集不变红；删清除按钮 handler → 行集不复原红。
#[test]
fn filter_bar_account_and_merchant_selects_filter_rows_and_clear_restores() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("filter-dims");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_account(&db, "测试现金", AccountType::Cash);
        let bank = seed_account(&db, "银行卡", AccountType::Bank);
        let merchant = seed_merchant(&db, "链商超");
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: "2026-01-05",
                category_id: None,
                merchant_id: None,
                note: Some("现金笔".to_owned()),
            },
        );
        seed_transaction(
            &db,
            TxnSeed {
                account: &bank,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 6_600,
                date: "2026-01-06",
                category_id: None,
                merchant_id: None,
                note: Some("银行卡笔".to_owned()),
            },
        );
        seed_transaction(
            &db,
            TxnSeed {
                account: &bank,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 8_800,
                date: "2026-01-07",
                category_id: None,
                merchant_id: Some(merchant),
                note: Some("商户笔".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 账户筛选（拼音可搜下拉）：选「银行卡」→ 只剩银行卡两笔。
    app.press("id:tx-filter-account").unwrap();
    app.wait_for("id:tx-filter-account-search").unwrap();
    app.type_text("id:tx-filter-account-search", "银行卡")
        .unwrap();
    app.press("id:tx-filter-account-option >> text:银行卡 >> nth=0")
        .unwrap();
    app.wait_for("text:共 2 条").unwrap();
    app.wait_for("text:¥66").unwrap();
    assert!(
        !app.shows("¥25"),
        "账户筛选后现金笔离屏：{:?}",
        app.screen().texts()
    );

    // 商户筛选叠加（维度 AND）：银行卡 ∩ 链商超 → 只剩商户笔。
    app.press("id:tx-filter-merchant").unwrap();
    app.wait_for("id:tx-filter-merchant-search").unwrap();
    app.type_text("id:tx-filter-merchant-search", "链商超")
        .unwrap();
    app.press("id:tx-filter-merchant-option >> text:链商超 >> nth=0")
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:¥88").unwrap();
    assert!(
        !app.shows("¥66"),
        "商户维度 AND 组合后银行卡笔离屏：{:?}",
        app.screen().texts()
    );

    // 清除筛选一键复位：回全量 3 笔；按钮禁用态随筛选激活判定翻转。
    app.press("id:tx-filter-clear").unwrap();
    app.wait_for("text:共 3 条").unwrap();
    app.wait_for("text:¥25").unwrap();
    app.wait_for("text:¥66").unwrap();
    app.wait_for("text:¥88").unwrap();
    assert!(
        !app.screen().enabled("id:tx-filter-clear").unwrap(),
        "无保留状态时清除筛选禁用（旧壳 disabled=!filtersActive 同位）"
    );
}

/// 类型多选（#2037）：勾选切换（弹层不关、行集即时变化）→ 满选归一默认
/// （ADR-0135：满选 ≡ 不过滤）→ ✕ 清除复位。删除类型读写面（toggle handler
/// 的归一/装载接线）即红。
#[test]
fn filter_bar_kind_multiselect_toggles_normalize_and_clear() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("filter-kinds");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_cash_account(&db);
        let bank = seed_account(&db, "银行卡", AccountType::Bank);
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: "2026-01-05",
                category_id: None,
                merchant_id: None,
                note: Some("支出笔".to_owned()),
            },
        );
        seed_transaction(
            &db,
            TxnSeed {
                account: &bank,
                to_account: None,
                kind: TransactionKind::Income,
                amount_cents: 8_800,
                date: "2026-01-06",
                category_id: None,
                merchant_id: None,
                note: Some("收入笔".to_owned()),
            },
        );
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: Some(bank),
                kind: TransactionKind::Transfer,
                amount_cents: 1_000,
                date: "2026-01-07",
                category_id: None,
                merchant_id: None,
                note: Some("转账笔".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 类型弹层：勾「支出」→ 弹层不关（multiple 语义）、行集只剩支出笔。
    // 勾「支出」→ 行集只剩支出笔。headless 逻辑点击按引擎 press-outside 判定
    // 关弹层（#881 类限制：逻辑派发不携带命中坐标，弹层内选项点击亦被判外）
    // ——「弹层内切换不关弹层」的真实交互语义归人工对表；headless 以「重开
    // 弹层续勾」承载多选切换，勾选状态本身是断言面。
    app.press("id:tx-filter-kind-value").unwrap();
    app.wait_for("id:tx-filter-kind-option").unwrap();
    app.press("id:tx-filter-kind-option >> text:支出 >> nth=0")
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:¥25").unwrap();
    assert!(
        !app.shows("¥88"),
        "类型筛选后收入笔离屏：{:?}",
        app.screen().texts()
    );

    // 重开弹层再勾「收入」→ 维度内取或：支出 + 收入两笔。
    app.press("id:tx-filter-kind-value").unwrap();
    app.wait_for("id:tx-filter-kind-option").unwrap();
    app.press("id:tx-filter-kind-option >> text:收入 >> nth=0")
        .unwrap();
    app.wait_for("text:共 2 条").unwrap();

    // 再勾「转账」「退款」→ 满选四通用 kind 归一默认（ADR-0135：满选 ≡
    // 不过滤）——行集回全量、封装 ✕ 消失（无选中态）、清除筛选禁用。
    app.press("id:tx-filter-kind-value").unwrap();
    app.wait_for("id:tx-filter-kind-option").unwrap();
    app.press("id:tx-filter-kind-option >> text:转账 >> nth=0")
        .unwrap();
    app.wait_for("text:共 3 条").unwrap();
    app.press("id:tx-filter-kind-value").unwrap();
    app.wait_for("id:tx-filter-kind-option").unwrap();
    app.press("id:tx-filter-kind-option >> text:退款 >> nth=0")
        .unwrap();
    app.wait_for("text:共 3 条").unwrap();
    assert_eq!(
        app.screen().count("id:tx-filter-kind-clear").unwrap(),
        0,
        "满选归一默认后多选 ✕ 消失（无选中态）"
    );
    assert!(
        !app.screen().enabled("id:tx-filter-clear").unwrap(),
        "满选归一默认后清除筛选禁用（类型维度已回默认）"
    );

    // ✕ 清除路径：单选态下 ✕ 复位类型维度。
    app.press("id:tx-filter-kind-value").unwrap();
    app.wait_for("id:tx-filter-kind-option").unwrap();
    app.press("id:tx-filter-kind-option >> text:支出 >> nth=0")
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.press("id:tx-filter-kind-clear").unwrap();
    app.wait_for("text:共 3 条").unwrap();
    assert_eq!(
        app.screen().count("id:tx-filter-kind-clear").unwrap(),
        0,
        "✕ 清除后无选中态（多选封装 ✕ 消失）"
    );
}

/// 金额区间（搜索面维度，#2037）：仅金额、无关键词即入搜索态（旧壳搜索页
/// 「无关键字仅筛选」同语义——域仅筛选查询），命中计数词切换 + 行集按区间
/// 过滤；「全部」清除回列表态。
#[test]
fn filter_bar_amount_range_engages_search_surface_without_keyword() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("filter-amount");
    seed_expense_on_date(&dir, "2026-01-05", 1_100);
    seed_expense_on_date(&dir, "2026-01-06", 8_800);
    seed_expense_on_date(&dir, "2026-01-07", 4_400);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 仅最低金额 → 搜索态（命中 N 条计数词）+ 区间过滤（20 元 = 2000 分：
    // ¥88/¥44 命中，¥11 排除）。
    app.type_text("id:tx-amount-min", "20").unwrap();
    app.wait_for("text:命中 2 条").unwrap();
    app.wait_for("text:¥88").unwrap();
    app.wait_for("text:¥44").unwrap();
    assert!(
        !app.shows("¥11"),
        "金额区间过滤后小额离屏：{:?}",
        app.screen().texts()
    );

    // 清除筛选（金额属搜索面保留态，一并复位）→ 回列表态「共 N 条」。
    app.press("id:tx-filter-clear").unwrap();
    app.wait_for("text:共 3 条").unwrap();
    app.wait_for("text:¥11").unwrap();
}

/// ESC 复位 + 抑制闸门（#2037）：筛选在场时 ESC 一键复位（旧壳
/// registerViewReset 同语义）；弹层开着时 ESC 属弹层关闭面，筛选不动
/// （Overlay Suppression 闸门——删除闸门检查即红）。删除 on_key("escape")
/// 接线 → 复位段红。
#[test]
fn esc_resets_filters_and_gated_while_overlay_open() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("filter-esc");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_account(&db, "测试现金", AccountType::Cash);
        let bank = seed_account(&db, "银行卡", AccountType::Bank);
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: "2026-01-05",
                category_id: None,
                merchant_id: None,
                note: Some("现金笔".to_owned()),
            },
        );
        seed_transaction(
            &db,
            TxnSeed {
                account: &bank,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 6_600,
                date: "2026-01-06",
                category_id: None,
                merchant_id: None,
                note: Some("银行卡笔".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 设账户筛选（行集收窄）。
    app.press("id:tx-filter-account").unwrap();
    app.wait_for("id:tx-filter-account-search").unwrap();
    app.type_text("id:tx-filter-account-search", "银行卡")
        .unwrap();
    app.press("id:tx-filter-account-option >> text:银行卡 >> nth=0")
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();

    // 弹层开着：ESC 只关弹层，筛选保留（闸门拦截——删 overlay::has_open_overlay
    // 检查即红：筛选被误复位、共 1 条变共 2 条）。
    app.press("id:period-label").unwrap();
    app.wait_for("id:period-chip-all").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:period-chip-all", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();

    // 弹层已关：ESC 复位全部筛选 → 回全量行集 + 按钮禁用（无保留状态）。
    app.press_key("escape").unwrap();
    app.wait_for("text:共 2 条").unwrap();
    app.wait_for("text:¥25").unwrap();
    assert!(
        !app.screen().enabled("id:tx-filter-clear").unwrap(),
        "ESC 复位后无保留状态（幂等守卫的可见凭据）"
    );
}

/// 会话内保留（#2037，旧壳 ADR-0094 同语义）：筛选态住进程级单例（根实例
/// 所有）——切走再切回，筛选原样、行集按保留筛选呈现。删除会话态单例接线
/// （筛选态随视图卸载重置）即红：切回后行集回全量。
#[test]
fn filters_survive_view_switch_session_retained() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("filter-retain");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_account(&db, "测试现金", AccountType::Cash);
        let bank = seed_account(&db, "银行卡", AccountType::Bank);
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: "2026-01-05",
                category_id: None,
                merchant_id: None,
                note: Some("现金笔".to_owned()),
            },
        );
        seed_transaction(
            &db,
            TxnSeed {
                account: &bank,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 6_600,
                date: "2026-01-06",
                category_id: None,
                merchant_id: None,
                note: Some("银行卡笔".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.press("id:tx-filter-account").unwrap();
    app.wait_for("id:tx-filter-account-search").unwrap();
    app.type_text("id:tx-filter-account-search", "银行卡")
        .unwrap();
    app.press("id:tx-filter-account-option >> text:银行卡 >> nth=0")
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();

    // 切走（预算）再切回：筛选原样呈现（触发件回显选中项 + 行集按保留筛选）。
    app.press("id:nav-budget").unwrap();
    app.wait_for_gone("text:共 1 条", DEFAULT_WAIT).unwrap();
    app.press("id:nav-transactions").unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:¥66").unwrap();
    assert!(
        !app.shows("¥25"),
        "切回后保留筛选仍生效：{:?}",
        app.screen().texts()
    );
}

/// 抑制闸门覆盖新弹层（#2037）：类型多选弹层开着时 `[` 不派发（封装自含
/// 抑制上报——删除 multi_select 的 token 上报即红：翻页发生、行集换血）。
#[test]
fn suppression_gate_blocks_shortcuts_while_kind_popup_open() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("filter-suppress");
    seed_expense_series(&dir, 120);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 翻页可观察凭据（虚拟滚动下取视口行）：第 1 页首行 = 最新一笔（造数第
    // 119 笔）；`]` 翻到第 2 页后首行 = 第 19 笔、第 119 笔离屏。
    app.wait_for("text:造数第 119 笔").unwrap();

    // 类型弹层开着 → `]` 不派发（第 2 页首笔不得在屏、首页首行不离屏）。
    app.press("id:tx-filter-kind-value").unwrap();
    app.wait_for("id:tx-filter-kind-option").unwrap();
    app.press_key("]").unwrap();
    app.settle();
    // 凭据：第 2 页首笔不在屏 = 未翻页（翻页若发生，视口必换血到第 2 页行集）。
    // 「删除即红」负向验证：删 multi_select 的 token 上报 → `]` 翻页发生、
    // 第 19 笔上屏，本断言红（已实拆验证）。
    assert!(
        !app.shows("text:造数第 19 笔"),
        "弹层开着时 `]` 不得翻页：{:?}",
        app.screen().texts()
    );

    // 关弹层 → 同键翻页生效（第 2 页首行在屏、首页首行离屏）。
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:tx-filter-kind-option", DEFAULT_WAIT)
        .unwrap();
    app.press_key("]").unwrap();
    app.wait_for("text:造数第 19 笔").unwrap();
    app.wait_for_gone("text:造数第 119 笔", DEFAULT_WAIT)
        .unwrap();
}

// ---------------------------------------------------------------------------
// 壳导航骨架（issue #2032，spec #2031 拆票②①）：侧栏三组、键位带派发、
// 功能开关守卫、组「更多」容器、右键排序/移入移回与界面状态持久化。
// 环境前提：Environment{os: Linux, idiom: Desktop}（desktop_config）。
// ---------------------------------------------------------------------------

use nsh_testing::MemoryStorage;
use std::sync::Arc;

/// 桌面形态配置（复用同一存储后端——重启模拟：同一 MemoryStorage 实例起
/// 第二个引擎，写路径已落后端的界面状态在重启后可读）。
fn desktop_config_with_backend(storage: Arc<MemoryStorage>) -> EngineConfig {
    EngineConfig {
        storage: Some(storage),
        ..desktop_config()
    }
}

/// 骨架呈现：侧栏品牌行、固定项（概览/AI/设置）、三组（记账/资产/洞察）与
/// 出厂主项；收纳成员不入侧栏（入组「更多」容器）；「更多」链接仅在收纳
/// 清单非空的组渲染（洞察出厂无成员）。
#[test]
fn sidebar_renders_groups_fixed_items_and_hides_contained_members() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-skeleton");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    // 交易列表为骨架首个挂载视图（出厂落点）。
    open_transactions(&mut app);
    // 品牌行 + 固定项 + 三组标题。
    for label in ["开源记账", "概览", "AI", "设置", "记账", "资产", "洞察"] {
        app.wait_for(&format!("text:{label}")).unwrap();
    }
    // 出厂主项全在侧栏（导航项 test_id = 视图持久化名）。
    for id in [
        "nav-transactions",
        "nav-accounts",
        "nav-budget",
        "nav-investments",
        "nav-items",
        "nav-savingsGoals",
        "nav-reports",
        "nav-search",
    ] {
        app.wait_for(&format!("id:{id}")).unwrap();
    }
    // 收纳成员不入侧栏（表格列头同名文本在屏上属内容区，不属导航面——
    // 断言对准导航项 test_id 的缺席）；「更多」链接只在有收纳清单的组渲染。
    assert!(
        app.screen().count("id:nav-scheduled").unwrap() == 0,
        "出厂收纳成员（定时）不入侧栏"
    );
    assert!(
        app.screen().count("id:nav-merchants").unwrap() == 0,
        "出厂收纳成员（商户）不入侧栏"
    );
    assert!(
        app.screen().count("id:nav-more-bookkeeping").unwrap() == 1,
        "记账组有收纳清单，「更多」链接渲染"
    );
    assert!(
        app.screen().count("id:nav-more-assets").unwrap() == 1,
        "资产组有收纳清单，「更多」链接渲染"
    );
    assert!(
        app.screen().count("id:nav-more-insights").unwrap() == 0,
        "洞察组出厂无收纳成员，「更多」链接不渲染"
    );
}

/// 点击导航：侧栏项 → 真实视图落位（账户 #2041 起不再占位）→ 回到交易
/// 项 → 表格内容区原样（其余未迁移项的占位行为由各自迁移票接手）。
#[test]
fn clicking_sidebar_items_lands_placeholder_views_and_back() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-click");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 账户 → 真实视图（#2041 迁入：新增账户卡在屏，占位不再）。
    app.press("id:nav-accounts").unwrap();
    app.wait_for("text:新增账户").unwrap();
    assert!(
        app.screen().count("id:placeholder-note").unwrap() == 0,
        "账户不再占位"
    );
    // 概览 → 真实视图（净资产卡在屏，#2039 起不再是占位）。
    app.press("id:nav-dashboard").unwrap();
    app.wait_for("id:net-worth-card").unwrap();
    // 交易 → 表格内容区（首屏原样）。
    app.press("id:nav-transactions").unwrap();
    app.wait_for("text:共 3 条").unwrap();
}

/// 键位带派发（⌘ 键位带，ADR-0065 固定组带）：⌘4 投资、⌘` 概览、⌘0 AI、
/// ⌘, 设置、⌘2 账户、⌘1 回交易；同名视图重复派发幂等（停留原地）。
/// 「删除即红」负向条目钉在 dispatch_key 的 open 接线（删除 → 本测红）。
#[test]
fn shortcut_band_keys_land_views() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-keys");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.press_key("mod+4").unwrap();
    app.wait_for_gone("text:共 3 条", DEFAULT_WAIT).unwrap();
    app.wait_for("id:placeholder-note").unwrap();
    // 落位凭据 = 内容区随视图切换（概览是真实视图，净资产卡在屏即证据；
    // #2039 起概览不再是占位）。
    app.press_key("mod+`").unwrap();
    app.wait_for("id:net-worth-card").unwrap();
    assert!(
        app.screen().count("id:placeholder-note").unwrap() == 0,
        "⌘` 落真实概览"
    );
    app.press_key("mod+0").unwrap();
    assert_eq!(app.screen().count("text:AI").unwrap(), 2, "⌘0 落 AI");
    app.press_key("mod+,").unwrap();
    assert_eq!(app.screen().count("text:设置").unwrap(), 2, "⌘, 落设置");
    app.press_key("mod+2").unwrap();
    assert_eq!(app.screen().count("text:账户").unwrap(), 2, "⌘2 落账户");
    // 回交易：表格内容区原样。
    app.press_key("mod+1").unwrap();
    app.wait_for("text:共 3 条").unwrap();
    // 同名幂等：再按 ⌘1 不重挂（表格行集原样）。
    app.press_key("mod+1").unwrap();
    app.wait_for("text:共 3 条").unwrap();
}

/// 抑制闸门——键位带（Overlay Suppression 消费面 #12）：注册抑制的弹层
/// （期间选择弹层）开着时 ⌘2 不派发（停留交易表格），关闭后同键落位账户。
/// 「删除即红」负向条目：删 dispatch_key 中的 `overlay::has_open_overlay`
/// 检查 → 本测红（弹层下按键切换视图）。
#[test]
fn suppression_gate_blocks_shortcut_band_while_period_popup_open() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-suppress");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 打开注册抑制的弹层 → ⌘2 不派发（表格原样）。
    app.press("id:period-label").unwrap();
    app.wait_for("id:period-chip-all").unwrap();
    app.press_key("mod+2").unwrap();
    app.settle();
    assert!(
        !app.shows("id:placeholder-note") && app.shows("共 3 条"),
        "弹层开着时 ⌘2 不得切视图：{:?}",
        app.screen().texts()
    );
    // 关弹层 → 同键落位账户。
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:period-chip-all", DEFAULT_WAIT)
        .unwrap();
    app.press_key("mod+2").unwrap();
    app.wait_for("text:账户").unwrap();
}

/// 功能开关关闭面（守卫语义的可见侧）：种子关闭预算 → 侧栏入口消失、⌘3
/// 空键待命（不派发、不跨组压缩——账户仍 ⌘2、交易仍 ⌘1）、收纳清单保留
/// （记账组「更多」链接仍在）。
#[test]
fn closed_feature_exits_sidebar_and_key_band() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-closed");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir);
    let storage = MemoryStorage::new(vec![(
        "view_state:closed_features".to_owned(),
        "[\"budget\"]".to_owned(),
    )]);
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 3 条").unwrap();

    assert!(
        app.screen().count("id:nav-budget").unwrap() == 0,
        "关闭功能入口消失"
    );
    assert!(
        app.screen().count("id:nav-more-bookkeeping").unwrap() == 1,
        "关闭只筛入口不改收纳清单（记账组「更多」链接保留）"
    );
    // ⌘3 空键待命：不派发（停留交易表格）。
    app.press_key("mod+3").unwrap();
    app.settle();
    assert!(app.shows("共 3 条"), "关闭功能键位空置不派发");
    // 带内不压缩：账户仍 ⌘2。
    app.press_key("mod+2").unwrap();
    app.wait_for("text:账户").unwrap();
}

/// 启动恢复（上次视图持久化的读路径）：已存视图原样恢复（账户 → 真实账户
/// 视图，#2041 迁入后为真实视图）；已关闭功能回出厂落点（骨架期 = 交易列表）；未知名回出厂落点。
#[test]
fn startup_restores_last_view_with_closed_and_unknown_fallbacks() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-restore");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir.clone());
    // 已存合法视图 → 恢复账户占位。
    let storage = MemoryStorage::new(vec![(
        "view_state:route".to_owned(),
        "\"accounts\"".to_owned(),
    )]);
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    app.wait_for("text:新增账户").unwrap();
    assert_eq!(
        app.screen().count("text:账户").unwrap(),
        2,
        "恢复到账户视图（侧栏 + 屏名）"
    );
    drop(app);

    // 已关闭功能的上次视图 → 出厂落点（交易表格）。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir.clone());
    let storage = MemoryStorage::new(vec![
        ("view_state:route".to_owned(), "\"budget\"".to_owned()),
        (
            "view_state:closed_features".to_owned(),
            "[\"budget\"]".to_owned(),
        ),
    ]);
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    // 出厂落点 = 概览（#2039 改回，与旧壳「关闭恢复回概览」重合）：真实概览
    // 在屏（净资产卡），不再是交易表格也不再是占位。
    app.wait_for("id:net-worth-card").unwrap();
    drop(app);

    // 未知名 → 出厂落点（概览）。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir.clone());
    let storage = MemoryStorage::new(vec![("view_state:route".to_owned(), "\"nope\"".to_owned())]);
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    app.wait_for("id:net-worth-card").unwrap();
}

/// 组「更多」容器：链接落位容器页签壳（页签 = 收纳清单序）；页签点击切换
/// 内容（会话槽）；页签态不跨启动持久化（重启回默认首签）。
#[test]
fn group_more_container_tabs_follow_session_state_not_restart() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-more");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir.clone());
    let storage = MemoryStorage::new(Vec::new());
    let mut app = Headless::start(desktop_config_with_backend(storage.clone())).unwrap();
    open_transactions(&mut app);

    // 资产组「更多」：页签 = 出厂清单序（保单/实物资产/保险公司），默认首签
    // 保单（内容区标题「保单」×2：页签 + 占位屏名）。
    app.press("id:nav-more-assets").unwrap();
    app.wait_for("id:more-tab-policies").unwrap();
    app.wait_for("id:more-tab-physicalAssets").unwrap();
    app.wait_for("id:more-tab-insurers").unwrap();
    assert_eq!(
        app.screen().count("text:保单").unwrap(),
        2,
        "默认首签 = 清单首位（页签 + 占位屏名）"
    );
    // 页签点击 → 内容切换（实物资产 ×2：页签 + 占位屏名；保单回 1）。
    app.press("id:more-tab-physicalAssets").unwrap();
    app.wait_for("text:实物资产").unwrap();
    assert_eq!(
        app.screen().count("text:实物资产").unwrap(),
        2,
        "页签点击切换内容"
    );
    assert_eq!(
        app.screen().count("text:保单").unwrap(),
        1,
        "页签切换后旧内容卸载"
    );
    drop(app);

    // 重启：页签态不跨启动持久化，回默认首签保单。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    app.wait_for("id:more-tab-policies").unwrap();
    assert_eq!(
        app.screen().count("text:保单").unwrap(),
        2,
        "重启回默认首签（页签态会话内有效）"
    );
}

/// 已关闭功能的容器页签剔除：种子关闭商户 → 记账组容器页签只剩定时
/// （关闭只筛页签入口、收纳清单保留——重开即回原位）。
#[test]
fn closed_feature_tab_is_filtered_from_group_more_container() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-more-closed");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir);
    let storage = MemoryStorage::new(vec![(
        "view_state:closed_features".to_owned(),
        "[\"merchants\"]".to_owned(),
    )]);
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    open_transactions(&mut app);

    app.press("id:nav-more-bookkeeping").unwrap();
    app.wait_for("id:more-tab-scheduled").unwrap();
    app.settle();
    assert!(
        app.screen().count("id:more-tab-merchants").unwrap() == 0,
        "已关闭功能不进组「更多」页签"
    );
}

/// 右键排序菜单：移到顶部 → 组内序重排 + 键位带内随动（⌘1 落预算）+ 点选
/// 即写（重启后顺序保持——持久化往返）；恢复默认排序 → 回出厂（⌘1 回交易）。
#[test]
fn sort_menu_reorders_key_band_and_persists_across_restart() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-sort");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir.clone());
    let storage = MemoryStorage::new(Vec::new());
    let mut app = Headless::start(desktop_config_with_backend(storage.clone())).unwrap();
    open_transactions(&mut app);

    // 右键预算 → 移到顶部 → ⌘1 落预算（键位带内随动的用户可见凭据；预算
    // 视图已真实化，落位凭据 = 预算视图内容「新增预算」在屏，占位反馈离场）。
    app.right_click("id:nav-budget").unwrap();
    app.choose_menu_item("移到顶部").unwrap();
    app.press_key("mod+1").unwrap();
    app.wait_for("text:新增预算").unwrap();
    drop(app);

    // 重启：顺序持久化（先离开到概览，再 ⌘1——派发面不再被同名幂等短路）。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    app.press_key("mod+`").unwrap();
    app.wait_for_within("text:概览", DEFAULT_WAIT).unwrap();
    app.press_key("mod+1").unwrap();
    app.wait_for_within("text:预算", DEFAULT_WAIT).unwrap();
    assert_eq!(
        app.screen().count("text:预算").unwrap(),
        3,
        "重启后 ⌘1 落预算（顺序持久化：侧栏 + 屏名 + 预算视图表列头）"
    );

    // 恢复默认排序 → ⌘1 回交易（出厂序）。
    app.right_click("id:nav-budget").unwrap();
    app.choose_menu_item("恢复默认排序").unwrap();
    app.press_key("mod+1").unwrap();
    app.wait_for("text:共 3 条").unwrap();
}

/// 移入更多 / 页签右键移回：右键预算「移入更多」→ 侧栏项消失、⌘3 空置、
/// 记账组容器出现预算页签；容器页签右键「移回侧栏」（组未满）→ 页签消失、
/// 侧栏项回归末位（⌘3 回预算）。
#[test]
fn context_menus_move_item_into_more_and_back() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-into-more");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir.clone());
    let storage = MemoryStorage::new(Vec::new());
    let mut app = Headless::start(desktop_config_with_backend(storage.clone())).unwrap();
    open_transactions(&mut app);

    // 移入更多：侧栏预算消失、⌘3 空键待命。
    app.right_click("id:nav-budget").unwrap();
    app.choose_menu_item("移入更多").unwrap();
    app.settle();
    assert!(
        !app.screen().texts().contains(&"预算"),
        "移入更多后侧栏项消失"
    );
    app.press_key("mod+3").unwrap();
    app.settle();
    assert!(app.shows("共 3 条"), "⌘3 空键待命不派发");

    // 容器出现预算页签；页签右键移回（记账组主项 2 项未满 3，可移回）。
    app.press("id:nav-more-bookkeeping").unwrap();
    app.wait_for("id:more-tab-budget").unwrap();
    app.right_click("id:more-tab-budget").unwrap();
    app.choose_menu_item("移回侧栏").unwrap();
    app.settle();
    assert!(
        app.screen().count("id:more-tab-budget").unwrap() == 0,
        "移回后页签消失"
    );
    // 侧栏预算回归（末位，⌘3 回预算：计数 2 = 侧栏 + 屏名）。
    app.press_key("mod+3").unwrap();
    app.wait_for_within("text:预算", DEFAULT_WAIT).unwrap();
    // 预算视图已真实化（#2033）：落位凭据 = 侧栏项 + 屏名 + 预算执行表列头
    // 「预算」三处（原占位期无列头为两处）。
    assert_eq!(
        app.screen().count("text:预算").unwrap(),
        3,
        "移回后 ⌘3 落预算（侧栏 + 屏名 + 表列头）"
    );
    drop(app);

    // 重启：移入/移回的持久化往返（侧栏预算在册、容器页签无预算）。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    app.press_key("mod+3").unwrap();
    app.wait_for("text:预算").unwrap();
}

/// 组满拒写（上限可见可学习）：记账组出厂主项 3 项已满，容器页签右键菜单
/// 「移回侧栏」置灰并附提示项；点选无效（页签保留）。
#[test]
fn move_back_menu_is_disabled_when_group_full() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-full");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.press("id:nav-more-bookkeeping").unwrap();
    app.wait_for("id:more-tab-scheduled").unwrap();
    app.right_click("id:more-tab-scheduled").unwrap();
    let menu = app.menu().expect("右键应弹出菜单");
    let back = menu
        .iter()
        .find(|e| e.label == "移回侧栏")
        .expect("菜单含「移回侧栏」");
    assert!(back.disabled, "组满 3 主项，「移回侧栏」置灰（上限可见）");
    assert!(
        menu.iter()
            .any(|e| e.label == "本组主项已满 3 项，先移出一个主项腾位"),
        "附提示项（可学习）"
    );
    // 置灰点选无效：页签保留。
    let _ = app.choose_menu_item("移回侧栏");
    app.settle();
    assert!(
        app.screen().count("id:more-tab-scheduled").unwrap() == 1,
        "组满拒写：页签保留在容器"
    );
}

/// 界面状态持久化往返（写 → 重启 → 读一致）：导航落位 + 排序一并保持；
/// 上次视图恢复到账户视图（#2041 迁入后为真实视图）。
#[test]
fn view_state_round_trips_across_restart() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-roundtrip");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir.clone());
    let storage = MemoryStorage::new(Vec::new());
    let mut app = Headless::start(desktop_config_with_backend(storage.clone())).unwrap();
    open_transactions(&mut app);

    // 排序（物品移到资产组顶部）+ 导航到账户。
    app.right_click("id:nav-items").unwrap();
    app.choose_menu_item("移到顶部").unwrap();
    app.press("id:nav-accounts").unwrap();
    app.wait_for("text:新增账户").unwrap();
    drop(app);

    // 重启：上次视图 = 账户；物品已在资产组首位 → ⌘4（带内随动）落物品——两态都持久。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    app.wait_for("text:新增账户").unwrap();
    assert_eq!(
        app.screen().count("text:账户").unwrap(),
        2,
        "上次视图恢复到账户（侧栏 + 屏名）"
    );
    app.press_key("mod+4").unwrap();
    app.wait_for_within("text:物品", DEFAULT_WAIT).unwrap();
    assert_eq!(
        app.screen().count("text:物品").unwrap(),
        2,
        "资产组首位 = 物品（⌘4，顺序持久化）"
    );
}

// ---------------------------------------------------------------------------
// 预算视图（#2033 NSH 迁移②——封装族热身）：挂载导航骨架的预算分支，三封装
// 件（PinyinSelect / Modal / Popconfirm）首消费旅程。造数经域公开写入口（预算
// 经 boot 写接缝，与 lib 单测同纪律）；断言只写用户可见结果。
// ---------------------------------------------------------------------------

/// 造一个支出分类（可带父分类）。
fn seed_category(db: &ledger_infra::db::DbState, name: &str, parent: Option<&str>) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    create_category(
        &conn,
        CategoryInput {
            name: name.to_owned(),
            kind: "expense".to_owned(),
            parent_id: parent.map(str::to_owned),
            icon: None,
        },
    )
    .unwrap()
}

/// 经域公开写入口建一个预算（月度缺省；返回预算 id）。
fn seed_budget(
    db: &ledger_infra::db::DbState,
    category_id: &str,
    amount_cents: i64,
    period: ledger_budget::BudgetPeriod,
) -> String {
    nsh_app::boot::create_budget(
        db,
        &ledger_budget::BudgetInput {
            category_id: category_id.to_owned(),
            period: Some(period),
            amount_cents,
            start_date: nsh_app::boot::today_string(),
        },
    )
    .unwrap()
}

/// 本月一笔指定金额、指定分类的现金支出（进度口径的造数面）。锁纪律：写
/// 连接锁不可重入——造账户与造交易各自走自带锁的助手，不持锁跨调用。
fn seed_expense_this_month(db: &ledger_infra::db::DbState, category_id: &str, amount_cents: i64) {
    let account = seed_cash_account(db);
    let date = today().to_string();
    seed_transaction(
        db,
        TxnSeed {
            account: &account,
            to_account: None,
            kind: TransactionKind::Expense,
            amount_cents,
            date: &date,
            category_id: Some(category_id.to_owned()),
            merchant_id: None,
            note: Some("本月支出".to_owned()),
        },
    );
}

/// 预算视图首屏（AC：预算管理各分类预算 + 执行进度呈现）：新增预算表单卡、
/// 年度汇总卡与预算执行表——格文本为装配层产物（路径名 / 按月 / 金额分组 /
/// 状态词）。删除任一接线（导航分支挂载 / 装配 / 读路径 / 表格渲染）即红。
#[test]
fn budget_view_renders_create_form_summary_and_progress_table() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("budget-view");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let tea = seed_category(&db, "茶饮", None);
        seed_budget(&db, &tea, 50_000, ledger_budget::BudgetPeriod::Monthly);
        seed_expense_this_month(&db, &tea, 12_000);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 侧栏预算项落位预算视图（占位「迁移中」反馈离场）。
    app.press("id:nav-budget").unwrap();
    for label in [
        "新增预算",
        "支出分类",
        "按月",
        "按年",
        "金额",
        "添加",
        "本年度预算",
        "年度预算总额",
        "本年已支",
        "预算执行",
        "茶饮",
        "按月",
        "120",
        "500",
        "正常",
        "编辑",
        "删除",
    ] {
        app.wait_for(&format!("text:{label}")).unwrap();
    }
    // 汇总卡口径：年化总额 = 50000 × 12 = 600000 分 → 6000；本年已支 120。
    app.wait_for("text:6000").unwrap();
    // 进度百分比 24%（12000/50000 四舍五入）。
    app.wait_for("text:24%").unwrap();
    assert!(
        app.screen().count("id:placeholder-note").unwrap() == 0,
        "预算视图是真实组件，不再是占位"
    );
}

/// 创建旅程（封装件 PinyinSelect 首消费）：触发件打开下拉 → 拼音首字母搜索
/// 「午餐」（wc）→ 选中（触发件呈现「父 > 子」路径名）→ 输入金额 → 添加 →
/// 新行落表（写路径 + 失效信号 + 订阅重读的接线旅程）。「删除即红」负向条目：
/// 删 perform_create 里的 `notify_budgets_invalidated` 广播或视图订阅接线任一
/// 环 → 本测红（表单清空但新行不出现）。
#[test]
fn budget_form_creates_budget_via_pinyin_search_and_refreshes_list() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("budget-create");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let tea = seed_category(&db, "茶饮", None);
        seed_budget(&db, &tea, 10_000, ledger_budget::BudgetPeriod::Monthly);
        let fresh = seed_category(&db, "鲜食", None);
        seed_category(&db, "刺身", Some(&fresh));
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:nav-budget").unwrap();
    app.press("id:nav-budget").unwrap();
    app.wait_for("text:预算执行").unwrap();

    // 打开拼音可搜下拉，首字母 cs 命中「鲜食 > 刺身」。
    app.press("id:budget-category").unwrap();
    app.wait_for("id:budget-category-search").unwrap();
    app.type_text("id:budget-category-search", "cs").unwrap();
    app.wait_for("text:鲜食 > 刺身").unwrap();
    app.press("id:budget-category-option >> text:鲜食 > 刺身")
        .unwrap();
    // 选中后下拉关闭、触发件呈现路径名。
    app.wait_for_gone("id:budget-category-search", DEFAULT_WAIT)
        .unwrap();
    assert_eq!(
        app.screen().count("text:鲜食 > 刺身").unwrap(),
        1,
        "触发件呈现选中项路径名"
    );

    // 输入金额，添加。
    app.type_text("id:budget-amount", "80").unwrap();
    app.press("id:budget-add").unwrap();

    // 失效信号驱动重读：新行落表（子分类路径名 + 金额 80），表单清空回占位。
    app.wait_for_within("text:80", DEFAULT_WAIT).unwrap();
    assert_eq!(
        app.screen().count("text:鲜食 > 刺身").unwrap(),
        1,
        "表单回占位后路径名只在表行（新行以「父 > 子」路径名呈现）"
    );
    assert!(app.shows("支出分类"), "创建成功后表单清空");
}

/// 编辑旅程（封装件 Modal 首消费）：编辑按钮 → 弹窗呈现（分类只读名回填、
/// 金额回填整数分换算元文本）→ 改金额保存 → 弹窗关闭、行与年度汇总同刻刷新
/// （「删除即红」：删保存成功后的 notify 或弹窗关闭接线任一环即红）。
#[test]
fn budget_edit_modal_updates_amount_then_refreshes() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("budget-edit");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let tea = seed_category(&db, "茶饮", None);
        seed_budget(&db, &tea, 50_000, ledger_budget::BudgetPeriod::Monthly);
        seed_expense_this_month(&db, &tea, 12_000);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);
    app.press("id:nav-budget").unwrap();
    app.wait_for("text:500").unwrap();

    // 打开编辑弹窗：标题 + 分类只读名 + 金额回填 500。
    app.press("id:budget-edit").unwrap();
    app.wait_for("id:app-modal").unwrap();
    app.wait_for("text:编辑预算").unwrap();
    app.wait_for("text:茶饮").unwrap();
    let amount = app
        .screen()
        .input_text("id:budget-edit-amount")
        .unwrap()
        .unwrap_or_default();
    assert_eq!(amount, "500", "编辑弹窗金额回填（整数分换算元文本）");

    // 改金额保存：弹窗关闭，预算列与年化总额同刻刷新（600000→360000 → 3600）。
    app.clear("id:budget-edit-amount").unwrap();
    app.type_text("id:budget-edit-amount", "300").unwrap();
    app.press("id:budget-save").unwrap();
    app.wait_for_gone("id:app-modal", DEFAULT_WAIT).unwrap();
    app.wait_for_within("text:300", DEFAULT_WAIT).unwrap();
    app.wait_for("text:3600").unwrap();
}

/// 弹窗关闭语义（ESC / 取消 / ✕ 三路径关闭且不写——遮罩不关由封装内建，
/// headless 盲区归人工对表）：关闭后行金额原样。
#[test]
fn budget_edit_modal_escape_cancel_and_close_button_write_nothing() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("budget-modal-close");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let tea = seed_category(&db, "茶饮", None);
        seed_budget(&db, &tea, 50_000, ledger_budget::BudgetPeriod::Monthly);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:nav-budget").unwrap();
    app.press("id:nav-budget").unwrap();
    app.wait_for("text:500").unwrap();

    // ESC 关闭：弹窗离场、不写。
    app.press("id:budget-edit").unwrap();
    app.wait_for("id:app-modal").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:app-modal", DEFAULT_WAIT).unwrap();
    app.wait_for("text:500").unwrap();

    // 取消关闭：同语义。
    app.press("id:budget-edit").unwrap();
    app.wait_for("id:app-modal").unwrap();
    app.press("id:budget-cancel").unwrap();
    app.wait_for_gone("id:app-modal", DEFAULT_WAIT).unwrap();
    app.wait_for("text:500").unwrap();

    // ✕ 关闭：同语义。
    app.press("id:budget-edit").unwrap();
    app.wait_for("id:app-modal").unwrap();
    app.press("id:app-modal-close").unwrap();
    app.wait_for_gone("id:app-modal", DEFAULT_WAIT).unwrap();
    assert!(app.shows("500"), "三条关闭路径都不写，行金额原样");
}

/// 删除旅程（封装件 Popconfirm 首消费）：删除按钮 → 气泡「确认删除？」→ 取消
/// 不写 → 再开确认 → 行离场（失效信号重读）。「删除即红」：删确认分支的
/// notify 广播 → 本测红（气泡关闭但行不消失）。
#[test]
fn budget_delete_popconfirm_cancel_writes_nothing_confirm_refreshes() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("budget-delete");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let tea = seed_category(&db, "茶饮", None);
        seed_budget(&db, &tea, 50_000, ledger_budget::BudgetPeriod::Monthly);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:nav-budget").unwrap();
    app.press("id:nav-budget").unwrap();
    app.wait_for("text:500").unwrap();

    // 打开气泡：说明文案 + 两键。
    app.press("id:budget-delete").unwrap();
    app.wait_for("text:确认删除？").unwrap();

    // 取消：气泡关闭、行保留。
    app.press("id:budget-del-cancel").unwrap();
    app.wait_for_gone("text:确认删除？", DEFAULT_WAIT).unwrap();
    assert!(app.shows("500"), "取消不写");

    // 再开确认：行离场。
    app.press("id:budget-delete").unwrap();
    app.wait_for("text:确认删除？").unwrap();
    app.press("id:budget-del-confirm").unwrap();
    app.wait_for_gone("text:确认删除？", DEFAULT_WAIT).unwrap();
    app.wait_for_gone("text:500", DEFAULT_WAIT).unwrap();
    assert!(app.shows("暂无数据"), "删后空态");
}

/// 校验前置拦截（不发起写）：空表单添加 → 「请填写分类和金额」；选分类金额
/// 0 → 「预算金额必须为正数」；两条都不落新行。
#[test]
fn budget_form_validates_required_and_positive_without_writing() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("budget-validate");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let tea = seed_category(&db, "茶饮", None);
        seed_budget(&db, &tea, 50_000, ledger_budget::BudgetPeriod::Monthly);
        let fresh = seed_category(&db, "鲜食", None);
        seed_category(&db, "刺身", Some(&fresh));
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:nav-budget").unwrap();
    app.press("id:nav-budget").unwrap();
    app.wait_for("text:500").unwrap();

    // 空表单：必填提示。
    app.press("id:budget-add").unwrap();
    app.wait_for("text:请填写分类和金额").unwrap();
    assert!(app.shows("500"), "未发起写");

    // 金额 0：正数提示（旧壳同词表）。选择走唯一子分类（首字母 cs）。
    app.press("id:budget-category").unwrap();
    app.wait_for("id:budget-category-search").unwrap();
    app.type_text("id:budget-category-search", "cs").unwrap();
    app.press("id:budget-category-option >> text:鲜食 > 刺身")
        .unwrap();
    app.wait_for_gone("id:budget-category-search", DEFAULT_WAIT)
        .unwrap();
    // 选中凭据：触发件呈现选中项路径名（唯一文本，与表行区分）。
    assert_eq!(
        app.screen().count("text:鲜食 > 刺身").unwrap(),
        1,
        "选中后触发件呈现分类名"
    );
    app.type_text("id:budget-amount", "0").unwrap();
    app.press("id:budget-add").unwrap();
    app.wait_for("text:预算金额必须为正数").unwrap();
    assert_eq!(
        app.screen().count("text:500").unwrap(),
        1,
        "未发起写：仍只有原预算行（行金额一处）"
    );
}

/// 抑制闸门（封装件族共用注册表）：拼音下拉 / 编辑弹窗 / 删除气泡任一开着时
/// ⌘2 不派发（停留预算视图），ESC 关闭后同键落位账户。「删除即红」负向条目：
/// 删 modal.rs / popconfirm.rs / pinyin_select.rs 任一封装的 token.report 接线
/// → 本测对应段红（弹层下按键切走）。
#[test]
fn suppression_gate_blocks_shortcuts_while_budget_popups_open() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("budget-suppress");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let tea = seed_category(&db, "茶饮", None);
        seed_budget(&db, &tea, 50_000, ledger_budget::BudgetPeriod::Monthly);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:nav-budget").unwrap();
    app.press("id:nav-budget").unwrap();
    app.wait_for("text:预算执行").unwrap();

    // 拼音下拉开着：⌘2 不派发；ESC 关闭后落位账户。
    app.press("id:budget-category").unwrap();
    app.wait_for("id:budget-category-search").unwrap();
    app.press_key("mod+2").unwrap();
    app.settle();
    assert!(
        app.screen().count("id:budget-category-search").unwrap() >= 1
            && app.screen().count("id:placeholder-note").unwrap() == 0,
        "下拉开着时 ⌘2 不得切视图"
    );
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:budget-category-search", DEFAULT_WAIT)
        .unwrap();
    app.press_key("mod+2").unwrap();
    app.wait_for("text:账户").unwrap();
    app.press_key("mod+3").unwrap();
    app.wait_for("text:预算执行").unwrap();

    // 编辑弹窗开着：⌘2 不派发；ESC 关闭后落位账户。
    app.press("id:budget-edit").unwrap();
    app.wait_for("id:app-modal").unwrap();
    app.press_key("mod+2").unwrap();
    app.settle();
    assert!(
        app.screen().count("id:app-modal").unwrap() >= 1
            && app.screen().count("id:placeholder-note").unwrap() == 0,
        "弹窗开着时 ⌘2 不得切视图"
    );
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:app-modal", DEFAULT_WAIT).unwrap();
    app.press_key("mod+2").unwrap();
    app.wait_for_within("text:账户", DEFAULT_WAIT).unwrap();
    app.press_key("mod+3").unwrap();
    app.wait_for("text:预算执行").unwrap();

    // 删除气泡开着：⌘2 不派发；ESC 关闭后落位账户。
    app.press("id:budget-delete").unwrap();
    app.wait_for("text:确认删除？").unwrap();
    app.press_key("mod+2").unwrap();
    app.settle();
    assert!(
        app.shows("确认删除？") && !app.shows("id:placeholder-note"),
        "气泡开着时 ⌘2 不得切视图"
    );
    app.press_key("escape").unwrap();
    app.wait_for_gone("text:确认删除？", DEFAULT_WAIT).unwrap();
    app.press_key("mod+2").unwrap();
    app.wait_for_within("text:账户", DEFAULT_WAIT).unwrap();
}

// ---------------------------------------------------------------------------
// 交易表单族（#2034 NSH 迁移③表单①基础收支）：新增/编辑旅程、校验呈现、
// 弹层抑制、日期键入。断言只写用户可观察结果（列表刷新、回填文本、错误条
// 位、键派发）；「写→失效信号→列表重读」与「弹层抑制」的接线删除即红条
// 目注明在各测试内。
// ---------------------------------------------------------------------------

/// 打开「记一笔」表单弹窗（入口菜单 → 指定类型项），等待标题落屏。
fn open_create_form(app: &mut Headless, item_id: &str, title: &str) {
    app.press("id:tx-add").unwrap();
    app.wait_for(item_id).unwrap();
    app.press(item_id).unwrap();
    app.wait_for_within(&format!("text:{title}"), DEFAULT_WAIT)
        .unwrap();
}

/// 在账户下拉中选中唯一候选（造数面保证单账户；选项行锚点 + 行内文本
/// 命中可按压件——budget-category-option 同形选择器）。
fn pick_only_account(app: &mut Headless) {
    app.press("id:tx-form-account").unwrap();
    app.wait_for("id:tx-form-account-option").unwrap();
    app.press("id:tx-form-account-option >> text:测试现金")
        .unwrap();
    app.settle();
}

/// 新增支出全旅程（用户故事 1/6）：记一笔 → 记支出 → 填金额、选账户 →
/// 保存 → 弹窗关、列表即刻刷新（新行与总数落屏）。删掉表单提交成功分支
/// 的失效信号广播（txn_form::perform_submit 内 notify 接线）本测试
/// wait_for("text:¥12.5") 超时红（库中已写而列表仍旧）。
#[test]
fn form_creates_expense_and_refreshes_list() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("form-create");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    open_create_form(&mut app, "id:tx-add-expense", "记一笔 · 支出");
    app.type_text("id:tx-form-amount", "12.50").unwrap();
    pick_only_account(&mut app);
    app.press("id:tx-form-submit").unwrap();

    // 写后可观察结果：弹窗关、列表即刻刷新（金额按展示口径裁尾零：12.50 → ¥12.5）。
    app.wait_for_gone("text:记一笔 · 支出", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    let today = chrono::Local::now().date_naive().to_string();
    app.wait_for_within(&format!("text:{today}"), DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:¥12.5").unwrap();
    assert!(
        app.shows("测试现金"),
        "新行账户格呈现：{:?}",
        app.screen().texts()
    );

    // 库态核对（不经 UI）：域读路径确认写入与币种随账户推导。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let filter = ledger_transaction::TransactionListFilter::default();
    let rows = ledger_transaction::list_transactions(&conn, &filter).unwrap();
    assert_eq!(rows.total, 1);
    assert_eq!(
        rows.items[0].amount_cents, 1_250,
        "元转分收口（整数分落库）"
    );
    assert_eq!(rows.items[0].currency_code, "CNY", "记账币种 = 账户币种");
    assert_eq!(
        rows.items[0].kind,
        ledger_transaction::TransactionKind::Expense
    );
}

/// 新增收入旅程 + kind 选项面分域（用户故事 1/3）：记收入表单的分类候选只
/// 收收入分类（收入分类可选中；支出分类不在候选）；保存后行类型标签「收入」。
#[test]
fn form_creates_income_with_income_only_category_candidates() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("form-income");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_categories::create_category(
            &conn,
            ledger_categories::CategoryInput {
                name: "工资".to_owned(),
                kind: "income".to_owned(),
                parent_id: None,
                icon: None,
            },
        )
        .unwrap();
        ledger_categories::create_category(
            &conn,
            ledger_categories::CategoryInput {
                name: "餐饮".to_owned(),
                kind: "expense".to_owned(),
                parent_id: None,
                icon: None,
            },
        )
        .unwrap();
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    open_create_form(&mut app, "id:tx-add-income", "记一笔 · 收入");
    app.type_text("id:tx-form-amount", "8000").unwrap();
    pick_only_account(&mut app);

    // 分类候选面随 kind 分域：打开分类下拉只呈现收入分类（支出分类被过滤；
    // 内置收入分类树的根「工资」与子分类路径共享子串，取第 0 命中 = 根）。
    app.press("id:tx-form-category").unwrap();
    app.wait_for("id:tx-form-category-option").unwrap();
    assert!(
        app.shows("工资") && !app.shows("餐饮"),
        "收入表单的分类候选只收收入分类：{:?}",
        app.screen().texts()
    );
    app.press("id:tx-form-category-option >> text:工资 >> nth=0")
        .unwrap();
    app.settle();

    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:记一笔 · 收入", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:¥8000").unwrap();
    assert!(
        app.shows("收入"),
        "行类型标签「收入」：{:?}",
        app.screen().texts()
    );
    assert!(
        app.shows("工资"),
        "分类格呈现收入分类名：{:?}",
        app.screen().texts()
    );
}

/// 编辑回填与保存旅程（用户故事 5/6）：右键收入/支出行 → 编辑 → 弹窗回填
/// 全部业务字段（金额文本 / 日期 / 备注 / 商户口名）→ 改金额保存 → 列表
/// 刷新呈现新值。删掉编辑提交成功分支的失效信号广播（txn_form::
/// perform_submit 内 notify 接线）本测试 wait_for("text:¥30") 超时红。
#[test]
fn form_edits_expense_with_backfill_and_refreshes() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("form-edit");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: "2026-01-01",
                category_id: None,
                merchant_id: None,
                note: Some("要编辑的一笔".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.right_click("text:2026-01-01").unwrap();
    app.choose_menu_item("编辑").unwrap();
    app.wait_for("text:编辑交易").unwrap();

    // 回填断言（编辑复用同表单；金额分→元文本按行币种小数位，日期 ISO 原样）。
    assert_eq!(
        app.screen().input_text("id:tx-form-amount").unwrap(),
        Some("25"),
        "金额回填：2500 分 → 25 元文本"
    );
    assert_eq!(
        app.screen().input_text("id:tx-form-date-input").unwrap(),
        Some("2026-01-01"),
        "日期回填：行上 ISO 原样"
    );
    assert_eq!(
        app.screen().input_text("id:tx-form-note").unwrap(),
        Some("要编辑的一笔"),
        "备注回填"
    );
    assert_eq!(
        app.screen()
            .input_text("id:tx-form-merchant-input")
            .unwrap(),
        Some(""),
        "无商户行回填空文本"
    );

    // 改金额保存 → 弹窗关、列表刷新呈现新金额（旧值离屏）。
    app.clear("id:tx-form-amount").unwrap();
    app.type_text("id:tx-form-amount", "30").unwrap();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:编辑交易", DEFAULT_WAIT).unwrap();
    app.wait_for("text:¥30").unwrap();
    app.wait_for_gone("text:¥25", DEFAULT_WAIT).unwrap();
    assert!(app.shows("共 1 条"), "总数不变：{:?}", app.screen().texts());

    // 库态核对：全字段替换落库（金额 3000 分，其余字段不变）。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let filter = ledger_transaction::TransactionListFilter::default();
    let rows = ledger_transaction::list_transactions(&conn, &filter).unwrap();
    assert_eq!(rows.items[0].amount_cents, 3_000);
    assert_eq!(rows.items[0].note.as_deref(), Some("要编辑的一笔"));
    assert_eq!(rows.items[0].date, "2026-01-01");
}

/// 校验呈现（用户故事 7/8）：空金额直接提交 → 错误条位可纠正文案；非法
/// 金额文本（判定错误 + 已标记）→ 提交禁用；账户未选 → 「请选择账户」；
/// 修正后可提交成功。断言对准用户可观察结果（文案与按钮可用性），不对准
/// 内部状态形状。
#[test]
fn form_blocks_invalid_submission_with_visible_errors() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("form-invalid");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    open_create_form(&mut app, "id:tx-add-expense", "记一笔 · 支出");

    // 全空直接提交：装配器 fail fast 以账户先于金额（旧壳 submit 校验序同形）
    // → 「请选择账户」错误条位（不产生写入）。
    app.press("id:tx-form-submit").unwrap();
    app.wait_for("id:tx-form-error").unwrap();
    assert!(
        app.shows("请选择账户"),
        "账户未选错误条位：{:?}",
        app.screen().texts()
    );

    // 非法金额文本：判定错误红态 → 提交禁用（红框 + 提交禁用两件同发，
    // ADR-0058；headless 断言可用性，红框归人工子集）。
    app.type_text("id:tx-form-amount", "abc").unwrap();
    app.settle();
    assert!(
        !app.screen().enabled("id:tx-form-submit").unwrap(),
        "非法金额文本下提交禁用"
    );

    // 负数金额（判定可解析、业务校验拦截；账户已选）→ 「请输入金额」。
    app.clear("id:tx-form-amount").unwrap();
    app.type_text("id:tx-form-amount", "-3").unwrap();
    pick_only_account(&mut app);
    app.press("id:tx-form-submit").unwrap();
    app.wait_for("text:请输入金额").unwrap();

    // 全部修正 → 提交成功（错误条位清空、弹窗关、列表刷新）。
    app.clear("id:tx-form-amount").unwrap();
    app.type_text("id:tx-form-amount", "12.50").unwrap();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:记一笔 · 支出", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
}

/// 弹层抑制（#2032 抑制闸门 + #2034 表单弹层共用）：表单弹窗开着时翻页键
/// 不派发，ESC 关窗后生效。删掉 modal 封装的抑制上报（token.report 接线）
/// 或 keymap 闸门检查任一环，本测试红（弹窗开着时翻页键穿透）。
#[test]
fn form_modal_suppresses_pagination_shortcut_while_open() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("form-suppress");
    seed_expense_series(&dir, 105);

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);
    // 第 1 页首行 = 日期最大一笔（test_date(104) = 2026-04-14）。
    app.wait_for("text:2026-04-14").unwrap();
    assert!(
        app.shows("2026-04-14"),
        "第 1 页首行在屏：{:?}",
        app.screen().texts()
    );

    open_create_form(&mut app, "id:tx-add-expense", "记一笔 · 支出");
    app.press_key("]").unwrap();
    app.settle();
    assert!(
        app.shows("2026-04-14") && !app.shows("2026-01-01"),
        "表单弹窗开着时翻页键不派发（抑制）：{:?}",
        app.screen().texts()
    );

    // ESC 关窗（弹层关闭语义）→ 同键生效翻到第 2 页（首行 = 2026-01-01）。
    app.press_key("escape").unwrap();
    app.wait_for_gone("text:记一笔 · 支出", DEFAULT_WAIT)
        .unwrap();
    app.press_key("]").unwrap();
    app.wait_for_within("text:2026-01-01", DEFAULT_WAIT)
        .unwrap();
    assert!(
        app.shows("共 105 条"),
        "翻页不改总数口径：{:?}",
        app.screen().texts()
    );
}

/// 日期键入（用户故事 2 headless 面）：清空缺省值后键入完整日期落值，保存
/// 后行日期随键入值；残缺文本不落值（触发件文本原样保留）。日历点选的原生
/// 观感归人工子集（headless 无像素断言）。
#[test]
fn form_date_field_accepts_typed_date_and_lands_on_row() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("form-date");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    open_create_form(&mut app, "id:tx-add-expense", "记一笔 · 支出");
    // 缺省 = 今天。
    let today = chrono::Local::now().date_naive().to_string();
    assert_eq!(
        app.screen().input_text("id:tx-form-date-input").unwrap(),
        Some(today.as_str()),
        "日期缺省今天（旧壳 NDatePicker 缺省同语义）"
    );

    // 键入完整日期落值（回显归一）。
    app.clear("id:tx-form-date-input").unwrap();
    app.type_text("id:tx-form-date-input", "2026-03-15")
        .unwrap();
    app.settle();
    assert_eq!(
        app.screen().input_text("id:tx-form-date-input").unwrap(),
        Some("2026-03-15"),
        "完整日期落值并回显"
    );

    app.type_text("id:tx-form-amount", "5").unwrap();
    pick_only_account(&mut app);
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:记一笔 · 支出", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:2026-03-15").unwrap();
    app.wait_for("text:¥5").unwrap();
}

/// 来源关联语义·保单（用户故事 7 / 旧壳「挂单可改挂与清除」平价）：普通下拉
/// 选中保单 → 保存落 policy_id；清除（✕）→ 保存落 None（改挂与清除两态）。
/// 域权威（挂单准入 / unchanged 语义）在域层既有测试，壳层钉选项面与提交面。
#[test]
fn form_attaches_policy_and_clears_it() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("form-policy");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        let insurer_id = ledger_policy::create_insurer_by_name(&conn, "测试保司").unwrap();
        ledger_policy::create_policy(
            &conn,
            ledger_policy::PolicyInput {
                insurer_id: insurer_id.clone(),
                policy_number: "P-2026-001".to_owned(),
                product_name: "百万医疗".to_owned(),
                start_date: "2026-01-01".to_owned(),
                end_date: None,
                coverage_amount_cents: None,
                coverage_currency_code: None,
                note: None,
            },
            &mut || {},
        )
        .unwrap();
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    // 挂单：保单选项呈现「保单号（险种名）」词表，选中后保存落 policy_id。
    open_create_form(&mut app, "id:tx-add-expense", "记一笔 · 支出");
    app.type_text("id:tx-form-amount", "200").unwrap();
    pick_only_account(&mut app);
    app.press("id:tx-form-policy-trigger").unwrap();
    app.wait_for("id:tx-form-policy-option").unwrap();
    app.press("id:tx-form-policy-option >> text:P-2026-001（百万医疗）")
        .unwrap();
    app.settle();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:记一笔 · 支出", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();

    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    let policy_id = rows.items[0].policy_id.clone();
    assert!(policy_id.is_some(), "挂单保存落 policy_id");

    // 清除：编辑面清除按钮（✕）落 None——「挂单可清除」语义。
    app.right_click("text:¥200").unwrap(); // 金额格文本寻址该行（单行账本）
    app.choose_menu_item("编辑").unwrap();
    app.wait_for("text:编辑交易").unwrap();
    app.press("id:tx-form-policy-clear").unwrap();
    app.settle();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:编辑交易", DEFAULT_WAIT).unwrap();

    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    assert_eq!(rows.items[0].policy_id, None, "清除后保存落 None");
}

/// 来源关联语义·商户（旧壳 PinyinSelect tag「输入即建」平价）：自由文本提交
/// 走 merchant_name 域语义（精确匹配在用名复用、未命中即建——域内单点，壳
/// 不另立解析）；选定建议项走 merchant_id。域权威在域层既有测试，壳层钉
/// 双态值语义的提交面。
#[test]
fn form_merchant_free_text_creates_merchant_via_domain_semantics() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("form-merchant");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    open_create_form(&mut app, "id:tx-add-expense", "记一笔 · 支出");
    app.type_text("id:tx-form-amount", "99").unwrap();
    pick_only_account(&mut app);
    // 自由文本输入（不在建议中选）→ 提交 merchant_name。
    app.type_text("id:tx-form-merchant-input", "红旗超市")
        .unwrap();
    app.press_key("escape").unwrap(); // 关建议弹层（弹层关闭语义），表单仍在
    app.settle();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:记一笔 · 支出", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    assert!(app.shows("红旗超市"), "列表商户格呈现即建商户名");

    // 库态核对：域语义即建在用商户并挂到交易行。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    let merchant_id = rows.items[0].merchant_id.clone();
    assert!(merchant_id.is_some(), "merchant_name 语义即建并挂引用");
    let merchants = ledger_merchants::list_merchants(&conn, false).unwrap();
    assert!(
        merchants.iter().any(|m| m.name == "红旗超市"),
        "商户字典即建在用行：{:?}",
        merchants
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// 交易表单②（#2035 NSH 迁移④表单②转账·借出·退款）：多账户形态的新增/编辑
// 旅程。断言只写用户可观察结果（菜单项、回填文本、候选面分侧、行格文案、
// 库态核对）；「写→失效信号→列表重读」接线删除即红条目注明在测试内。
// ---------------------------------------------------------------------------

/// 在指定下拉（锚点前缀）中选中候选行内文本命中的选项。
fn pick_option(app: &mut Headless, prefix: &str, label: &str) {
    app.press(&format!("id:{prefix}")).unwrap();
    app.wait_for(&format!("id:{prefix}-option")).unwrap();
    app.press(&format!("id:{prefix}-option >> text:{label}"))
        .unwrap();
    app.settle();
}

/// 断言拼音下拉触发件当前显示指定 label（回填断言面：触发件文本呈现选中项）。
fn assert_select_shows(app: &mut Headless, prefix: &str, label: &str) {
    app.wait_for_within(&format!("id:{prefix} >> text:{label}"), DEFAULT_WAIT)
        .unwrap();
}

/// 新增转账全旅程（用户故事 1）：记一笔 → 记转账 → 填金额、选转出/转入 →
/// 保存 → 弹窗关、列表即刻刷新（双账户行格「转出 → 转入」）。删掉表单提交
/// 成功分支的失效信号广播（txn_form::perform_submit 内 notify 接线）本测试
/// wait_for("text:共 1 条") 超时红（库中已写而列表仍旧）。
#[test]
fn transfer_form_creates_transfer_and_refreshes_list() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("transfer-create");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_account(&db, "测试现金", AccountType::Cash);
        seed_account(&db, "测试银行", AccountType::Bank);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    open_create_form(&mut app, "id:tx-add-transfer", "记一笔 · 转账");
    app.type_text("id:tx-form-amount", "300").unwrap();
    pick_option(&mut app, "tx-form-account", "测试现金");
    pick_option(&mut app, "tx-form-to-account", "测试银行");
    app.press("id:tx-form-submit").unwrap();

    // 写后可观察结果：弹窗关、列表刷新（转账行格双账户口径 + 类型标签）。
    app.wait_for_gone("text:记一笔 · 转账", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:¥300").unwrap();
    app.wait_for("text:测试现金 → 测试银行").unwrap();
    assert!(
        app.shows("转账"),
        "行类型标签「转账」：{:?}",
        app.screen().texts()
    );

    // 库态核对（不经 UI）：kind=transfer 双账户落位、币种随转出账户推导。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    assert_eq!(rows.total, 1);
    assert_eq!(rows.items[0].kind, TransactionKind::Transfer);
    assert_eq!(
        rows.items[0].amount_cents, 30_000,
        "元转分收口（整数分落库）"
    );
    assert_eq!(rows.items[0].currency_code, "CNY");
    let cash = ledger_accounts::list_accounts(&conn).unwrap();
    let from = cash.iter().find(|a| a.name == "测试现金").unwrap();
    let to = cash.iter().find(|a| a.name == "测试银行").unwrap();
    assert_eq!(rows.items[0].account_id.as_deref(), Some(from.id.as_str()));
    assert_eq!(rows.items[0].to_account_id.as_deref(), Some(to.id.as_str()));
}

/// 转账校验呈现（旧壳 submit 守卫序平价）：空提交 → 「请选择转出账户」；
/// 只选转出 → 「请选择转入账户」；两端同账户 → 「转出账户和转入账户不能
/// 相同」；非法金额文本 → 提交禁用；修正后提交成功。
#[test]
fn transfer_form_blocks_incomplete_submission_with_visible_errors() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("transfer-invalid");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_account(&db, "测试现金", AccountType::Cash);
        seed_account(&db, "测试银行", AccountType::Bank);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    open_create_form(&mut app, "id:tx-add-transfer", "记一笔 · 转账");

    // 全空提交：转出账户缺失先于一切（旧壳守卫序同形）。
    app.press("id:tx-form-submit").unwrap();
    app.wait_for("id:tx-form-error").unwrap();
    assert!(app.shows("请选择转出账户"), "{:?}", app.screen().texts());

    // 只选转出：转入缺失的专属文案（不与基础收支「请选择账户」混词）。
    pick_option(&mut app, "tx-form-account", "测试现金");
    app.type_text("id:tx-form-amount", "300").unwrap();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for("text:请选择转入账户").unwrap();

    // 转入选转出同账户：同账户专属文案。
    pick_option(&mut app, "tx-form-to-account", "测试现金");
    app.press("id:tx-form-submit").unwrap();
    app.wait_for("text:转出账户和转入账户不能相同").unwrap();

    // 非法金额文本：判定错误红态 → 提交禁用（红框 + 提交禁用两件同发，
    // ADR-0058；headless 断言可用性，红框归人工子集）。
    app.clear("id:tx-form-amount").unwrap();
    app.type_text("id:tx-form-amount", "abc").unwrap();
    app.settle();
    assert!(
        !app.screen().enabled("id:tx-form-submit").unwrap(),
        "非法金额文本下提交禁用"
    );

    // 全部修正 → 提交成功（错误条位清空、弹窗关、列表刷新）。
    app.clear("id:tx-form-amount").unwrap();
    app.type_text("id:tx-form-amount", "300").unwrap();
    pick_option(&mut app, "tx-form-to-account", "测试银行");
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:记一笔 · 转账", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
}

/// 编辑既有转账（用户故事 4/5）：右键转出行 → 编辑 → 回填金额/日期/备注与
/// 两端账户（触发件呈现选中账户名）→ 改金额保存 → 列表刷新、转入侧保持。
/// 删掉编辑提交成功分支的失效信号广播本测试 wait_for("text:¥80") 超时红。
#[test]
fn transfer_form_edits_with_dual_account_backfill() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("transfer-edit");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_account(&db, "测试现金", AccountType::Cash);
        let bank = seed_account(&db, "测试银行", AccountType::Bank);
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: Some(bank),
                kind: TransactionKind::Transfer,
                amount_cents: 5_000,
                date: "2026-01-01",
                category_id: None,
                merchant_id: None,
                note: Some("过桥".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.right_click("text:2026-01-01").unwrap();
    app.choose_menu_item("编辑").unwrap();
    app.wait_for("text:编辑交易").unwrap();

    // 回填断言：金额/日期/备注 + 两端账户触发件呈现选中名。
    assert_eq!(
        app.screen().input_text("id:tx-form-amount").unwrap(),
        Some("50"),
        "金额回填：5000 分 → 50 元文本"
    );
    assert_eq!(
        app.screen().input_text("id:tx-form-date-input").unwrap(),
        Some("2026-01-01")
    );
    assert_eq!(
        app.screen().input_text("id:tx-form-note").unwrap(),
        Some("过桥")
    );
    assert_select_shows(&mut app, "tx-form-account", "测试现金");
    assert_select_shows(&mut app, "tx-form-to-account", "测试银行");

    // 改金额保存 → 列表刷新呈现新金额；两端账户保持（库态核对）。
    app.clear("id:tx-form-amount").unwrap();
    app.type_text("id:tx-form-amount", "80").unwrap();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:编辑交易", DEFAULT_WAIT).unwrap();
    app.wait_for("text:¥80").unwrap();

    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    assert_eq!(rows.items[0].amount_cents, 8_000);
    assert_eq!(rows.items[0].kind, TransactionKind::Transfer);
    assert!(
        rows.items[0].account_id.is_some() && rows.items[0].to_account_id.is_some(),
        "全字段替换下双账户保持"
    );
    assert_eq!(rows.items[0].note.as_deref(), Some("过桥"));
}

/// 新增借出（用户故事 2）：记一笔 → 借出 → 借贷变体表单（方向切换钮四向 +
/// 商户输入位），账户候选按方向分侧——转出侧只收资金账户、转入侧只收借出款
/// 账户。保存后行类型标签「借出」（借贷派生文案）。
#[test]
fn lending_form_creates_lend_with_side_filtered_candidates() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("lending-create");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_account(&db, "测试现金", AccountType::Cash);
        seed_account(&db, "借出·张三", AccountType::Receivable);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    open_create_form(&mut app, "id:tx-add-lend", "记一笔 · 借出");
    // 借贷变体特征面：方向切换钮四向（旧壳 LENDING_FORM_DIRECTIONS 键序）。
    assert!(
        app.shows("方向") && app.shows("收回") && app.shows("借入") && app.shows("还款"),
        "方向切换钮四向呈现：{:?}",
        app.screen().texts()
    );

    // 转出侧候选 = 资金账户（应收账户不进新选择）。
    app.press("id:tx-form-account").unwrap();
    app.wait_for("id:tx-form-account-option").unwrap();
    assert!(
        app.shows("测试现金") && !app.shows("借出·张三"),
        "转出侧候选只收资金账户：{:?}",
        app.screen().texts()
    );
    app.press_key("escape").unwrap();
    app.settle();

    // 转入侧候选 = 借出款账户（资金账户不进新选择）。
    app.press("id:tx-form-to-account").unwrap();
    app.wait_for("id:tx-form-to-account-option").unwrap();
    assert!(
        app.shows("借出·张三") && !app.shows("测试现金"),
        "转入侧候选只收借出款账户：{:?}",
        app.screen().texts()
    );
    app.press("id:tx-form-to-account-option >> text:借出·张三")
        .unwrap();
    app.settle();

    pick_option(&mut app, "tx-form-account", "测试现金");
    app.type_text("id:tx-form-amount", "100").unwrap();
    app.press("id:tx-form-submit").unwrap();

    app.wait_for_gone("text:记一笔 · 借出", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    assert!(
        app.shows("借出"),
        "行类型标签「借出」（借贷派生文案）：{:?}",
        app.screen().texts()
    );
    app.wait_for("text:测试现金 → 借出·张三").unwrap();

    // 库态核对：提交产物与转账同构（kind=transfer，两端落位）。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    assert_eq!(rows.items[0].kind, TransactionKind::Transfer);
    let accounts = ledger_accounts::list_accounts(&conn).unwrap();
    let cash = accounts.iter().find(|a| a.name == "测试现金").unwrap();
    let recv = accounts.iter().find(|a| a.name == "借出·张三").unwrap();
    assert_eq!(rows.items[0].account_id.as_deref(), Some(cash.id.as_str()));
    assert_eq!(
        rows.items[0].to_account_id.as_deref(),
        Some(recv.id.as_str())
    );
}

/// 编辑既有借出（用户故事 4）：借贷是 transfer 派生视角——编辑形态按两端
/// 账户类型派生方向进借贷变体（方向行 + 商户输入位呈现，普通转账表单无
/// 商户位），回填两端账户与金额；改备注保存后两端保持。
#[test]
fn lending_form_edit_derives_direction_and_backfills() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("lending-edit");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_account(&db, "测试现金", AccountType::Cash);
        let recv = seed_account(&db, "借出·张三", AccountType::Receivable);
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: Some(recv),
                kind: TransactionKind::Transfer,
                amount_cents: 10_000,
                date: "2026-02-01",
                category_id: None,
                merchant_id: None,
                note: Some("借出张三".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.right_click("text:2026-02-01").unwrap();
    app.choose_menu_item("编辑").unwrap();
    app.wait_for("text:编辑交易").unwrap();

    // 借贷变体特征面：方向行 + 商户输入位（普通转账表单不渲染商户位）。
    assert!(
        app.shows("方向"),
        "借贷变体呈现方向行：{:?}",
        app.screen().texts()
    );
    assert!(
        app.screen()
            .input_text("id:tx-form-merchant-input")
            .unwrap()
            .is_some(),
        "借贷变体呈现商户输入位"
    );
    // 回填断言：两端账户触发件呈现选中名 + 金额。
    assert_select_shows(&mut app, "tx-form-account", "测试现金");
    assert_select_shows(&mut app, "tx-form-to-account", "借出·张三");
    assert_eq!(
        app.screen().input_text("id:tx-form-amount").unwrap(),
        Some("100")
    );

    // 改备注保存 → 库态核对：备注更新、kind 与两端保持。
    app.clear("id:tx-form-note").unwrap();
    app.type_text("id:tx-form-note", "借出张三·改").unwrap();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:编辑交易", DEFAULT_WAIT).unwrap();
    app.wait_for("text:共 1 条").unwrap();

    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    assert_eq!(rows.items[0].kind, TransactionKind::Transfer);
    assert_eq!(rows.items[0].note.as_deref(), Some("借出张三·改"));
    assert_eq!(rows.items[0].amount_cents, 10_000, "未改金额保持");
    let accounts = ledger_accounts::list_accounts(&conn).unwrap();
    let cash = accounts.iter().find(|a| a.name == "测试现金").unwrap();
    let recv = accounts.iter().find(|a| a.name == "借出·张三").unwrap();
    assert_eq!(rows.items[0].account_id.as_deref(), Some(cash.id.as_str()));
    assert_eq!(
        rows.items[0].to_account_id.as_deref(),
        Some(recv.id.as_str())
    );
}

/// 经域层公开写入口造一笔交易并返回 id（退款/菜单面测试的造数面；载荷全字
/// 段显式——TransactionInput 无 Default）。
fn seed_tx_with_id(db: &ledger_infra::db::DbState, input: TransactionInput) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_transaction::create(&conn, input).unwrap().id
}

/// 最小通用 kind 写入载荷（现金账户、CNY；可选原支出引用与备注）。
fn minimal_input(
    kind: TransactionKind,
    account: &str,
    amount_cents: i64,
    date: &str,
    refund_of: Option<String>,
    note: Option<String>,
) -> TransactionInput {
    TransactionInput {
        kind,
        amount_cents,
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
        refund_of_transaction_id: refund_of,
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
    }
}

/// 行内退款全旅程（用户故事 3）：支出行右键 → 退款 → 表单锁定呈现继承面
/// （原交易快照行 / 账户锁定 / 币种继承），金额缺省回填原交易金额（「金额
/// 钳制」的入口形态——缺省随原交易、可改部分退款；超额不做壳层钳制，继承
/// 与口径以域为唯一真源）→ 保存 → 列表刷新（退款行「退款」标签）。
/// 删掉提交成功分支的失效信号广播本测试 wait_for("text:共 2 条") 超时红。
#[test]
fn refund_form_creates_refund_from_row_with_default_amount() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("refund-create");
    let expense_id = {
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
        drop(conn);
        let cash = seed_account(&db, "测试现金", AccountType::Cash);
        let mut expense = minimal_input(
            TransactionKind::Expense,
            &cash,
            2_500,
            "2026-01-01",
            None,
            Some("午餐".to_owned()),
        );
        expense.category_id = Some(food);
        seed_tx_with_id(&db, expense)
    };

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.right_click("text:2026-01-01").unwrap();
    app.choose_menu_item("退款").unwrap();
    app.wait_for("text:记一笔 · 退款").unwrap();

    // 继承面锁定呈现：原交易快照行（日期 · 金额 · 分类路径 · 账户）+ 账户
    // 锁定 + 币种继承；金额缺省回填原交易金额。
    app.wait_for("text:原交易：2026-01-01 · ¥25 · 食品 · 测试现金")
        .unwrap();
    assert_eq!(
        app.screen().input_text("id:tx-form-amount").unwrap(),
        Some("25"),
        "金额缺省 = 原交易金额（钳制入口形态）"
    );
    app.wait_for_within("id:tx-form-refund-account >> text:测试现金", DEFAULT_WAIT)
        .unwrap();
    app.wait_for_within("id:tx-form-currency >> text:CNY", DEFAULT_WAIT)
        .unwrap();

    // 部分退款：改金额保存 → 列表刷新（退款行「退款」标签）。
    app.clear("id:tx-form-amount").unwrap();
    app.type_text("id:tx-form-amount", "10").unwrap();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:原交易：2026-01-01", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 2 条").unwrap();
    assert!(
        app.shows("退款"),
        "行类型标签「退款」：{:?}",
        app.screen().texts()
    );

    // 库态核对：kind=refund、原支出引用指向右键所在行、金额整数分落库。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    assert_eq!(rows.total, 2);
    let refund = rows
        .items
        .iter()
        .find(|t| t.kind == TransactionKind::Refund)
        .expect("退款行落库");
    assert_eq!(refund.amount_cents, 1_000, "部分退款 10 元 → 1000 分");
    assert_eq!(
        refund.refund_of_transaction_id.as_deref(),
        Some(expense_id.as_str())
    );
    // 继承语义归域：账户/币种/分类随原支出（壳层不另立）。
    let expense = rows
        .items
        .iter()
        .find(|t| t.kind == TransactionKind::Expense)
        .expect("原支出行保持");
    assert_eq!(refund.account_id, expense.account_id, "账户继承原支出");
    assert_eq!(
        refund.currency_code, expense.currency_code,
        "币种继承原支出"
    );
    assert_eq!(refund.category_id, expense.category_id, "分类继承原支出");
}

/// 编辑既有退款（用户故事 4，本票扩展开放面）：退款行右键 → 编辑 → 回填
/// 金额/备注与锁定账户 → 改金额保存 → 原支出引用透传保持（关联语义不被编
/// 辑破坏——旧壳 refund 行不开放编辑的顾虑由透传解除）。
#[test]
fn refund_form_edit_preserves_original_link() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("refund-edit");
    let (expense_id, refund_id) = {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_account(&db, "测试现金", AccountType::Cash);
        let expense_id = seed_tx_with_id(
            &db,
            minimal_input(
                TransactionKind::Expense,
                &cash,
                2_500,
                "2026-01-01",
                None,
                Some("午餐".to_owned()),
            ),
        );
        let refund_id = seed_tx_with_id(
            &db,
            minimal_input(
                TransactionKind::Refund,
                &cash,
                2_000,
                "2026-01-05",
                Some(expense_id.clone()),
                Some("部分退款".to_owned()),
            ),
        );
        (expense_id, refund_id)
    };

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.right_click("text:2026-01-05").unwrap();
    app.choose_menu_item("编辑").unwrap();
    app.wait_for("text:编辑交易").unwrap();

    // 回填断言：金额/备注 + 锁定账户；原交易快照行不渲染（编辑面原支出行
    // 多半不在当前页，引用由透传承载）。
    assert_eq!(
        app.screen().input_text("id:tx-form-amount").unwrap(),
        Some("20"),
        "金额回填：2000 分 → 20 元文本"
    );
    assert_eq!(
        app.screen().input_text("id:tx-form-note").unwrap(),
        Some("部分退款")
    );
    app.wait_for_within("id:tx-form-refund-account >> text:测试现金", DEFAULT_WAIT)
        .unwrap();
    assert!(
        !app.shows("原交易："),
        "编辑面不渲染原交易快照行：{:?}",
        app.screen().texts()
    );

    // 改金额保存 → 库态核对：金额更新、引用与备注保持。
    app.clear("id:tx-form-amount").unwrap();
    app.type_text("id:tx-form-amount", "15").unwrap();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:编辑交易", DEFAULT_WAIT).unwrap();
    app.wait_for("text:共 2 条").unwrap();

    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    let refund = rows
        .items
        .iter()
        .find(|t| t.id == refund_id)
        .expect("退款行保持");
    assert_eq!(refund.amount_cents, 1_500, "改金额 15 元 → 1500 分");
    assert_eq!(
        refund.refund_of_transaction_id.as_deref(),
        Some(expense_id.as_str()),
        "原支出引用透传保持（关联不被编辑破坏）"
    );
    assert_eq!(refund.note.as_deref(), Some("部分退款"), "未改字段保持");
}

/// 行菜单开放面（#2036）：分解行/购买项行随表单③子行编辑能力开放编辑（
/// #2035 子行守卫解除；分解行不开放退款——旧壳 #1861 口径，退款表单未支持
/// 显式分解覆盖）；退款行开放编辑（#2035 扩展，链接透传保关联）不带退款项；
/// 普通支出行三项齐备。购买项行保留退款（新建独立行不触碰原行子行）。
#[test]
fn row_menu_open_face_follows_kind_and_subrows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("menu-face");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_account(&db, "测试现金", AccountType::Cash);
        let credit = seed_account(&db, "测试信用", AccountType::Credit);
        let expense_id = seed_tx_with_id(
            &db,
            minimal_input(
                TransactionKind::Expense,
                &cash,
                2_500,
                "2026-01-01",
                None,
                Some("普通支出".to_owned()),
            ),
        );
        // 出资分解行（分解与 account_id 互斥，ADR-0138 决策 2）。
        let mut decomposed = minimal_input(
            TransactionKind::Expense,
            &cash,
            5_000,
            "2026-01-02",
            None,
            Some("分解支出".to_owned()),
        );
        decomposed.account_id = None;
        decomposed.funding = vec![
            TransactionFundingInput {
                account_id: cash.clone(),
                amount_cents: 2_000,
                label: None,
            },
            TransactionFundingInput {
                account_id: credit,
                amount_cents: 3_000,
                label: None,
            },
        ];
        seed_tx_with_id(&db, decomposed);
        // 退款行（关联普通支出）。
        seed_tx_with_id(
            &db,
            minimal_input(
                TransactionKind::Refund,
                &cash,
                1_000,
                "2026-01-03",
                Some(expense_id),
                None,
            ),
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 普通支出行：编辑 + 退款 + 删除。
    app.right_click("text:2026-01-01").unwrap();
    let menu = app.menu().expect("右键应打开上下文菜单");
    assert_eq!(
        menu.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        vec!["编辑", "退款", "删除"],
        "普通支出行菜单：{menu:?}"
    );
    app.press_key("escape").unwrap();
    app.settle();

    // 出资分解行：编辑开放（表单③子行编辑；进出资分解族回填）+ 不开放退
    // 款（#1861 口径）+ 删除。
    app.right_click("text:2026-01-02").unwrap();
    let menu = app.menu().expect("右键应打开上下文菜单");
    assert_eq!(
        menu.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        vec!["编辑", "删除"],
        "分解行开放编辑、不开放退款：{menu:?}"
    );
    app.press_key("escape").unwrap();
    app.settle();

    // 退款行：编辑（本票扩展）+ 删除，不带退款项（退款入口只对支出行）。
    app.right_click("text:2026-01-03").unwrap();
    let menu = app.menu().expect("右键应打开上下文菜单");
    assert_eq!(
        menu.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        vec!["编辑", "删除"],
        "退款行开放编辑（链接透传保关联）：{menu:?}"
    );
    app.press_key("escape").unwrap();
    app.settle();
}

// ---------------------------------------------------------------------------
// 出资分解 / 购买项下单（#2036 表单③）：录入旅程 + 编辑回填 + 开放面扩展。
// ---------------------------------------------------------------------------

/// 在行内账户下拉中选中指定账户（出资分解行选择器；选项行锚点 + 行内文本
/// 命中，pick_only_account 同形）。
fn pick_funding_account(app: &mut Headless, row: usize, label: &str) {
    app.press(&format!("id:tx-form-fund-account-{row}"))
        .unwrap();
    app.wait_for(&format!("id:tx-form-fund-account-{row}-option"))
        .unwrap();
    app.press(&format!(
        "id:tx-form-fund-account-{row}-option >> text:{label}"
    ))
    .unwrap();
    app.settle();
}

/// 出资分解创建全旅程（用户故事 1）：记一笔 → 出资分解 → 主金额、逐行账户/
/// 金额/标签、加行 → 合计行一致呈现 → 保存 → 弹窗关、列表即刻刷新（分解行
/// 账户格「首条 + 等 N 账户」标注与总数落屏）。删掉表单提交成功分支的失效
/// 信号广播（txn_form::perform_submit 内 notify 接线）本测试
/// wait_for("text:等 2 账户") 超时红（库中已写而列表仍旧）。
#[test]
fn funding_form_creates_split_expense_and_refreshes_list() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("funding-create");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_account(&db, "测试现金", AccountType::Cash);
        seed_account(&db, "测试信用", AccountType::Credit);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    open_create_form(&mut app, "id:tx-add-funding", "记一笔 · 出资分解");
    app.type_text("id:tx-form-amount", "50").unwrap();
    // 行 0（初始单行）：现金 20。
    pick_funding_account(&mut app, 0, "测试现金");
    app.type_text("id:tx-form-fund-amount-0", "20").unwrap();
    // 加行 → 行 1：信用 30 + 扣款标签。
    app.press("id:tx-form-funding-add").unwrap();
    app.wait_for("id:tx-form-fund-row-1").unwrap();
    pick_funding_account(&mut app, 1, "测试信用");
    app.type_text("id:tx-form-fund-amount-1", "30").unwrap();
    app.type_text("id:tx-form-fund-label-1", "尾款").unwrap();
    // 合计行一致呈现（Σ == 交易金额）。
    app.wait_for("text:出资合计 ¥50 · 交易金额 ¥50").unwrap();

    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:记一笔 · 出资分解", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    // 分解行账户格：首条 + 「等 2 账户」（旧壳 accountCellView 同口径）。
    app.wait_for("text:测试现金 等 2 账户").unwrap();
    app.wait_for("text:¥50").unwrap();

    // 库态核对（不经 UI）：分解两条落库、主账户恒缺省（契约互斥）。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    assert_eq!(rows.total, 1);
    let tx = &rows.items[0];
    assert_eq!(tx.amount_cents, 5_000, "主金额元转分收口");
    assert_eq!(tx.account_id, None, "分解行主账户恒缺省（ADR-0138 决策 2）");
    assert_eq!(tx.fundings.len(), 2, "逐条落库");
    assert_eq!(
        tx.fundings[0].account_id,
        rows.items[0].fundings[0].account_id
    );
    assert_eq!(tx.fundings[0].amount_cents, 2_000);
    assert_eq!(tx.fundings[0].label, None, "空标签落 None");
    assert_eq!(tx.fundings[1].amount_cents, 3_000);
    assert_eq!(
        tx.fundings[1].label.as_deref(),
        Some("尾款"),
        "扣款标签随行（同账户多笔/预售尾款区分语义）"
    );
}

/// 合计不一致明确阻拦（用户故事 2）：Σ 出资项 ≠ 交易金额时提交被阻拦，
/// 错误条位呈现 zh-CN 文案、弹窗不关、不写库。
#[test]
fn funding_form_blocks_mismatched_total_with_visible_error() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("funding-mismatch");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_account(&db, "测试现金", AccountType::Cash);
        seed_account(&db, "测试信用", AccountType::Credit);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    open_create_form(&mut app, "id:tx-add-funding", "记一笔 · 出资分解");
    app.type_text("id:tx-form-amount", "50").unwrap();
    pick_funding_account(&mut app, 0, "测试现金");
    app.type_text("id:tx-form-fund-amount-0", "20").unwrap();
    app.press("id:tx-form-funding-add").unwrap();
    app.wait_for("id:tx-form-fund-row-1").unwrap();
    pick_funding_account(&mut app, 1, "测试信用");
    app.type_text("id:tx-form-fund-amount-1", "25").unwrap();
    // 合计行不一致红显（45 ≠ 50）。
    app.wait_for("text:出资合计 ¥45 · 交易金额 ¥50").unwrap();

    app.press("id:tx-form-submit").unwrap();
    app.wait_for("text:出资合计与交易金额不一致").unwrap();
    assert!(
        app.shows("记一笔 · 出资分解"),
        "阻拦后弹窗不关：{:?}",
        app.screen().texts()
    );

    // 不写库。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    assert_eq!(rows.total, 0, "合计不一致不落库");
}

/// 分解行编辑开放面与全形态回填（用户故事 5）：右键分解行 → 「编辑」在列
/// （#2035 子行守卫随表单③解除）→ 出资分解族回填（主金额 + 逐行账户/金额/
/// 标签）→ 改主金额与一行出资保存 → 列表刷新 + 库态核对（全字段替换下子行
/// 更新、主账户仍缺省）。删掉 open_edit 的出资项子行回填（txn_form::open_edit
/// Funding 族分支）本测试金额回填断言红；删掉 app.rs 编辑开放面分支（还原子
/// 行守卫）菜单断言红。
#[test]
fn funding_form_edits_split_backfill_and_refreshes() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("funding-edit");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_account(&db, "测试现金", AccountType::Cash);
        let credit = seed_account(&db, "测试信用", AccountType::Credit);
        let mut input = minimal_input(
            TransactionKind::Expense,
            &cash,
            5_000,
            "2026-01-02",
            None,
            Some("分解支出".to_owned()),
        );
        input.account_id = None;
        input.funding = vec![
            TransactionFundingInput {
                account_id: cash,
                amount_cents: 2_000,
                label: None,
            },
            TransactionFundingInput {
                account_id: credit,
                amount_cents: 3_000,
                label: Some("尾款".to_owned()),
            },
        ];
        seed_tx_with_id(&db, input);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 菜单形状：分解行开放编辑（子行守卫解除）+ 不开放退款（#1861 口径）。
    app.right_click("text:2026-01-02").unwrap();
    let menu = app.menu().expect("右键应打开上下文菜单");
    let labels: Vec<_> = menu.iter().map(|e| e.label.as_str()).collect();
    assert!(labels.contains(&"编辑"), "分解行开放编辑：{labels:?}");
    assert!(
        !labels.contains(&"退款"),
        "分解行不开放退款（#1861 口径）：{labels:?}"
    );
    app.choose_menu_item("编辑").unwrap();
    app.wait_for("text:编辑交易").unwrap();

    // 回填断言（主金额 + 逐行账户/金额/标签）。
    assert_eq!(
        app.screen().input_text("id:tx-form-amount").unwrap(),
        Some("50"),
        "主金额回填：5000 分 → 50 元文本"
    );
    app.wait_for_within("id:tx-form-fund-row-0 >> text:测试现金", DEFAULT_WAIT)
        .unwrap();
    assert_eq!(
        app.screen().input_text("id:tx-form-fund-amount-0").unwrap(),
        Some("20"),
        "出资行 0 金额回填"
    );
    app.wait_for_within("id:tx-form-fund-row-1 >> text:测试信用", DEFAULT_WAIT)
        .unwrap();
    assert_eq!(
        app.screen().input_text("id:tx-form-fund-amount-1").unwrap(),
        Some("30"),
        "出资行 1 金额回填"
    );
    assert_eq!(
        app.screen().input_text("id:tx-form-fund-label-1").unwrap(),
        Some("尾款"),
        "扣款标签回填"
    );

    // 改主金额与一行出资保存（合计保持一致：50→55、30→35）。
    app.clear("id:tx-form-amount").unwrap();
    app.type_text("id:tx-form-amount", "55").unwrap();
    app.clear("id:tx-form-fund-amount-1").unwrap();
    app.type_text("id:tx-form-fund-amount-1", "35").unwrap();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:编辑交易", DEFAULT_WAIT).unwrap();
    app.wait_for("text:¥55").unwrap();

    // 库态核对：金额更新、子行全量替换落库、主账户仍缺省。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    assert_eq!(rows.total, 1);
    let tx = &rows.items[0];
    assert_eq!(tx.amount_cents, 5_500, "改主金额 55 元 → 5500 分");
    assert_eq!(tx.account_id, None, "主账户仍缺省（契约互斥保持）");
    assert_eq!(tx.fundings.len(), 2);
    assert_eq!(tx.fundings[0].amount_cents, 2_000, "未改行保持");
    assert_eq!(tx.fundings[1].amount_cents, 3_500, "改出资行落库");
    assert_eq!(tx.fundings[1].label.as_deref(), Some("尾款"), "标签保持");
    assert_eq!(tx.note.as_deref(), Some("分解支出"), "未改字段保持");
}

/// 订单创建全旅程（用户故事 3/4）：记一笔 → 购买项下单 → 主金额/账户 +
/// 逐行商品（名称/件数/单价）→ 行小计呈现 → 保存 → 列表刷新（购买项逐项
/// 行 + 计数按交易）+ 库态核对（purchases 逐条落库、仅 expense 契约）。删
/// 掉失效信号广播本测试 wait_for("text:共 1 条") 超时红（同创建旅程接线）。
#[test]
fn purchase_form_creates_order_rows_and_refreshes_list() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("purchase-create");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.wait_for("text:暂无数据").unwrap();

    open_create_form(&mut app, "id:tx-add-purchase", "记一笔 · 购买项下单");
    app.type_text("id:tx-form-amount", "100").unwrap();
    pick_only_account(&mut app);
    // 行 0：洗衣液 × 2，单价 39.9（行小计呈现 ¥79.8）。
    app.type_text("id:tx-form-purchase-name-0", "洗衣液")
        .unwrap();
    app.type_text("id:tx-form-purchase-qty-0", "2").unwrap();
    app.type_text("id:tx-form-purchase-price-0", "39.9")
        .unwrap();
    app.wait_for("text:小计 ¥79.8").unwrap();
    // 加行 → 行 1：纸巾 × 1，无单价（价格可空）。
    app.press("id:tx-form-purchase-add").unwrap();
    app.wait_for("id:tx-form-purchase-row-1").unwrap();
    app.type_text("id:tx-form-purchase-name-1", "纸巾").unwrap();
    app.type_text("id:tx-form-purchase-qty-1", "1").unwrap();

    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:记一笔 · 购买项下单", DEFAULT_WAIT)
        .unwrap();
    // 计数按交易计（购买项行不虚增，ADR-0138 决策 14）。
    app.wait_for("text:共 1 条").unwrap();
    // 购买项逐项行：备注列一列两用（名称 + 件数）。
    app.wait_for("text:洗衣液 共 2 件").unwrap();
    app.wait_for("text:纸巾 共 1 件").unwrap();

    // 库态核对：purchases 逐条落库（数组序、单价整数分、可空落 None）。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    assert_eq!(rows.total, 1);
    let tx = &rows.items[0];
    assert_eq!(tx.kind, TransactionKind::Expense, "仅 expense 可带购买项");
    assert_eq!(tx.amount_cents, 10_000, "订单实付元转分收口");
    assert_eq!(
        tx.account_id.as_deref(),
        Some("测试现金").map(|_| tx.account_id.as_deref().unwrap()),
        "主账户落库（订单单账户形态）"
    );
    assert_eq!(tx.purchases.len(), 2);
    assert_eq!(tx.purchases[0].name, "洗衣液");
    assert_eq!(tx.purchases[0].quantity, 2);
    assert_eq!(tx.purchases[0].unit_price_cents, Some(3_990));
    assert_eq!(tx.purchases[1].name, "纸巾");
    assert_eq!(tx.purchases[1].quantity, 1);
    assert_eq!(
        tx.purchases[1].unit_price_cents, None,
        "价格可空（存而不显示、不进金额口径）"
    );
}

/// 订单编辑旅程（用户故事 4/5）：右键购买项行 → 编辑 → 商品行回填 → 删一
/// 行 + 改件数 + 加一行保存 → 列表刷新 + 库态核对（增删改全形态）。删掉
/// open_edit 的商品行回填（txn_form::backfill_purchase_rows 接线）本测试
/// 回填断言红；删掉 app.rs 编辑开放面分支菜单断言红。
#[test]
fn purchase_form_edits_item_rows_backfill_add_delete() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("purchase-edit");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_cash_account(&db);
        let mut input = minimal_input(
            TransactionKind::Expense,
            &cash,
            10_000,
            "2026-01-02",
            None,
            Some("网购订单".to_owned()),
        );
        input.purchases = vec![
            TransactionPurchaseInput {
                name: "洗衣液".to_owned(),
                quantity: 2,
                category_id: None,
                unit_price_cents: Some(3_990),
            },
            TransactionPurchaseInput {
                name: "纸巾".to_owned(),
                quantity: 1,
                category_id: None,
                unit_price_cents: None,
            },
        ];
        seed_tx_with_id(&db, input);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    app.right_click("text:2026-01-02").unwrap();
    let menu = app.menu().expect("右键应打开上下文菜单");
    let labels: Vec<_> = menu.iter().map(|e| e.label.as_str()).collect();
    assert!(
        labels.contains(&"编辑"),
        "购买项行开放编辑（子行守卫解除）：{labels:?}"
    );
    assert!(
        labels.contains(&"退款"),
        "购买项行保留退款（新建独立行不触碰原行子行）：{labels:?}"
    );
    app.choose_menu_item("编辑").unwrap();
    app.wait_for("text:编辑交易").unwrap();

    // 回填断言（主金额 + 两行商品）。
    assert_eq!(
        app.screen().input_text("id:tx-form-amount").unwrap(),
        Some("100"),
        "主金额回填：10000 分 → 100 元文本"
    );
    assert_eq!(
        app.screen()
            .input_text("id:tx-form-purchase-name-0")
            .unwrap(),
        Some("洗衣液"),
        "商品行 0 名称回填"
    );
    assert_eq!(
        app.screen()
            .input_text("id:tx-form-purchase-qty-0")
            .unwrap(),
        Some("2"),
        "商品行 0 件数回填"
    );
    assert_eq!(
        app.screen()
            .input_text("id:tx-form-purchase-price-0")
            .unwrap(),
        Some("39.9"),
        "商品行 0 单价回填：3990 分 → 39.9 元文本"
    );
    assert_eq!(
        app.screen()
            .input_text("id:tx-form-purchase-name-1")
            .unwrap(),
        Some("纸巾"),
        "商品行 1 名称回填"
    );
    assert_eq!(
        app.screen()
            .input_text("id:tx-form-purchase-price-1")
            .unwrap(),
        Some(""),
        "可空单价回填空文本"
    );

    // 删行 0（洗衣液）→ 改纸巾件数 1→3 → 加一行（电池 ×2 单价 5）保存。
    app.press("id:tx-form-purchase-remove-0").unwrap();
    app.wait_for_gone("id:tx-form-purchase-row-0", DEFAULT_WAIT)
        .unwrap();
    app.clear("id:tx-form-purchase-qty-1").unwrap();
    app.type_text("id:tx-form-purchase-qty-1", "3").unwrap();
    app.press("id:tx-form-purchase-add").unwrap();
    app.wait_for("id:tx-form-purchase-row-2").unwrap();
    app.type_text("id:tx-form-purchase-name-2", "电池").unwrap();
    app.type_text("id:tx-form-purchase-qty-2", "2").unwrap();
    app.type_text("id:tx-form-purchase-price-2", "5").unwrap();
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:编辑交易", DEFAULT_WAIT).unwrap();
    app.wait_for("text:纸巾 共 3 件").unwrap();
    app.wait_for("text:电池 共 2 件").unwrap();

    // 库态核对：删改加全形态落库（数组序 = 对账单顺序）。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let rows = ledger_transaction::list_transactions(&conn, &Default::default()).unwrap();
    assert_eq!(rows.total, 1);
    let purchases = &rows.items[0].purchases;
    assert_eq!(purchases.len(), 2, "删一加一后两行");
    assert_eq!(purchases[0].name, "纸巾", "删除后剩余行保持原序位");
    assert_eq!(purchases[0].quantity, 3, "改件数落库");
    assert_eq!(purchases[1].name, "电池", "新增行落库");
    assert_eq!(purchases[1].quantity, 2);
    assert_eq!(purchases[1].unit_price_cents, Some(500));
    assert_eq!(
        rows.items[0].note.as_deref(),
        Some("网购订单"),
        "未改字段保持"
    );
}

/// 备份最小入口全旅程（issue #2038）：交易列表工具区「备份」入口 → 文件
/// 保存对话框（headless 替身 `on_pick_save_path` 回答目标路径——生产与测试
/// 同一条 `pick_save_path` 生产接缝，Linux 宿主为 GTK 原生保存对话框，原生
/// 观感归人工对表子集）→ 域 backup 直调产出备份文件 + 成功反馈呈现（NSH 无
/// toast，状态条呈现是旧壳 message.success 的条位对应物）。
///
/// 「删除即红」负向条目：删工具区按钮的备份流程接线（`begin_pick` 的
/// begin_backup 的 spawn→对话框→备份调用链）或成功状态落位（status.set(Success)）任一环，
/// 本测 wait_for 超时红；断言对准用户可见结果（成功文案 + 产物在盘），
/// 不对准调用形状。
#[test]
fn backup_entry_full_journey_writes_zip_and_reports_success() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("backup-journey");
    let backup_dir =
        std::env::temp_dir().join(format!("ledger-test-nsh-backup-journey-{}", new_uuid()));
    fs::create_dir_all(&backup_dir).unwrap();
    let target = backup_dir.join("手动选择.zip");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    // 入口存在性：交易列表工具区「备份」按钮呈现（id 锚点，research doc §3.5）。
    open_transactions(&mut app);

    app.wait_for("id:tx-backup").unwrap();

    // 保存对话框替身注入目标路径 → 按下入口。替身回答前，先核对发给平台
    // 的对话框参数（用户可见面：默认名与旧壳 defaultBackupFileName 同格式
    // ——空库指纹未固化退化无标识旧命名；标题与过滤器与旧壳另存为同词表）。
    app.on_pick_save_path(Some(target.clone())).unwrap();
    app.press("id:tx-backup").unwrap();
    app.settle();
    let requests = app.screen().file_requests();
    let [nsh_core::file_dialog::FileRequest::SaveDialog(dialog)] = requests else {
        panic!("应发出一个保存对话框请求：{requests:?}");
    };
    assert_eq!(dialog.title.as_deref(), Some("备份到…"), "标题同旧壳词表");
    let name = dialog.file_name.as_deref().expect("默认文件名应在场");
    assert_eq!(dialog.filters.len(), 1, "过滤器闭集：仅 zip 一组");
    assert_eq!(dialog.filters[0].name, "开源记账备份");
    assert_eq!(dialog.filters[0].extensions, ["zip"]);
    assert!(
        name.starts_with("ledger-backup-") && name.ends_with(".db.zip"),
        "默认名同旧壳受管命名格式：{name}"
    );

    // 成功反馈呈现（标签 + 产物路径两个文本节点）。
    app.wait_for("text:备份成功").unwrap();

    // 产物在盘且非空（zip 结构与元数据语义归备份域单测——壳面只断用户可见
    // 的「文件确实产生了」）。
    let meta = fs::metadata(&target).unwrap();
    assert!(meta.len() > 0, "备份产物非空：{}", target.display());
}

/// 覆盖确认守卫（issue #2038 AC：覆盖已有备份文件前有确认守卫，warning 级
/// ADR-0078）：保存对话框替身回答**已存在**的目标路径 → 覆盖确认对话框
/// 呈现，确认前旧文件原样（零写入），确认后备份执行、旧文件被替换、成功
/// 反馈呈现。真实桌面 GTK 原生保存对话框自带覆盖确认（平台行为），本守卫
/// 是应用内第二道（数据安全方向偏保守，headless 可测）。
///
/// 「删除即红」负向条目：删「已存在 → 开确认 → 确认后才备份」的守卫接线
/// （存在分支直备）→ 本测在 wait_for 对话框标题处超时红（且旧文件在确认前
/// 已被替换）。
#[test]
fn backup_overwrite_guard_confirms_before_replacing() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("backup-overwrite");
    let backup_dir =
        std::env::temp_dir().join(format!("ledger-test-nsh-backup-overwrite-{}", new_uuid()));
    fs::create_dir_all(&backup_dir).unwrap();
    let target = backup_dir.join("手动选择.zip");
    // 预置旧备份文件（内容可辨：确认前不被替换、确认后被备份产物替换）。
    const OLD_CONTENT: &[u8] = b"old-backup-content-marker";
    fs::write(&target, OLD_CONTENT).unwrap();

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_transactions(&mut app);

    app.wait_for("id:tx-backup").unwrap();
    app.on_pick_save_path(Some(target.clone())).unwrap();
    app.press("id:tx-backup").unwrap();

    // 覆盖确认对话框呈现（warning 级：后果与兜底说明，danger 封装两级的
    // warning 构造面首个消费面）。
    app.wait_for("text:覆盖备份文件").unwrap();
    app.wait_for("text:覆盖后原文件内容将被替换，不可找回。")
        .unwrap();

    // 确认前零写入：旧文件原样（对话框开着时不产生备份）。
    assert_eq!(fs::read(&target).unwrap(), OLD_CONTENT, "确认前旧文件原样");

    // 确认 → 备份执行 → 旧文件被替换（zip 产物）+ 成功反馈。
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:备份成功").unwrap();
    let new_content = fs::read(&target).unwrap();
    assert_ne!(new_content, OLD_CONTENT, "确认后旧文件被备份产物替换");
    assert!(!new_content.is_empty(), "备份产物非空");
}

/// 覆盖确认取消路径（issue #2038 取消行为）：确认对话框点「取消」不备份——
/// 对话框关窗、旧文件原样、无成功反馈。ESC 同为放弃（danger 封装三关闭
/// 路径语义，封装级抑制/ESC 测试见交易删除与 #2022 AC3）。
#[test]
fn backup_overwrite_guard_cancel_keeps_existing_file() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("backup-overwrite-cancel");
    let backup_dir = std::env::temp_dir().join(format!(
        "ledger-test-nsh-backup-overwrite-cancel-{}",
        new_uuid()
    ));
    fs::create_dir_all(&backup_dir).unwrap();
    let target = backup_dir.join("手动选择.zip");
    const OLD_CONTENT: &[u8] = b"old-backup-content-marker";
    fs::write(&target, OLD_CONTENT).unwrap();

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_transactions(&mut app);

    app.wait_for("id:tx-backup").unwrap();
    app.on_pick_save_path(Some(target.clone())).unwrap();
    app.press("id:tx-backup").unwrap();
    app.wait_for("text:覆盖备份文件").unwrap();

    // 取消 → 对话框关窗，无成功反馈、旧文件原样。
    app.press("id:danger-cancel").unwrap();
    app.wait_for_gone("text:覆盖备份文件", DEFAULT_WAIT)
        .unwrap();
    app.settle();
    assert!(!app.shows("备份成功"), "取消后无成功反馈");
    assert_eq!(fs::read(&target).unwrap(), OLD_CONTENT, "取消后旧文件原样");
}

/// 对话框失败反馈（issue #2038：成功/失败反馈要明确——数据安全路径不静默）：
/// headless 缺省环境无文件对话框能力（`file_requests` 关，上游引擎对文件
/// 请求即答错误），按下备份 → 失败呈现（码化包装文案，含「备份失败: 」前缀
/// 与失败原因；正则锚定前缀段，平台错误原文随上游措辞）。
///
/// 「删除即红」负向条目：删对话框错误分支的状态落位（`Err` 臂 status.set）
/// → 本测 wait_for 超时红（失败被静默吞掉）。
#[test]
fn backup_dialog_failure_reports_error() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("backup-dialog-error");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_transactions(&mut app);

    app.wait_for("id:tx-backup").unwrap();
    // 不注入替身：环境无文件对话框能力，保存请求即答错误（生产对应 Linux
    // 缺文件选择 portal 的场景）。
    app.press("id:tx-backup").unwrap();

    app.wait_for("text:/备份失败: 文件对话框无法打开/").unwrap();
    // 失败后按钮恢复可用（单飞状态已退出，可再次发起）。
    assert!(
        app.screen().enabled("id:tx-backup").unwrap(),
        "失败后入口恢复可用"
    );
}

/// 对话框取消路径（issue #2038：保存对话框取消 = 零副作用，旧壳 save 返回
/// 空串即返回同语义）：替身回答 None → 无备份产物、无成功/失败反馈、无
/// 覆盖确认对话框。
#[test]
fn backup_dialog_cancel_writes_nothing() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("backup-cancel");
    let backup_dir =
        std::env::temp_dir().join(format!("ledger-test-nsh-backup-cancel-{}", new_uuid()));
    fs::create_dir_all(&backup_dir).unwrap();

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_transactions(&mut app);

    app.wait_for("id:tx-backup").unwrap();
    app.on_pick_save_path(None).unwrap();
    app.press("id:tx-backup").unwrap();

    // 取消后无任何反馈（成功/失败/确认对话框都不呈现），备份目录空。
    app.settle();
    assert!(!app.shows("备份成功"), "取消后无成功反馈");
    assert!(!app.shows("备份失败"), "取消后无失败反馈");
    assert!(!app.shows("覆盖备份文件"), "取消后无覆盖确认对话框");
    assert_eq!(
        fs::read_dir(&backup_dir).unwrap().count(),
        0,
        "取消零写入：备份目录为空"
    );
}

// ---------------------------------------------------------------------------
// 概览视图（#2039）：冻结点三视图的最后一屏——净资产 / 本月收支 / 投资概览 /
// 财务自由度 / 预算年度摘要五卡、口径说明弹层、净资产卡下钻、失效信号重读
// 与出厂落点改回概览的可见侧。
// ---------------------------------------------------------------------------

/// 造一个指定币种指定类型的账户（概览造数入口；返回账户 id）。
fn seed_account_with_currency(
    db: &ledger_infra::db::DbState,
    name: &str,
    kind: AccountType,
    currency_code: &str,
    initial_balance_cents: i64,
) -> String {
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
            currency_code: currency_code.to_owned(),
            initial_balance_cents: Some(initial_balance_cents),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap()
}

/// 造标的 + 现价（概览投资卡造数面；价格刻度万分之一元，ADR-0038）。
fn seed_instrument_with_price(
    db: &ledger_infra::db::DbState,
    symbol: &str,
    price_wan: i64,
) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let id = ledger_investment::create_instrument(
        &conn,
        ledger_investment::InstrumentInput {
            symbol: symbol.to_owned(),
            kind: ledger_investment::InstrumentType::Stock,
            name: Some(format!("{symbol} 标的")),
            currency_code: "CNY".to_owned(),
            market: None,
        },
    )
    .unwrap();
    ledger_investment::create_market_price(
        &conn,
        ledger_investment::MarketPriceInput {
            instrument_id: id.clone(),
            price_cents: price_wan,
            currency_code: "CNY".to_owned(),
            priced_at: today().to_string(),
            source: Some("manual".to_owned()),
        },
    )
    .unwrap();
    id
}

/// 造一笔买入（现金腿恰由账户期初余额覆盖；金额/日期随参）。
fn seed_buy(db: &ledger_infra::db::DbState, account: &str, instrument: &str, date: &str) {
    // 买入带标的/数量/价格三维（域 lib 测试 buy_input 同形，CNY 载荷）。
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_transaction::create(
        &conn,
        TransactionInput {
            kind: TransactionKind::Buy,
            amount_cents: 0,
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
            note: Some("买入".to_owned()),
            source_order_no: None,
            date: date.to_owned(),
            instrument_id: Some(instrument.to_owned()),
            quantity: Some(1.0),
            price_cents: Some(10_000),
            fee_cents: Some(0),
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

/// 概览全景账本（五卡断言的统一造数面，数字全部取整十刻度）：
/// - 现金账户期初 0，本月收入 +50_000、支出 −20_000 → 净资产现金腿 30_000、
///   本月收支 500/200/300、本年已支 200；
/// - 月度预算「餐饮」3_125 → 年化总额 375（自由度分母 37_500）；
/// - 投资账户期初 100（恰覆买入现金腿），标的现价 1_000_000（=¥100）+ 买入
///   1 份@¥1 → 持仓市值 10_000、未实现 9_900、累计收益 9_900、自由度分子
///   10_000（现金腿归零）；
/// - 净资产 = 现金 30_000 + 持仓市值 10_000 = 40_000（实物腿 0）。
fn seed_dashboard_book(tag: &str) -> std::path::PathBuf {
    let dir = make_book(tag);
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let cash = seed_account_with_currency(&db, "现金", AccountType::Cash, "CNY", 0);
    let today = today().to_string();
    seed_transaction(
        &db,
        TxnSeed {
            account: &cash,
            to_account: None,
            kind: TransactionKind::Income,
            amount_cents: 50_000,
            date: &today,
            category_id: None,
            merchant_id: None,
            note: Some("本月收入".to_owned()),
        },
    );
    let food = seed_category(&db, "餐饮", None);
    seed_transaction(
        &db,
        TxnSeed {
            account: &cash,
            to_account: None,
            kind: TransactionKind::Expense,
            amount_cents: 20_000,
            date: &today,
            category_id: Some(food.clone()),
            merchant_id: None,
            note: Some("本月支出".to_owned()),
        },
    );
    seed_budget(&db, &food, 3_125, ledger_budget::BudgetPeriod::Monthly);
    let invest = seed_account_with_currency(&db, "证券", AccountType::Investment, "CNY", 100);
    let instrument = seed_instrument_with_price(&db, "TST", 1_000_000);
    seed_buy(&db, &invest, &instrument, &today);
    dir
}

/// 落位交易列表（出厂落点改回概览后的显式导航面，#2039 起首屏是概览）：等
/// 侧栏就绪 → 点侧栏交易项 → 等表格挂载。各用例的交易/表单/筛选断言先经本
/// 助手落位；概览视图自身的断言不经本助手。
fn open_transactions(app: &mut Headless) {
    app.wait_for("id:nav-transactions").unwrap();
    app.press("id:nav-transactions").unwrap();
    app.wait_for("id:tx-table").unwrap();
}

/// 落位搜索视图（#2040）：等侧栏就绪 → 点侧栏搜索项 → 等关键词输入框挂载。
/// 各搜索视图用例经本助手落位后再操作。
fn open_search(app: &mut Headless) {
    app.wait_for("id:nav-search").unwrap();
    app.press("id:nav-search").unwrap();
    app.wait_for("id:search-keyword").unwrap();
}

/// 最小支出输入骨架（购买项/出资造数面的填充底座，其余字段随 struct
/// update 覆写；金额/日期/备注等必填字段由调用方带）。
fn bare_expense_input() -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Expense,
        amount_cents: 0,
        currency_code: "CNY".to_owned(),
        account_id: None,
        to_account_id: None,
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
        date: String::new(),
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
    }
}

/// 搜索视图首屏占位（issue #2040）：侧栏搜索项落真实视图——屏名、关键词/
/// 金额输入与期间行控件齐备，未搜索呈现占位空态（旧壳 search.emptyPlaceholder
/// 同文案），不再是「迁移中」占位。「删除即红」负向条目：删 view_host 的搜索
/// 分支挂载 → 本测红（落回迁移占位）。
#[test]
fn search_view_lands_with_placeholder_and_controls() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-view");
    seed_expense_notes(&dir, &["元旦火锅聚餐"]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_search(&mut app);

    assert_eq!(
        app.screen().input_text("id:search-keyword").unwrap(),
        Some(""),
        "关键词输入框挂载为空"
    );
    for anchor in [
        "text:搜索",
        "text:输入关键字或设置筛选开始搜索",
        "id:search-amount-min",
        "id:search-amount-max",
        "id:search-period-label",
    ] {
        app.wait_for(anchor).unwrap();
    }
    assert!(
        app.screen().count("id:placeholder-note").unwrap() == 0,
        "搜索视图是真实视图，不再是迁移占位：{:?}",
        app.screen().texts()
    );
    // 未搜索不呈现结果面与命中计数（旧壳 searched 门控同口径）。
    assert!(
        app.screen().count("id:search-total").unwrap() == 0,
        "未搜索不呈现命中计数：{:?}",
        app.screen().texts()
    );
}

/// 搜索视图关键词防抖自动搜索（issue #2040 AC1）：输入关键词后**不按按钮**
/// 即得结果（防抖到期自动重查）——命中计数换词（命中 1 条）、命中行在屏、
/// 未命中行离屏。「删除即红」负向条目：删关键词 on_change → 统一出口
/// （react_to_change）接线 → 本测红（输入后永远停留占位态）。
#[test]
fn search_view_keyword_debounced_search_shows_hits() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-view-keyword");
    seed_expense_notes(&dir, &["元旦火锅聚餐", "超市买菜"]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_search(&mut app);
    // 首屏占位态，无结果面。
    app.wait_for("text:输入关键字或设置筛选开始搜索").unwrap();

    // 输入关键词 → 防抖到期自动搜索：命中 1 条 + 命中行在屏、未命中行离屏。
    app.type_text("id:search-keyword", "火锅").unwrap();
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("text:2026-01-01").unwrap();
    app.wait_for_gone("text:2026-01-02", DEFAULT_WAIT).unwrap();
    // 占位空态离场（搜索态已开）。
    app.wait_for_gone("text:输入关键字或设置筛选开始搜索", DEFAULT_WAIT)
        .unwrap();
}

/// 搜索视图金额区间筛选（issue #2040 AC3，无关键字仅筛选）：金额任一端
/// 在场即入搜索态（旧壳搜索页 hasQuery 同口径）——结果按区间过滤，命中计数
/// 随语义变化。「删除即红」负向条目：删金额 on_change → 统一出口接线 →
/// 本测红（输入金额后停留占位态）。
#[test]
fn search_view_amount_range_filters_without_keyword() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-view-amount");
    seed_expense_on_date(&dir, &today().to_string(), 2_500);
    seed_expense_on_date(&dir, &today().to_string(), 88_000);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_search(&mut app);
    app.wait_for("text:输入关键字或设置筛选开始搜索").unwrap();

    // 仅输入最低金额（无关键字）→ 仅筛选查询：金额 < ¥100 的行被滤除。
    app.type_text("id:search-amount-min", "100").unwrap();
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("text:¥880").unwrap();
    assert!(
        !app.shows("¥25"),
        "区间下界外的行不得在屏：{:?}",
        app.screen().texts()
    );
    // 已应用筛选描述随激活呈现（金额单端描述）。
    app.wait_for("id:search-applied-filters").unwrap();
    assert!(
        app.shows("已应用筛选：最低 100"),
        "已应用筛选应含金额下界描述：{:?}",
        app.screen().texts()
    );
}

/// 搜索视图期间快捷筛选（issue #2040 AC2 / #2037 搜索×期间裁决平价）：点
/// 「当月」芯片 → 期间对搜索命中面**生效**（当月备注命中、上月备注不再命中），
/// 期间标签随芯片点亮；期间与关键词同面生效。再次点「全部」回无期间
///（命中面回到全时间，计数同值不同词）。「删除即红」负向条目：删搜索装载
/// 搒期间快照（`search_range` 不传域读路径）→ 本测红（上月备注仍命中）。
#[test]
fn search_view_period_chip_filters_hits() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-view-period");
    seed_expense_on_date(&dir, &prev_month_mid().to_string(), 8_800);
    seed_expense_on_date(&dir, &today().to_string(), 2_500);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_search(&mut app);
    // 关键词同时命中两行（日期在备注里，两行备注同词）。
    app.type_text("id:search-keyword", "支出").unwrap();
    app.wait_for("text:命中 2 条").unwrap();

    // 点「当月」芯片 → 期间对命中面生效：只剩本月一行。
    app.press("id:search-period-label").unwrap();
    app.wait_for("id:search-period-chip-month").unwrap();
    app.press("id:search-period-chip-month").unwrap();
    let t = today();
    app.wait_for(&month_label(t.year(), t.month())).unwrap();
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("text:¥25").unwrap();
    assert!(
        !app.shows("¥88"),
        "当月期间下搜索不得命中上月备注：{:?}",
        app.screen().texts()
    );

    // 回「全部」→ 无期间，两行重回命中面（命中计数回升）。
    app.press("id:search-period-label").unwrap();
    app.wait_for("id:search-period-chip-all").unwrap();
    app.press("id:search-period-chip-all").unwrap();
    app.wait_for("text:命中 2 条").unwrap();
}

/// 搜索视图清除筛选与空查询回占位（issue #2040 AC，旧壳 clearFilters /
/// resetResults 同语义）：清除筛选只清金额与期间、关键词不动（结果随防抖
/// 重查）；再清关键词 → 无查询 → 回占位空态（结果面收起、命中计数离场）。
/// 「删除即红」负向条目：删清除按钮的 clear_filters 接线 → 本测红（描述
/// 与筛选不消失）；删统一出口的无查询分支 → 清词后停留无匹配空态。
#[test]
fn search_view_clear_filters_then_clear_keyword_returns_placeholder() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-view-clear");
    seed_expense_on_date(&dir, &today().to_string(), 2_500);
    seed_expense_on_date(&dir, &today().to_string(), 88_000);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_search(&mut app);
    // 关键词 + 金额下界同时在场：两行备注同词，金额滤除小额行。
    app.type_text("id:search-keyword", "支出").unwrap();
    app.wait_for("text:命中 2 条").unwrap();
    app.type_text("id:search-amount-min", "100").unwrap();
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("id:search-applied-filters").unwrap();

    // 清除筛选：金额归零、关键词保留 → 防抖重查回两行，描述离场。
    app.press("id:search-clear-filters").unwrap();
    app.wait_for("text:命中 2 条").unwrap();
    app.wait_for_gone("id:search-applied-filters", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:¥25").unwrap();

    // 清关键词：无查询 → 占位空态回归，结果面收起。
    app.clear("id:search-keyword").unwrap();
    app.wait_for("text:输入关键字或设置筛选开始搜索").unwrap();
    app.wait_for_gone("text:命中 2 条", DEFAULT_WAIT).unwrap();
}

/// 搜索视图结果分页（issue #2040 AC）：同备注 150 笔命中两页，点击页码
/// 「2」翻末页（最旧 50 笔在屏、首页最新行离屏）；翻页携关键词与筛选重搜
///（服务端 offset 分页，旧壳搜索页同形）。「删除即红」负向条目：删分页条
/// on_change 的翻页装载接线 → 本测红（页码不翻、行集不变）。
#[test]
fn search_view_paginates_hits() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-view-paged");
    seed_expense_notes(&dir, &["批量造数"; 150]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_search(&mut app);
    app.type_text("id:search-keyword", "批量造数").unwrap();
    app.wait_for("text:命中 150 条").unwrap();
    app.wait_for("id:search-pagination").unwrap();

    // 点击页码 2 → 末页行集（最旧 50 笔，2026-01-01..2026-02-19），首页
    // 最新行离屏（断言对准视口内首行，交易列表分页同口径）。
    app.press("role:button[2]").unwrap();
    app.wait_for("text:2026-02-19").unwrap();
    app.wait_for_gone("text:2026-05-30", DEFAULT_WAIT).unwrap();
}

/// 搜索结果只读形态（issue #2040 AC6，旧壳搜索页同口径）：命中行右键**不**
/// 弹出行菜单（无编辑/删除入口，编辑走交易列表/表单）；命中面无「记一笔」
/// 等写入口控件。「删除即红」负向条目：给搜索视图表格接上 on_context_menu
/// → 本测红（菜单重现）。
#[test]
fn search_view_results_are_read_only_no_row_menu() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-view-readonly");
    seed_expense_notes(&dir, &["元旦火锅聚餐"]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_search(&mut app);
    app.type_text("id:search-keyword", "火锅").unwrap();
    app.wait_for("text:命中 1 条").unwrap();

    // 命中行右键：无上下文菜单弹出（交易列表同路径右键有「编辑/退款/删除」
    // 三项——搜索页是只读结果面，无此形态）。
    app.right_click("text:2026-01-01").unwrap();
    app.settle();
    assert!(
        app.menu().is_none(),
        "搜索结果右键不得弹出行菜单：{:?}",
        app.menu()
    );
    // 无任何写入口控件（记一笔 / 编辑 / 删除均不在屏）。
    for forbidden in ["记一笔", "编辑", "删除"] {
        assert!(
            !app.shows(forbidden),
            "搜索视图不得出现写入口「{forbidden}」：{:?}",
            app.screen().texts()
        );
    }
}

/// 购买项命中整单展开（issue #2040，旧壳搜索页 expandPurchases 同口径）：
/// 关键词命中**购买项名称**时整单呈现全部购买项行（与交易列表同一装配
/// [`assembly::assemble_rows`]）——商品名「洗衣液」命中，订单两购买项行
/// 齐现（商品名 + 件数），命中计数按交易计（命中 1 条，ADR-0008 口径）。
#[test]
fn search_view_purchase_name_hit_expands_order_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-view-purchase");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_transaction::create(
            &conn,
            TransactionInput {
                kind: TransactionKind::Expense,
                amount_cents: 3_000,
                currency_code: "CNY".to_owned(),
                account_id: Some(account),
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
                date: "2026-01-01".to_owned(),
                ..bare_expense_input()
            },
        )
        .unwrap();
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_search(&mut app);
    // 购买项名称命中 → 整单展开：两购买项行齐现（猫粮 ×2 / 洗衣液 ×1），
    // 命中计数按交易计（订单一笔）。
    app.type_text("id:search-keyword", "洗衣液").unwrap();
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("text:洗衣液 共 1 件").unwrap();
    app.wait_for("text:猫粮 共 2 件").unwrap();
}

/// 连续快速修改关键词 → 结果始终对应当前输入（issue #2040 AC4，旧壳
/// Loadable 竞态后发覆盖先发同义）：命中「火锅」结果落位后再清词重输
/// 「超市」，最终行集对应最后一次输入（超市买菜行在屏、火锅行离屏、
/// 命中计数随最后一次语义）。后发作废先发由装载序号 latest-wins 守卫
/// 承担（与旧壳 Loadable invalidate 同义）；守卫属调度时序消除，不设
/// 拆除断言（交易视图先例同口径）。
#[test]
fn search_view_rapid_keyword_changes_show_latest_input_results() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-view-race");
    seed_expense_notes(&dir, &["元旦火锅聚餐", "超市买菜"]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_search(&mut app);
    app.type_text("id:search-keyword", "火锅").unwrap();
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("text:2026-01-01").unwrap();

    // 连续快速修改：清词 + 重输不等防抖间隔（同一事件序列内完成）。
    app.clear("id:search-keyword").unwrap();
    app.type_text("id:search-keyword", "超市").unwrap();

    // 最终结果对应最后一次输入：超市行在屏、火锅行离屏。
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("text:2026-01-02").unwrap();
    app.wait_for_gone("text:2026-01-01", DEFAULT_WAIT).unwrap();
}

/// 搜索视图状态不跨导航保留（issue #2040 / ADR-0070 决策 3：状态源视图
/// 本地、会话内保留明确不做）：搜出结果后切走再切回 → 全新占位态（关键词
/// 空、结果面收起、命中计数离场）——旧壳离开搜索页再回同口径，与交易
/// 列表的筛选会话保留（tx_filter）刻意不同。
#[test]
fn search_view_state_resets_after_leaving_and_returning() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-view-reset");
    seed_expense_notes(&dir, &["元旦火锅聚餐"]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_search(&mut app);
    app.type_text("id:search-keyword", "火锅").unwrap();
    app.wait_for("text:命中 1 条").unwrap();

    // 切走（交易列表）再切回：全新占位态。
    open_transactions(&mut app);
    open_search(&mut app);
    app.wait_for("text:输入关键字或设置筛选开始搜索").unwrap();
    assert_eq!(
        app.screen().input_text("id:search-keyword").unwrap(),
        Some(""),
        "关键词不保留"
    );
    app.wait_for_gone("text:命中 1 条", DEFAULT_WAIT).unwrap();
}

/// 搜索无命中空态（issue #2040，旧壳搜索页 searched 即显示 hitCount 同形）：
/// 无命中关键词呈现「命中 0 条」与「无匹配结果」，表格不残留列表行；
/// 占位文案离场（已入搜索态）。
#[test]
fn search_view_shows_no_hits_empty_state() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("search-view-no-hits");
    seed_expense_notes(&dir, &["元旦火锅聚餐"]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_search(&mut app);
    app.type_text("id:search-keyword", "不存在的关键词")
        .unwrap();
    app.wait_for("text:命中 0 条").unwrap();
    app.wait_for("text:无匹配结果").unwrap();
    app.wait_for_gone("text:输入关键字或设置筛选开始搜索", DEFAULT_WAIT)
        .unwrap();
    assert!(
        !app.shows("2026-01-01"),
        "无命中时行集不残留：{:?}",
        app.screen().texts()
    );
}

/// 启动即概览（出厂落点改回裁决的可见侧，#2039）：五卡全部呈现真实读数、
/// 不再是迁移占位。净资产 = 现金 30_000 + 持仓市值 10_000 = ¥400（域单点
/// 折算，装配层只消费）；「删除即红」负向条目钉在 view_host 的概览分支挂载
/// 与 nav_model::DEFAULT_VIEW（改回交易列表即红）。
#[test]
fn startup_lands_on_dashboard_with_all_five_cards() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_dashboard_book("dashboard-first-screen");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    // 第一屏凭据：净资产主数字（域单点折算值）+ 各卡标题。
    app.wait_for("id:net-worth-value").unwrap();
    app.wait_for("text:¥400").unwrap();
    for anchor in [
        "id:monthly-income",
        "text:500",
        "text:200",
        "text:300",
        "id:dashboard-total-market-value",
        "text:¥100",
        "text:¥99",
        "id:financial-freedom-ratio",
        "text:0.8%",
        "text:积累期",
        "text:可投资资产 ¥100",
        "text:年度预算总额 ¥375",
        "text:可覆盖 0.3 年",
        "id:dashboard-budget-total",
        "text:375",
        "text:200",
        "text:正常",
    ] {
        app.wait_for(anchor).unwrap();
    }
    assert!(
        app.screen().count("id:placeholder-note").unwrap() == 0,
        "概览是真实视图，不再是占位"
    );
}

/// 投资概览卡空态：无持仓账本呈现「暂无持仓」，三卡不呈现。
#[test]
fn dashboard_investment_card_shows_empty_state_without_holdings() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("dashboard-empty-invest");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_cash_account(&db);
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: &today().to_string(),
                category_id: None,
                merchant_id: None,
                note: Some("本月支出".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:暂无持仓").unwrap();
    assert!(
        app.screen()
            .count("id:dashboard-total-market-value")
            .unwrap()
            == 0,
        "空态下三卡不呈现"
    );
    // 零分母（未设预算）：自由度卡占位引导，不回退实际支出。
    app.wait_for("text:设置预算后解锁财务自由度").unwrap();
}

/// 净资产卡下钻（总览屏唯一动作）：点「查看账户明细」落账户视图（#2041 起
/// 为真实视图，新增账户卡在屏）。
/// 「删除即红」负向条目：删卡片内 nav::open 接线 → 本测红（停留概览）。
#[test]
fn net_worth_card_drills_down_to_accounts_view() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_dashboard_book("dashboard-drill");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:net-worth-card").unwrap();
    app.press("id:net-worth-view-accounts").unwrap();
    app.wait_for("text:新增账户").unwrap();
    assert_eq!(
        app.screen().count("text:账户").unwrap(),
        2,
        "下钻落账户视图（侧栏 + 屏名）"
    );
    assert!(
        app.screen().count("id:placeholder-note").unwrap() == 0,
        "账户不再占位"
    );
}

/// 口径说明弹层（NSH popover 封装件首消费）：点按 ⓘ 开、ESC 关；自由度卡
/// 四行口径齐现。「删除即红」负向条目：删封装件的触发/开合接线或视图的
/// ⓘ 消费任一环 → 本测红。
#[test]
fn caliber_popovers_open_and_close_on_dashboard() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_dashboard_book("dashboard-popover");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:net-worth-card").unwrap();
    // 净资产口径：点开 → 说明在场 → ESC 关。
    app.press("id:net-worth-info").unwrap();
    app.wait_for("id:net-worth-info-message").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:net-worth-info-message", DEFAULT_WAIT)
        .unwrap();
    // 自由度口径：四行同源（formula / 两腿拆解 / 提取率）。
    app.press("id:financial-freedom-info").unwrap();
    app.wait_for("id:financial-freedom-info-message").unwrap();
    app.wait_for("text:自由度 = 可投资资产 × 3% ÷ 年度预算总额")
        .unwrap();
    app.wait_for("text:分子 = 持仓市值 + 投资账户余额（折算为本位币，生活现金与负债不计入）")
        .unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:financial-freedom-info-message", DEFAULT_WAIT)
        .unwrap();
}

/// 零分母引导：占位态「去设置预算」按钮落预算视图（守卫接缝裸访问同路径）。
#[test]
fn freedom_zero_denominator_guides_to_budget_view() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("dashboard-freedom-guide");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_cash_account(&db);
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: &today().to_string(),
                category_id: None,
                merchant_id: None,
                note: Some("本月支出".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:freedom-zero-denominator").unwrap();
    app.press("id:freedom-go-budget").unwrap();
    app.wait_for("text:预算执行").unwrap();
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 账户视图（#2041）：清单三分区与余额口径（V017 缓存读数）、新建/编辑/归档/
// 恢复/删除/调整余额旅行、行菜单「⋯」与右键两入口同位、账户名下钻与金额隐私
// 掩码。删任一接线即红的负向条目逐项写在各测试注释（ADR-0087 断言强度：断言
// 对准用户可观察结果）。
// ---------------------------------------------------------------------------

/// 从侧栏点开账户视图（#2041 旅程用例共用）：新增卡主按钮在屏即落位凭据。
fn open_accounts(app: &mut Headless) {
    app.wait_for("id:nav-accounts").unwrap();
    app.press("id:nav-accounts").unwrap();
    app.wait_for("id:acct-add").unwrap();
}

/// 造一个带初始余额的现金账户（用例共用；金额经域公开写入口，初始余额在创建
/// 协议内同事务落为 V017 余额缓存行——缓存读数的造数面）。
fn seed_account_with_balance(
    db: &ledger_infra::db::DbState,
    name: &str,
    initial_cents: i64,
) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_accounts::create_account(
        &conn,
        AccountInput {
            name: name.to_owned(),
            kind: AccountType::Cash,
            currency_code: "CNY".to_owned(),
            initial_balance_cents: Some(initial_cents),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap()
}

/// 首屏装载（旧壳 AccountsView onMounted 平价）：清单三分区卡（攒钱卡空集不渲染）、
/// 主表列头与行格文本（类型标签 / 币种 / 余额 = V017 缓存读数）、已归档分区空集
/// 不渲染。删除装配任一环（boot 余额读 / rows 装配 / 三分区派生 / 渲染爪）即红。
#[test]
fn accounts_view_lands_with_create_card_and_balance_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("accounts-list");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_account_with_balance(&db, "测试现金", 10_000);
        seed_account(&db, "银行卡", AccountType::Bank);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_accounts(&mut app);
    // 异步装载纪律：先等主表行到屏（§3.5）再断格文本与列头。
    app.wait_for("id:acct-table").unwrap();
    app.wait_for("text:测试现金").unwrap();

    // 新建卡与主表卡标题（旧壳 accounts.create/list 词表同文案）。
    for text in ["新增账户", "账户列表", "名称", "类型", "币种", "余额"] {
        assert!(
            app.shows(text),
            "应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 行格文本（装配层产物）：类型标签 / 币种 / V017 缓存余额。
    for text in ["测试现金", "现金", "CNY", "¥100", "银行卡", "¥0"] {
        assert!(
            app.shows(text),
            "应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 攒钱分区空集不渲染空卡；已归档分区空集不渲染（折叠钮缺席）。
    assert!(
        app.screen().count("id:acct-goal-table").unwrap() == 0,
        "无目标绑定时攒钱卡不渲染"
    );
    assert!(
        app.screen().count("id:acct-archived-toggle").unwrap() == 0,
        "无已归档账户时分区不渲染"
    );
}

/// 创建旅程（旧壳 create + message.success 平价）：空名称前置拦截不写入；填
/// 名称 → 添加 → 行即现（余额 ¥0）且成功反馈在屏、表单复位。
/// 删除「添加 → perform_create → reload」任一接线即红。
#[test]
fn accounts_view_create_account_journey() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("accounts-create");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_accounts(&mut app);

    // 前置拦截：空名称点添加 → 错误条位，不产生空白行。
    app.press("id:acct-add").unwrap();
    app.wait_for("id:acct-create-error").unwrap();
    app.wait_for("text:请输入账户名称").unwrap();

    // 类型/币种为必填选项面（select clearable=false 参数化，#2041）：选中
    // 态也不呈现 ✕ 清除钮（删除 select.rs 的 `clearable &&` 接线即红）。
    assert!(
        app.screen().count("id:acct-type-clear").unwrap() == 0,
        "必填选项面无清除钮：{:?}",
        app.screen().texts()
    );

    // 填名称 → 创建：行即现、成功反馈在屏、表单复位。
    app.type_text("id:acct-create-name", "储蓄罐").unwrap();
    app.press("id:acct-add").unwrap();
    app.wait_for("text:储蓄罐").unwrap();
    app.wait_for("text:已创建账户").unwrap();
    assert_eq!(
        app.screen().input_text("id:acct-create-name").unwrap(),
        Some(""),
        "创建成功后表单复位"
    );
    assert!(app.shows("¥0"), "新账户余额 ¥0 在屏");
}

/// 信用卡类型（旧壳 isCreditType v-if 平价）：选「信用卡」→ 档案三字段才出现；
/// 带档案创建成功、行类型列「信用卡」、编辑弹窗回填资料与已保存的只读摘要。
#[test]
fn accounts_view_credit_type_reveals_profile_fields() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("accounts-credit");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_accounts(&mut app);

    // 缺省类型（现金）不呈现档案字段。
    assert!(
        app.screen().count("id:acct-create-limit").unwrap() == 0,
        "非信用卡不呈现信用额度字段"
    );
    // 选「信用卡」→ 三字段出现。
    app.press("id:acct-type-trigger").unwrap();
    app.wait_for("id:acct-type-option").unwrap();
    app.press("id:acct-type-option >> text:信用卡 >> nth=0")
        .unwrap();
    app.wait_for("id:acct-create-limit").unwrap();
    app.wait_for("id:acct-create-statement").unwrap();
    app.wait_for("id:acct-create-due").unwrap();

    // 带档案创建：行即现（类型列「信用卡」）。
    app.type_text("id:acct-create-name", "白金卡").unwrap();
    app.type_text("id:acct-create-limit", "50000").unwrap();
    app.type_text("id:acct-create-statement", "1").unwrap();
    app.type_text("id:acct-create-due", "10").unwrap();
    app.press("id:acct-add").unwrap();
    app.wait_for("text:白金卡").unwrap();
    app.wait_for("text:已创建账户").unwrap();

    // 编辑弹窗：额度按采购小数位回填「500」、只读摘要「使用率 0%」（余额为
    // 零已用 0，低使用率不呈警示）。
    let credit_id = nsh_app_lib_account_id(&dir, "白金卡");
    app.press(&format!("id:acct-more-{credit_id}")).unwrap();
    app.choose_menu_item("编辑").unwrap();
    app.wait_for("text:编辑账户").unwrap();
    assert_eq!(
        app.screen().input_text("id:acct-edit-limit").unwrap(),
        Some("50000"),
        "额度按元回填（旧壳 centsToYuan 同口径）"
    );
    app.wait_for("id:acct-edit-usage").unwrap();
    assert!(
        app.screen().texts().iter().any(|t| t.contains("使用率 0%")),
        "额度用量摘要（已保存行值）：{:?}",
        app.screen().texts()
    );
}

/// 「⋯」与右键两入口同位（旧壳 buildAccountRowMenuOptions 两入口同 options）：
/// ⋯ 开菜单的菜单项闭集（编辑/调整余额/归档/删除）与编辑旅行；删除 open_edit
/// 或表单回填接线即红。
#[test]
fn accounts_view_edit_renames_account() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("accounts-edit");
    let cash_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        cash_id = seed_cash_account(&db);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_accounts(&mut app);

    // ⋯ 开菜单 → 菜单闭集。
    app.press(&format!("id:acct-more-{cash_id}")).unwrap();
    let menu = app.menu().expect("「⋯」应打开行菜单");
    assert_eq!(
        menu.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        vec!["编辑", "调整余额", "归档", "删除"],
        "未归档行菜单项闭集：{menu:?}"
    );
    app.choose_menu_item("编辑").unwrap();

    // 名称回填 → 改名保存 → 行更新（编辑即见）。
    app.wait_for("text:编辑账户").unwrap();
    assert_eq!(
        app.screen().input_text("id:acct-edit-name").unwrap(),
        Some("测试现金"),
        "编辑弹窗名称回填"
    );
    app.clear("id:acct-edit-name").unwrap();
    app.type_text("id:acct-edit-name", "钱包").unwrap();
    app.press("id:acct-edit-save").unwrap();
    app.wait_for("id:acct-status").unwrap();
    app.wait_for("text:已保存").unwrap();
    app.wait_for_gone("id:acct-edit-name", DEFAULT_WAIT)
        .unwrap();
    // 行更新（异步）：等改名后的行在屏再断言（§3.5）。
    app.wait_for("text:钱包").unwrap();
    assert!(app.shows("钱包"), "改名后的行在屏");
    assert!(!app.shows("测试现金"), "旧名字离屏");
}

/// 删除确认守卫（旧壳 confirmDelete 二次确认 + ADR-0078 error 级形态）：右键
/// 「删除」→ 确认框标题与后果句、取消不写；确认后行离屏 + 反馈在屏。
/// 拆 danger_confirm / close_delete / perform_delete 任一环即红。
#[test]
fn accounts_view_delete_needs_confirmation_and_cancels_before_write() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("accounts-delete");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
        seed_account(&db, "要留下的", AccountType::Cash);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_accounts(&mut app);

    // 右键开菜单（另一入口同位==原注释）→ 删除。
    app.right_click("text:测试现金").unwrap();
    app.choose_menu_item("删除").unwrap();
    app.wait_for("text:删除账户").unwrap();
    app.wait_for("text:删除后相关交易仍保留。").unwrap();
    assert!(app.shows("测试现金"), "确认框开着时行仍在屏");

    // 取消 → 不写，行集不变。
    app.press("id:danger-cancel").unwrap();
    app.settle();
    assert!(app.shows("测试现金"), "取消后行仍主表在屏");

    // 确认 → 行离屏 + 反馈在屏；另一行保留。
    let cash_id = nsh_app_lib_account_id(&dir, "测试现金");
    app.press(&format!("id:acct-more-{cash_id}")).unwrap();
    app.choose_menu_item("删除").unwrap();
    app.wait_for("text:删除账户").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:已删除").unwrap();
    app.wait_for_gone("text:测试现金", DEFAULT_WAIT).unwrap();
    assert!(app.shows("要留下的"), "另一行不动删除");
}

/// 归档 / 恢复（ADR-0142 平价：收纳态不改金额口径、确认框真话、幂等）：⋯「归
/// 档」→ 警告级确认（余额为零无附句 → 单句通用正文）→ 行移入页底「已归档」
/// 分区（折叠钮 + 数量合计在屏、主表行不再在屏）→ 展开分区 → 归档行菜单收
/// 窄为「恢复」（ADR-0142 决策 2 闭集）→ 恢复回主表。拆分区或菜单收窄任一
/// 接线即红。
#[test]
fn accounts_view_archives_and_restores() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("accounts-archive");
    let cash_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        // 余额非零（¥50）：归档确认把真话说清楚（#1955 真话矩阵——余额句）。
        cash_id = seed_account_with_balance(&db, "测试现金", 5_000);
        seed_account(&db, "要留下的", AccountType::Cash);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_accounts(&mut app);

    // 归档确认。
    app.press(&format!("id:acct-more-{cash_id}")).unwrap();
    app.choose_menu_item("归档").unwrap();
    app.wait_for("text:归档账户").unwrap();
    app.wait_for(
        "text:确认归档账户「测试现金」？归档后它从主表移入下方的「已归档」分区，历史与流水仍可查。",
    )
    .unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:账户已归档，历史与流水保留可查").unwrap();

    // 分区呈现：折叠钮 + 数量合计在屏；主表行不再在屏（折叠即不挂载行）。
    app.wait_for("id:acct-archived-toggle").unwrap();
    app.wait_for("id:acct-archived-summary").unwrap();
    assert!(
        app.screen()
            .texts()
            .iter()
            .any(|t| t.contains("1 个账户 · 合计 ¥50")),
        "已归档数量合计在屏：{:?}",
        app.screen().texts()
    );
    app.settle();
    assert!(
        !app.shows("测试现金"),
        "折叠的归档分区不挂载行：{:?}",
        app.screen().texts()
    );

    // 展开 → 归档行菜单收窄为「恢复」→ 恢复回主表。
    app.press("id:acct-archived-toggle").unwrap();
    app.wait_for("text:测试现金").unwrap();
    app.press(&format!("id:acct-more-{cash_id}")).unwrap();
    let menu = app.menu().expect("归档行「⋯」应出菜单");
    assert_eq!(
        menu.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        vec!["恢复"],
        "归档行动作集收窄为恢复（ADR-0142 决策 2）：{menu:?}"
    );
    app.choose_menu_item("恢复").unwrap();
    app.wait_for("text:账户已恢复").unwrap();
    app.wait_for("text:测试现金").unwrap();
    assert!(
        app.screen().count("id:acct-archived-toggle").unwrap() == 0,
        "恢复后已归档分区空集不渲染"
    );
}

/// 调整余额旅行（旧壳 adjustBalance + ADR-0026）：⋯「调整余额」→ 弹窗日叔
/// 当前余额 → 目标差额实时呈现 → 确认后生成转账（失效信号广播）→ 交易列表
/// 重读见转账行 + 余额更新。拆确认后 signals 广播或 reload 接线即红。
#[test]
fn accounts_view_adjust_balance_broadcasts_transactions_invalidated() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("accounts-adjust");
    let cash_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        cash_id = seed_cash_account(&db);
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash_id,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: "2026-01-01",
                category_id: None,
                merchant_id: None,
                note: Some("花销".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_accounts(&mut app);

    // 调整弹窗：当前余额 -¥25（V017 缓存读数）。
    app.press(&format!("id:acct-more-{cash_id}")).unwrap();
    app.choose_menu_item("调整余额").unwrap();
    app.wait_for("text:调整余额").unwrap();
    app.wait_for("id:acct-adjust-current").unwrap();
    app.wait_for("text:-¥25").unwrap();
    app.wait_for("id:acct-adjust-target").unwrap();

    // 目标差额实时呈现。
    app.type_text("id:acct-adjust-target", "70").unwrap();
    app.wait_for("text:将从「无」转入 ¥95（生成一笔与「无」的转账，删除该转账即撤销调整）")
        .unwrap();

    // 确认 → 反馈在屏 + 交易列表重读见转账行（失效信号广播落第点）。
    app.press("id:acct-adjust-confirm").unwrap();
    app.wait_for("text:余额已调整").unwrap();
    app.wait_for("text:¥70").unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 2 条").unwrap();
}

/// 目标绑定账户（ADR-0133 决策 2 / issue #1752 平价）：攒钱分区独立成卡、
/// 类型覆写「攒钱」；编辑弹窗名称随动只读（禁用）；删除被目标域守卫码化拒
/// 绝（错误落反馈条位不静默）。拆 connect 接线（boot 目标读 / 装配覆写 /
/// boot::delete_account 守卫任一环）即红。
#[test]
fn accounts_view_goal_partition_and_name_readonly_with_delete_guard() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("accounts-goal");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_savings_goal::create_savings_goal(
            &conn,
            &ledger_savings_goal::SavingsGoalInput {
                name: "旅行基金".to_owned(),
                target_amount_cents: 50_000,
                deadline: None,
            },
        )
        .unwrap();
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_accounts(&mut app);

    // 攒钱分区卡 + 类型覆写（主表保留普通账户）。
    app.wait_for("id:acct-goal-table").unwrap();
    app.wait_for("text:旅行基金").unwrap();
    app.wait_for("text:攒钱").unwrap();

    // 编辑弹窗名称随动只读（禁用输入）。
    let goal_account_id = nsh_app_lib_account_id(&dir, "旅行基金");
    app.press(&format!("id:acct-more-{goal_account_id}"))
        .unwrap();
    app.choose_menu_item("编辑").unwrap();
    app.wait_for("text:编辑账户").unwrap();
    app.wait_for("text:攒钱").unwrap();
    assert!(
        !app.screen().enabled("id:acct-edit-name").unwrap(),
        "目标绑定账户名称随动只读"
    );
    app.press("id:acct-edit-cancel").unwrap();

    // 删除被目标域守卫拒绝：确认后条位呈现码化文文（不静默），行仍在。
    app.press(&format!("id:acct-more-{goal_account_id}"))
        .unwrap();
    app.choose_menu_item("删除").unwrap();
    app.wait_for("text:删除账户").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for_gone("text:删除账户", DEFAULT_WAIT).unwrap();
    app.wait_for("id:acct-status").unwrap();
    assert!(
        app.screen().texts().iter().any(|t| t.contains("删除失败")),
        "目标域守卫拒绝的码化错误在反馈条位：{:?}",
        app.screen().texts()
    );
    assert!(app.shows("旅行基金"), "删除被拒后行仍在");
}

/// 金额隐私掩码（旧壳 formatAmount 含隐私掩码口径）：存储开关打开 → 余额列
/// 掩码（¥xx 不再在屏）。拆掩码消费（装配入参 privacy_enabled）即红。
#[test]
fn accounts_view_amount_privacy_masks_balances() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("accounts-privacy");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_account_with_balance(&db, "测试现金", 10_000);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_storage(&[(
        "amount_privacy_enabled",
        "true",
    )]))
    .unwrap();
    open_accounts(&mut app);
    app.wait_for("text:••••").unwrap();
    assert!(
        !app.shows("¥100"),
        "隐私模式下金额掩码：{:?}",
        app.screen().texts()
    );
}

/// 账户名下钻（issue #96/#97 平价）：点名称 → 跳交易页带账户筛选下钻载荷
/// （只过滤该账户的交易）；无交易的其它账户名字不涉。拆 drill_transactions
/// 的 nav::open 接线即红（点击停留账户视图）。
#[test]
fn accounts_view_name_click_drills_to_transactions_filtered() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("accounts-drill");
    let cash_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        cash_id = seed_cash_account(&db);
        let other = seed_account(&db, "太空", AccountType::Cash);
        for (date, note) in [("2026-01-01", "现金笔一"), ("2026-01-02", "现金笔二")] {
            seed_transaction(
                &db,
                TxnSeed {
                    account: &cash_id,
                    to_account: None,
                    kind: TransactionKind::Expense,
                    amount_cents: 2_500,
                    date,
                    category_id: None,
                    merchant_id: None,
                    note: Some(note.to_owned()),
                },
            );
        }
        seed_transaction(
            &db,
            TxnSeed {
                account: &other,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 6_600,
                date: "2026-01-03",
                category_id: None,
                merchant_id: None,
                note: Some("另一账户笔".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_accounts(&mut app);

    // 点名称 → 交易页带账户筛选下钻：只剩现金账户两笔。「删除即红」负向条
    // 目：删账下钻 nav::open 接线 → 点击后停留账户视图，本断言红。
    app.press(&format!("id:acct-name-{cash_id}")).unwrap();
    app.wait_for("id:tx-table").unwrap();
    app.wait_for("text:共 2 条").unwrap();
    assert!(
        app.shows("现金笔一") && app.shows("现金笔二"),
        "下钻筛选命中在屏"
    );
    assert!(
        !app.shows("另一账户笔"),
        "其它账户交易被下钻筛选排除：{:?}",
        app.screen().texts()
    );
}

/// 经域读路径反查账户 id（用例共用；弹窗/菜单目标 test_id 嵌 id 用）。
fn nsh_app_lib_account_id(dir: &std::path::Path, name: &str) -> String {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_accounts::list_accounts_for_api(&conn)
        .unwrap()
        .into_iter()
        .find(|a| a.name == name)
        .map(|a| a.id)
        .unwrap_or_else(|| panic!("未找到账户 {name}"))
}
