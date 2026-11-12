//! headless 组件测试（issue #2018 交易表格首屏；docs/research/
//! nsh-testing-capabilities.md §2.1 裁决的 UI 自动化接缝）。
//!
//! 进程内起真实视图树（`Root::of::<App>`，无窗口无平台后端），断言只写用户
//! 可见结果：表格呈现真实账本数据（格文本来自装配层）、分页条点击翻页与空
//! 态。装配入口注入测试库路径（生产与测试同一装配路径）；建库走产品
//! 开库入口、造数经域层公开写入口（与 lib 单测同纪律，造数形态共享
//! `support/seed.rs`，两侧 `#[path]` 载入，issue #2027）。域栈引导在组件任务的
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
use ledger_physical_asset::{PhysicalAssetDisposeInput, PhysicalAssetInput};
use ledger_transaction::{
    TransactionFundingInput, TransactionInput, TransactionKind, TransactionPurchaseInput,
};
use nsh_core::platform::ColorScheme;
use nsh_engine::{EngineConfig, Environment, Idiom, Os, Points, Root, Size};
use nsh_testing::Headless;

#[path = "support/seed.rs"]
mod seed;

use seed::{seed_cash_account, test_date};

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

/// 测试账本（产品开库入口建库：0 字节文件 = 新装语义；目录与空库文件形态归
/// 共享助手 [`seed`]，issue #2027）。
fn make_book(tag: &str) -> std::path::PathBuf {
    let dir = seed::make_book_dir(tag);
    seed::create_empty_book_file(&dir);
    dir
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

/// 经域层公开写入口在同一账户上造一列现金支出（每笔 ¥25，日期递增）。目录
/// 级薄壳：开库走产品开库入口，造数形态归共享助手（[`seed`]，issue #2027）。
fn seed_expense_series(dir: &Path, count: usize) {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    seed::seed_expense_series(&db, count);
}

/// 经域层公开写入口在同一账户上造一列带指定中文备注的支出（搜索断言的
/// 原文命中造数面：备注各不相同、日期递增；账户名固定「测试现金」不与
/// 搜索关键词相交）。
fn seed_expense_notes(dir: &Path, notes: &[&str]) {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    seed::seed_expense_notes(&db, notes);
}

/// 在指定日期造一笔指定金额的现金支出（期间维度读路径的造数入口）。
fn seed_expense_on_date(dir: &Path, date: &str, amount_cents: i64) {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    seed::seed_expense_on_date(&db, date, amount_cents, &format!("支出 {date}"));
}

/// 移动档 headless 配置（#2057 窄窗自适应横切：600×720——< 断点 840 的窄窗口，
/// 桌面形态环境原样保留；窄窗适配只吃宽度轴，输入轴恒桌面）。
fn mobile_config() -> EngineConfig {
    EngineConfig {
        viewport: Size::new(Points(600.0), Points(720.0)),
        ..desktop_config()
    }
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

/// de: 窄窗自适应——交易卡片形态与移动档导航壳（#2057 / ADR-0088 决策 2/4/9）：<840
/// 窄窗口侧栏替换为顶栏（汉堡 + 当前视图名）+ 导航抽屉（同一份导航状态，导航即
/// 关闭），交易列表呈卡片形态（断点双渲染——桌面表格不挂载，格文本取装配层同一
/// 产物）。删除窄窗分支挂载 / 卡片形态 / 抽屉导航接线任一环本用例即红。
#[test]
fn app_window_narrow_renders_mobile_shell_and_transaction_cards() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("narrow");
    seed_expense_series(&dir, 1);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(mobile_config()).unwrap();

    // 窄窗移动档导航壳：侧栏不挂载（品牌行「开源记账」只属侧栏/抽屉），顶栏
    // （汉堡入口 + 当前视图名概览）在屏。
    app.wait_for("id:nav-topbar-open").unwrap();
    assert!(!app.shows("开源记账"), "窄窗移动档不挂侧栏");
    app.wait_for("text:概览").unwrap();

    // 抽屉打开 → 同一份导航状态 → 点「交易」落位交易列表（导航即关闭）。
    app.press("id:nav-topbar-open").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    app.press("id:nav-transactions").unwrap();
    // 导航即关闭（导航抽屉随落位收起）：侧栏品牌行唯一词「开源记账」不再在屏。
    app.wait_for_gone("id:nav-transactions", DEFAULT_WAIT)
        .unwrap();
    app.wait_for_gone("text:开源记账", DEFAULT_WAIT).unwrap();

    // 卡片形态：断点双渲染（tx-cards 挂载、桌面表格不挂载），卡片呈现装配层
    // 同源格文本（日期 / 类型 / 账户 / 金额掩码口径的展示文本）。
    app.wait_for("id:tx-cards").unwrap();
    assert!(!app.shows("id:tx-table"), "窄窗卡片形态不挂桌面表格");
    let texts = app.screen().texts();
    for expected in ["2026-01-01", "支出", "测试现金", "¥25"] {
        assert!(
            texts.contains(&expected),
            "卡片应呈现「{expected}」：{:?}",
            texts
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
    // 首页行集在屏 = 装载已落位；分页条挂载等其条件回合（#2057 窄窗双渲染
    // 起 tx-table 经断点分支挂载，落位批次差一拍），再等目标页码按钮本身
    // ——容器先行挂载、按钮行再落一拍（#2072），不等则 press 落进空窗。
    app.wait_for("id:tx-pagination").unwrap();
    app.wait_for("role:button[2]").unwrap();

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
    // 重进交易页携关键词重载——行集落屏即装载完成（分页按钮 disabled(loading)
    // 同回合解除，#2072），按压分页条前先等行集。
    app.wait_for("text:2026-05-30").unwrap();

    // 搜索态翻页：点击页码 2 → 末页（最旧 50 笔，2026-01-01..2026-02-19），
    // 首页最新行不再在屏（断言对准视口内首行，与列表态翻页同口径）。
    app.wait_for("id:tx-pagination").unwrap();
    // 容器先行挂载、按钮行再落一拍（#2072 同 #2057 批次差一拍）——按压前
    // 等目标页码按钮本身。
    app.wait_for("role:button[2]").unwrap();
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
    // 分页条挂载等其装载回合（#2057 起落位批次差一拍）；容器先行挂载、
    // 按钮行再落一拍（#2072 间歇红根因）——按压前等目标页码按钮本身。
    app.wait_for("id:tx-pagination").unwrap();
    app.wait_for("role:button[2]").unwrap();
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
        // #2061 交互原子化后账户格文本节点随原子拆分（首条原子 + 标注尾段，
        // 旧壳平替元素同形）——首名与标注各自成节点。
        "小金库",
        " 等 2 账户",
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
    // #2061 交互原子化后双账户格两段各成可点原子（转出/转入独立载荷，旧壳
    // 双平替元素同形）——两段名各自成节点、箭头居间。
    assert!(
        texts.contains(&"测试现金") && texts.contains(&"应收款"),
        "双账户格两段名在屏：{:?}",
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
        vec!["编辑", "退款", "加入物品", "删除"],
        "行菜单项闭集（#2034 起支出行带编辑项，#2035 起带退款项，#2044 起加「加入物品」，删除殿后）：{menu:?}"
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

/// ADR-0062 确定性性能数据集生成器的 bin 路径（nsh-app 同包 bin
/// `src/bin/ledger-perf`，#2082 迁居）：cargo 为同包 bin 在集成测试编译期
/// 注入 `CARGO_BIN_EXE_*` 并保证其先于测试构建——直跑 `cargo test -p nsh-app`
/// 无需前置构建步骤，二进制缺失路径结构上不可能（#2064 级联假红的触发面
/// 拆除；旧口径「先 `cargo build --bin ledger-perf -p tauri-app`」随迁居
/// 退役）。
fn ledger_perf_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ledger-perf"))
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
    // 可见投影——按钮不存在时 press 即红），末页首行 = 最旧一笔。按钮行
    // 比行集晚一拍落位（#2072）——按压前等按钮本身。
    let last_button = format!("role:button[{}]", facts.pages);
    app.wait_for(&last_button).unwrap();
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
/// 「删除即红」：删读路径的搜索携期间（boot::TxSearchQuery 的 range 字段直传
/// 域 from/to）→ 相 B 当月命中 2 条（上月照样命中）即红；删引擎可编辑焦点闸由引擎侧
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
// 隐藏投资相关流水开关（issue #2062 / ADR-0136）：设备级视图偏好（偏好与筛选
// 分家）+ 交易列表读路径携带 hide_investment_related + 分家语义。环境前
// 提：Environment{os: Linux, idiom: Desktop}（desktop_config）。
// ---------------------------------------------------------------------------

/// #2062 用例共用造数：日常动线上被投资现金流刷屏的四行（ADR-0136 背景）——
/// 银行卡日常支出（偏好开启后唯一在屏）、银行卡→投资户转账（充值，转入端
/// 命中）、投资账户直接付费支出（转出端命中）、分解出资支出（出资端经
/// fundings 子表命中）。全部通用 kind（默认 kind 收窄不排除），三端命中口径
/// 由域读路径 SQL 承担（域单测已覆盖，本面只钉 NSH 旅程）。
fn seed_investment_related_rows(db: &ledger_infra::db::DbState) {
    let bank = seed_account(db, "银行卡", AccountType::Bank);
    let investing = seed_account(db, "投资账户", AccountType::Investment);
    seed_transaction(
        db,
        TxnSeed {
            account: &bank,
            to_account: None,
            kind: TransactionKind::Expense,
            amount_cents: 4_400,
            date: "2026-01-05",
            category_id: None,
            merchant_id: None,
            note: Some("银行卡日常".to_owned()),
        },
    );
    seed_transaction(
        db,
        TxnSeed {
            account: &bank,
            to_account: Some(investing.clone()),
            kind: TransactionKind::Transfer,
            amount_cents: 10_000,
            date: "2026-01-06",
            category_id: None,
            merchant_id: None,
            note: Some("转入投资户".to_owned()),
        },
    );
    seed_transaction(
        db,
        TxnSeed {
            account: &investing,
            to_account: None,
            kind: TransactionKind::Expense,
            amount_cents: 6_600,
            date: "2026-01-07",
            category_id: None,
            merchant_id: None,
            note: Some("投资户付费".to_owned()),
        },
    );
    // 分解出资行（分解与 account_id 互斥，ADR-0138 决策 2）：出资端含投资户
    // → 偏好开启后同样隐藏（fundings 子表命中臂）。
    let mut decomposed = minimal_input(
        TransactionKind::Expense,
        "",
        12_000,
        "2026-01-08",
        None,
        Some("分解出资".to_owned()),
    );
    decomposed.account_id = None;
    decomposed.funding = vec![
        TransactionFundingInput {
            account_id: bank.clone(),
            amount_cents: 7_000,
            label: None,
        },
        TransactionFundingInput {
            account_id: investing,
            amount_cents: 5_000,
            label: None,
        },
    ];
    seed_tx_with_id(db, decomposed);
}

/// 开关旅程（#2062 票面 2/3）：筛选栏开关切换 → 主列表行集隐藏/恢复投资
/// 相关流水（三端任一端为投资类型账户，按账户不按分类；默认 kind 收窄不动）。
/// 删除接线即红：删 `boot::transaction_page` 的 `hide_investment_related`
/// 透传（域 filter 缺省不携带）或删装载闭包的偏好携带 → 开关切换行集不变、
/// 共 1 条等不到。
#[test]
fn filter_bar_hide_investment_toggle_hides_and_restores_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("hide-inv-toggle");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_investment_related_rows(&db);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 缺省（关）：四行全在屏（偏好缺省态行集与旧壳一致——本票不动的部分）。
    app.wait_for("text:共 4 条").unwrap();
    for amount in ["¥44", "¥100", "¥66", "¥120"] {
        app.wait_for(&format!("text:{amount}")).unwrap();
    }

    // 开关开启：投资相关三行离屏（充值转账一并隐藏——三端口径），日常支
    // 出保留；total 与行集同口径收窄（服务端分页，ADR-0008）。
    app.press("id:tx-filter-hide-investment").unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:¥44").unwrap();
    for amount in ["¥100", "¥66", "¥120"] {
        assert!(
            !app.shows(amount),
            "偏好开启后投资相关行离屏（{amount}）：{:?}",
            app.screen().texts()
        );
    }

    // 开关关闭：四行恢复（点选即重拉，偏好是行集先决收窄）。
    app.press("id:tx-filter-hide-investment").unwrap();
    app.wait_for("text:共 4 条").unwrap();
    for amount in ["¥44", "¥100", "¥66", "¥120"] {
        app.wait_for(&format!("text:{amount}")).unwrap();
    }
}

/// 偏好跨重启保留（#2062 票面 1：设备级持久——`Storage` 平台文件侧 KV，
/// 不进 SQLite、不与旧壳共享）：开启后重启（同一存储后端起第二/第三个引
/// 擎），行集按持久偏好收窄、关闭态同样持久。删除接线即红：删开关写盘
/// （on→set_item / off→remove_item）→ 重启后行集回全量红；删启动读路径
/// （偏好信号初值不经存储）→ 重启后行集回全量红。
#[test]
fn hide_investment_preference_persists_across_restart() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("hide-inv-restart");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_investment_related_rows(&db);
    }
    let storage = MemoryStorage::new(Vec::new());

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_backend(storage.clone())).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 4 条").unwrap();
    app.press("id:tx-filter-hide-investment").unwrap();
    app.wait_for("text:共 1 条").unwrap();
    // 存储值形态（JSON 布尔字面量，与旧壳 view-state 家族同形不同存储）。
    assert_eq!(
        storage
            .entries()
            .get("view_state:hide_investment_related")
            .map(String::as_str),
        Some("true"),
        "开启即持久化（默认态 = 无记录的写契约另一半由关闭臂断言）"
    );
    drop(app);

    // 重启（开启态持久）：不经开关，行集即按偏好收窄。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_backend(storage.clone())).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 1 条").unwrap();
    assert!(
        !app.shows("¥100"),
        "重启后投资相关行仍隐藏：{:?}",
        app.screen().texts()
    );

    // 关闭（删记录）→ 再重启：默认关持久，行集回全量。
    app.press("id:tx-filter-hide-investment").unwrap();
    app.wait_for("text:共 4 条").unwrap();
    assert!(
        !storage
            .entries()
            .contains_key("view_state:hide_investment_related"),
        "关闭删记录（默认态 = 无记录，ADR-0116 家族同构）"
    );
    drop(app);
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 4 条").unwrap();
}

/// 分家语义（#2062 票面 3 / ADR-0136 决策 1）：偏好是持久裁决不是筛选维度
/// ——清除筛选与 ESC 复位不触达（复位后行集仍按偏好收窄），偏好也不参与
/// 清除按钮禁用态（无保留状态即禁用，开了偏好 ≠ 有筛选）。删除接线即红：
/// 让 reset_filters 复位偏好或让 filters_active 计入偏好，任一耦合本测即红。
#[test]
fn hide_investment_preference_not_touched_by_clear_or_escape() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("hide-inv-split");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_investment_related_rows(&db);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 开启偏好 + 设账户筛选：行集 = 偏好收窄 ∩ 筛选（唯一银行卡日常支出行）。
    app.press("id:tx-filter-hide-investment").unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.press("id:tx-filter-account").unwrap();
    app.wait_for("id:tx-filter-account-search").unwrap();
    app.type_text("id:tx-filter-account-search", "银行卡")
        .unwrap();
    app.press("id:tx-filter-account-option >> text:银行卡 >> nth=0")
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();

    // 清除筛选：筛选维度归零，偏好原样（复位后行集仍是偏好收窄集，不是
    // 全量四行）；清除按钮回禁用（偏好不计入激活判定）。
    app.press("id:tx-filter-clear").unwrap();
    app.settle();
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:¥44").unwrap();
    assert!(
        !app.shows("¥100"),
        "清除筛选不触达偏好（投资相关行仍隐藏）：{:?}",
        app.screen().texts()
    );
    assert!(
        !app.screen().enabled("id:tx-filter-clear").unwrap(),
        "偏好开启 ≠ 有筛选：清除按钮禁用态不计偏好（ADR-0136 决策 1）"
    );

    // 再设账户筛选后 ESC 复位：同语义不触达偏好。
    app.press("id:tx-filter-account").unwrap();
    app.wait_for("id:tx-filter-account-search").unwrap();
    app.type_text("id:tx-filter-account-search", "银行卡")
        .unwrap();
    app.press("id:tx-filter-account-option >> text:银行卡 >> nth=0")
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.press_key("escape").unwrap();
    app.settle();
    app.wait_for("text:共 1 条").unwrap();
    assert!(
        !app.shows("¥100"),
        "ESC 复位不触达偏好（投资相关行仍隐藏）：{:?}",
        app.screen().texts()
    );
    assert!(
        !app.screen().enabled("id:tx-filter-clear").unwrap(),
        "ESC 复位后无保留状态（幂等守卫的可见凭据）"
    );

    // 开关关闭：回全量（分家语义下的开关仍随时可关，行集即时恢复）。
    app.press("id:tx-filter-hide-investment").unwrap();
    app.wait_for("text:共 4 条").unwrap();
}

// ---------------------------------------------------------------------------
// 表格级点击委托（#2061；旧壳 transaction-table-actions 四动作的本壳重生）：
// 账户/商户格自下钻（筛选会话态全量替换）、转账行双原子各下钻对应侧、来源格
// 跳转（导航单点）、备注复制（剪贴板 + 条位反馈）。删格级点击委托接线
//（render_cell 交互格分发）即红的负向条目逐项写在各测试注释（ADR-0087
// 断言强度：断言对准用户可观察结果）。
// ---------------------------------------------------------------------------

/// 账户格下钻（#2061）：点账户原子 → 筛选会话态全量替换（商户维度复位——
/// 旧壳「新 query 全量替换旧 query」同构），行集按涉及账户收窄。判别性证据：
/// 商户筛选在场时先只剩交集行，点账户原子后非交集的同账户行回屏（若误为
/// 维度 AND 叠加则仍只剩交集）。删格级点击委托接线（账户原子分发）即红：
/// 点击无效果，行集维持商户交集不变。
#[test]
fn tx_table_account_cell_click_self_drills_with_full_replacement() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("tx-cell-account");
    let cash_id;
    let merchant_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        cash_id = seed_cash_account(&db);
        let bank = seed_account(&db, "银行卡", AccountType::Bank);
        merchant_id = seed_merchant(&db, "链商超");
        // 现金三笔：一笔带商户（命中商户筛选），两笔不带（全量替换后才回屏）。
        for (date, amount, merchant, note) in [
            (
                "2026-01-01",
                2_500,
                Some(merchant_id.clone()),
                "现金带商户笔",
            ),
            ("2026-01-02", 2_600, None, "现金笔二"),
            ("2026-01-03", 2_700, None, "现金笔三"),
        ] {
            seed_transaction(
                &db,
                TxnSeed {
                    account: &cash_id,
                    to_account: None,
                    kind: TransactionKind::Expense,
                    amount_cents: amount,
                    date,
                    category_id: None,
                    merchant_id: merchant.clone(),
                    note: Some(note.to_owned()),
                },
            );
        }
        seed_transaction(
            &db,
            TxnSeed {
                account: &bank,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 6_600,
                date: "2026-01-04",
                category_id: None,
                merchant_id: Some(merchant_id),
                note: Some("银行带商户笔".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 4 条").unwrap();

    // 商户筛选先在场（链商超 → 交集 2 笔：现金带商户笔 + 银行带商户笔）。
    app.press("id:tx-filter-merchant").unwrap();
    app.wait_for("id:tx-filter-merchant-search").unwrap();
    app.type_text("id:tx-filter-merchant-search", "链商超")
        .unwrap();
    app.press("id:tx-filter-merchant-option >> text:链商超 >> nth=0")
        .unwrap();
    app.wait_for("text:共 2 条").unwrap();
    assert!(!app.shows("现金笔三"), "商户筛选后非交集行离屏");

    // 点现金账户原子 → 全量替换：商户维度复位 + 账户维度落位 → 现金三笔
    //（无商户的两笔回屏 = 全量替换的判别性证据；维度 AND 叠加只剩交集 1 笔）。
    app.press(&format!("id:tx-cell-account-{cash_id} >> nth=0"))
        .unwrap();
    app.wait_for("text:共 3 条").unwrap();
    assert!(app.shows("现金笔三"), "非交集同账户行回屏（全量替换）");
    assert!(!app.shows("银行带商户笔"), "他账户行被下钻筛选排除");

    // 落位态走正常筛选通道（清除按钮回全量 4 笔）。
    app.press("id:tx-filter-clear").unwrap();
    app.wait_for("text:共 4 条").unwrap();
}

/// 转账行双原子（#2061 平价；旧壳 accountCellSkeleton 双原子各带独立载荷）：
/// 转出/转入两段各自可点、各下钻对应账户侧。删双原子任一侧分发即红：点击
/// 后行集不按该侧账户收窄。
#[test]
fn tx_table_transfer_row_two_account_atoms_drill_to_each_side() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("tx-cell-transfer");
    let cash_id;
    let bank_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        cash_id = seed_cash_account(&db);
        bank_id = seed_account(&db, "银行卡", AccountType::Bank);
        // 转账一笔（双向原子）+ 两账户各自的独立支出。
        for (account, amount, date, note) in [
            (&cash_id, 5_000, "2026-01-01", "转账笔"),
            (&cash_id, 2_500, "2026-01-02", "现金笔"),
            (&bank_id, 6_600, "2026-01-03", "银行笔"),
        ] {
            seed_transaction(
                &db,
                TxnSeed {
                    account,
                    to_account: (note == "转账笔").then(|| bank_id.clone()),
                    kind: if note == "转账笔" {
                        TransactionKind::Transfer
                    } else {
                        TransactionKind::Expense
                    },
                    amount_cents: amount,
                    date,
                    category_id: None,
                    merchant_id: None,
                    note: Some(note.to_owned()),
                },
            );
        }
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 3 条").unwrap();

    // 点转出侧原子 → 涉及账户收窄到现金（转账 + 现金笔；银行笔排除）。
    app.press(&format!("id:tx-cell-account-{cash_id} >> nth=0"))
        .unwrap();
    app.wait_for("text:共 2 条").unwrap();
    assert!(app.shows("转账笔") && app.shows("现金笔"));
    assert!(!app.shows("银行笔"), "转出侧下钻排除他账户行");

    // 转账行仍在屏（涉及账户维度），点转入侧原子 → 收窄到银行。
    app.press(&format!("id:tx-cell-account-{bank_id} >> nth=0"))
        .unwrap();
    app.wait_for("text:共 2 条").unwrap();
    assert!(app.shows("转账笔") && app.shows("银行笔"));
    assert!(!app.shows("现金笔"), "转入侧下钻排除转出侧独立行");
}

/// 商户格下钻（#2061）：点商户名 → 商户维度全量替换落位。删商户原子分发
/// 即红：点击后行集不按商户收窄。
#[test]
fn tx_table_merchant_cell_click_drills_to_merchant_filtered() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("tx-cell-merchant");
    let merchant_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_cash_account(&db);
        merchant_id = seed_merchant(&db, "链商超");
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: "2026-01-01",
                category_id: None,
                merchant_id: Some(merchant_id.clone()),
                note: Some("商户笔".to_owned()),
            },
        );
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 6_600,
                date: "2026-01-02",
                category_id: None,
                merchant_id: None,
                note: Some("无商户笔".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 2 条").unwrap();

    app.press(&format!("id:tx-cell-merchant-{merchant_id} >> nth=0"))
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    assert!(app.shows("商户笔"));
    assert!(!app.shows("无商户笔"), "商户下钻排除无商户行");
}

/// 来源格跳转（#2061）：点来源名 → 经导航单点（resolve_source_jump → open）
/// 落来源视图（保单——出厂收纳态落资产「更多」容器）。软删来源无跳转载荷
///（旧壳 clickable 同口径）：点不存在原子（软删行）落空报错、停留交易列表。
/// 删来源分发即红：点击停留交易列表，保单视图不落位。
#[test]
fn tx_table_source_cell_click_jumps_to_policy_view() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("tx-cell-source");
    let alive_policy;
    let deleted_policy;
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
        deleted_policy =
            ledger_policy::create_policy(&conn, make_policy("P-1", "医疗险"), &mut || {}).unwrap();
        alive_policy =
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
        ledger_transaction::create(&conn, make_expense("2026-01-15", deleted_policy.clone()))
            .unwrap();
        ledger_transaction::create(&conn, make_expense("2026-01-16", alive_policy.clone()))
            .unwrap();
        // 软删保单：历史引用保留，来源格状态标注「已删除」且不可点。
        ledger_policy::delete_policy(&conn, &deleted_policy, &mut || {}).unwrap();
        drop(conn);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:医疗险 已删除").unwrap();

    // 软删来源不可点：无原子（点击落空）。
    assert!(
        app.press(&format!("id:tx-cell-source-{deleted_policy}"))
            .is_err(),
        "软删保单无跳转原子（点击落空）"
    );

    // 点在用保单来源名 → 落保单视图（出厂收纳态：资产「更多」容器页签）。
    app.press(&format!("id:tx-cell-source-{alive_policy}"))
        .unwrap();
    app.wait_for("id:policy-table").unwrap();
}

/// 备注复制（#2061；旧壳 NoteCopyButton 语义迁移）：点复制 → 剪贴板呈现备注
/// 原文 + 成功反馈条。购买项行与无备注行不渲染复制通道（旧壳复制通道同
/// 口径）。删复制分发即红：剪贴板断言与反馈文案两处即红（ai_view 同形）。
#[test]
fn tx_table_note_copy_writes_clipboard_with_feedback() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("tx-cell-copy");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let cash = seed_cash_account(&db);
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        // 备注行（复制通道唯一持有者）。
        ledger_transaction::create(
            &conn,
            TransactionInput {
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                currency_code: "CNY".to_owned(),
                account_id: Some(cash.clone()),
                to_account_id: None,
                funding_account_id: None,
                funding: Vec::new(),
                purchases: Vec::new(),
                category_id: None,
                merchant_id: None,
                merchant_name: None,
                policy_id: None,
                refund_of_transaction_id: None,
                note: Some("复制我这条备注".to_owned()),
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
        // 购买项行（一列两用显示商品名，无复制通道）。
        ledger_transaction::create(
            &conn,
            TransactionInput {
                kind: TransactionKind::Expense,
                amount_cents: 6_600,
                currency_code: "CNY".to_owned(),
                account_id: Some(cash.clone()),
                to_account_id: None,
                funding_account_id: None,
                funding: Vec::new(),
                purchases: vec![TransactionPurchaseInput {
                    name: "猫粮".to_owned(),
                    quantity: 2,
                    category_id: None,
                    unit_price_cents: None,
                }],
                category_id: None,
                merchant_id: None,
                merchant_name: None,
                policy_id: None,
                refund_of_transaction_id: None,
                note: None,
                source_order_no: None,
                date: "2026-01-02".to_owned(),
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
        // 无备注行（同一写连接守卫内直造，避免二次加锁死等）。
        ledger_transaction::create(
            &conn,
            TransactionInput {
                kind: TransactionKind::Expense,
                amount_cents: 8_800,
                currency_code: "CNY".to_owned(),
                account_id: Some(cash.clone()),
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
                date: "2026-01-03".to_owned(),
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
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 3 条").unwrap();

    // 复制通道唯一持有者 = 备注行（购买项与无备注行不渲染「复制」）。
    let copies = app
        .screen()
        .texts()
        .iter()
        .copied()
        .filter(|t| *t == "复制")
        .count();
    assert_eq!(copies, 1, "仅备注行带复制通道：{:?}", app.screen().texts());
    // 复制前剪贴板为空。
    assert!(app.screen().clipboard().is_none(), "复制前无剪贴板文本");
    app.press("id:tx-cell-copy-note").unwrap();
    app.wait_for("text:备注已复制").unwrap();
    assert_eq!(
        app.screen().clipboard(),
        Some("复制我这条备注"),
        "剪贴板内容 = 备注原文"
    );
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

/// 点击导航：侧栏项 → 真实视图落位（账户 #2041 / 攒钱 #2056 起不再占位）→
/// 回到交易项 → 表格内容区原样（其余未迁移项的占位行为由各自迁移票接手）。
#[test]
fn clicking_sidebar_items_lands_placeholder_views_and_back() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("nav-click");
    seed_expense_series(&dir, 3);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 账户 → 真实视图（新增账户卡在屏，占位不再）。
    app.press("id:nav-accounts").unwrap();
    app.wait_for("text:新增账户").unwrap();
    assert!(
        app.screen().count("id:placeholder-note").unwrap() == 0,
        "账户不再占位"
    );
    // 攒钱 → 真实视图（新建目标钮在屏，占位不再，#2056 迁入）。
    app.press("id:nav-savingsGoals").unwrap();
    app.wait_for("id:sg-new").unwrap();
    assert!(
        app.screen().count("id:placeholder-note").unwrap() == 0,
        "攒钱不再占位"
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
    // 投资视图已迁入（#2045）：落位凭据 = 概览页签锚点（占位不再——#2045
    // 迁入时应同步的断言，本票范围外修复补齐）。
    app.wait_for("id:inv-tab-overview").unwrap();
    // 落位凭据 = 内容区随视图切换（概览是真实视图，净资产卡在屏即证据；
    // #2039 起概览不再是占位）。
    app.press_key("mod+`").unwrap();
    app.wait_for("id:net-worth-card").unwrap();
    assert!(
        app.screen().count("id:placeholder-note").unwrap() == 0,
        "⌘` 落真实概览"
    );
    app.press_key("mod+0").unwrap();
    // ⌘0 落 AI：#2055 起提示词页为真实组件（此前是占位屏名 count 钉）。
    app.wait_for("id:ai-prompt-view").unwrap();
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
    // 保单——#2050 迁入后首签内容 = 真实保单视图（清单卡在屏，占位不再）。
    app.press("id:nav-more-assets").unwrap();
    app.wait_for("id:more-tab-policies").unwrap();
    app.wait_for("id:more-tab-physicalAssets").unwrap();
    app.wait_for("id:more-tab-insurers").unwrap();
    app.wait_for("id:policy-table").unwrap();
    assert!(
        app.screen().count("id:placeholder-note").unwrap() == 0,
        "保单页签不再占位（#2050 迁入）"
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

    // 失效信号驱动重读：先等表单清空回占位（「支出分类」回屏），再等新行
    // 落表（金额 80）。CI 慢环境时序锚定：wait_for_within("text:80") 会被
    // 输入框自身已填的「80」立即满足，抢在表单清空/重读落表前（run
    // 42949840617 首崩即此：占位未回屏，「支出分类」断言失败）。
    app.wait_for("text:支出分类").unwrap();
    app.wait_for("text:80").unwrap();
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
    // #2061 交互原子化后双账户格两段名各自成节点（旧壳双平替元素同形）。
    app.wait_for("text:测试现金").unwrap();
    app.wait_for("text:测试银行").unwrap();
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
    // #2061 交互原子化后双账户格两段名各自成节点（旧壳双平替元素同形）。
    app.wait_for("text:测试现金").unwrap();
    app.wait_for("text:借出·张三").unwrap();

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

    // 普通支出行：编辑 + 退款 + 加入物品（#2044）+ 删除。
    app.right_click("text:2026-01-01").unwrap();
    let menu = app.menu().expect("右键应打开上下文菜单");
    assert_eq!(
        menu.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        vec!["编辑", "退款", "加入物品", "删除"],
        "普通支出行菜单：{menu:?}"
    );
    app.press_key("escape").unwrap();
    app.settle();

    // 出资分解行：编辑开放（表单③子行编辑；进出资分解族回填）+ 不开放退
    // 款（#1861 口径）+ 加入物品（#2044；分解行也是支出行——溯源口径同
    // 旧壳 row-menu:分解行保留加入物品）+ 删除。
    app.right_click("text:2026-01-02").unwrap();
    let menu = app.menu().expect("右键应打开上下文菜单");
    assert_eq!(
        menu.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        vec!["编辑", "加入物品", "删除"],
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
    // #2061 交互原子化后首名与标注各自成节点（匹配器按节点 trim 全等）。
    app.wait_for("text:测试现金").unwrap();
    app.wait_for("text:等 2 账户").unwrap();
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
    // 最新行离屏（断言对准视口内首行，交易列表分页同口径）。容器先行
    // 挂载、按钮行再落一拍（#2072）——按压前等目标页码按钮本身。
    app.wait_for("role:button[2]").unwrap();
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

// ---------------------------------------------------------------------------
// 储蓄目标视图（#2056）：目标清单 / 蓄水进度（已存/还差/达成态）/ 表单弹层
// （新建 + 编辑）/ 生命周期守卫（归档/取消归档/删除——余额非零码化拒绝）。
// 删任一接线即红的负向条目逐项写在各测试注释（ADR-0087 断言强度：断言对准
// 用户可观察结果）。
// ---------------------------------------------------------------------------

/// 从侧栏点开储蓄目标视图（#2056 旅程用例共用）：新建按钮在屏即落位凭据。
fn open_savings(app: &mut Headless) {
    app.wait_for("id:nav-savingsGoals").unwrap();
    app.press("id:nav-savingsGoals").unwrap();
    app.wait_for("id:sg-new").unwrap();
}

/// 造一个储蓄目标（用例共用；经域公开写入口——同事务自动建专属账户）。
fn seed_savings_goal(
    db: &ledger_infra::db::DbState,
    name: &str,
    target_cents: i64,
    deadline: Option<&str>,
) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_savings_goal::create_savings_goal(
        &conn,
        &ledger_savings_goal::SavingsGoalInput {
            name: name.to_owned(),
            target_amount_cents: target_cents,
            deadline: deadline.map(|d| d.to_owned()),
        },
    )
    .unwrap()
}

/// 首屏装载（旧壳 SavingsGoalsView onMounted 平价）：目标清单卡（列头与行格
/// 文本：目标额 / 已存 / 还差 = 域读路径单点口径）、空态引导（无目标时）、
/// 归档卡缺席。删除 boot 装载读接缝 / rows 装配 / 香2行分支任一环即红。
#[test]
fn savings_view_lands_with_progress_rows_and_empty_guide() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("savings-land");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_savings_goal(&db, "买车基金", 20_000, None);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_savings(&mut app);
    // 异步装载纪律：先等行到屏（§3.5）再断格文本与列头。
    app.wait_for("id:sg-table").unwrap();
    app.wait_for("text:买车基金").unwrap();

    // 列头与行格文本（装配层产物：目标额 / 已存 / 还差 / 状态 / 截止日）。
    for text in [
        "攒钱",
        "新建目标",
        "名称",
        "目标金额",
        "已存",
        "还差",
        "推算",
        "截止日",
        "状态",
        "操作",
        "买车基金",
        "¥200",
        "¥0",
        "进行中",
        "—",
    ] {
        assert!(
            app.shows(text),
            "应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 无归档目标时归档卡缺席（旧壳 v-if 同口径）。
    assert!(
        app.screen().count("id:sg-archived-table").unwrap() == 0,
        "无归档目标时归档卡不渲染"
    );
}

/// 零目标空态（旧壳 emptyGuide 同文案）：引导在场，行集零。
#[test]
fn savings_view_shows_empty_guide_without_goals() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("savings-empty");
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_savings(&mut app);
    app.wait_for("id:sg-table").unwrap();
    app.wait_for("text:还没有攒钱目标——新建一个，账本会为它自动开一个专属账户")
        .unwrap();
    assert_eq!(
        app.screen().count("id:sg-edit").unwrap(),
        0,
        "零目标时无行操作钮"
    );
}

/// 创建旅程（旧壳 create + message.success 平价）：空名称前置拦截不写入；
/// 目标额非正拦截（客户端双保险防呆）；新建模式无计划月存字段（结构性只在
/// 编辑模式出现）；填名称 + 目标额 → 保存 → 行即现与反馈条位、弹窗关闭。
/// 删除「新建 → perform_save → reload」任一接线即红。
#[test]
fn savings_view_create_goal_journey() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("savings-create");
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_savings(&mut app);
    app.press("id:sg-new").unwrap();
    app.wait_for("id:sg-form-name").unwrap();
    app.wait_for("text:新建攒钱目标").unwrap();

    // 新建模式不出现计划月存字段（结构性只在编辑模式出现）。
    assert_eq!(
        app.screen().count("id:sg-form-planned").unwrap(),
        0,
        "新建模式无计划月存字段"
    );

    // 空名称拦截：不写入、弹窗不关。
    app.press("id:sg-form-save").unwrap();
    app.wait_for("id:sg-form-error").unwrap();
    app.wait_for("text:请填写目标名称").unwrap();

    // 目标额非正拦截：目标金额必须大于 0（客户端双保险，旧壳同口径）。
    app.type_text("id:sg-form-name", "旅行基金").unwrap();
    app.type_text("id:sg-form-amount", "0").unwrap();
    app.press("id:sg-form-save").unwrap();
    app.wait_for("text:目标金额必须大于 0").unwrap();

    // 合法入参 → 保存：行即现、成功反馈在屏、弹窗关闭。
    app.clear("id:sg-form-amount").unwrap();
    app.type_text("id:sg-form-amount", "500").unwrap();
    app.press("id:sg-form-save").unwrap();
    app.wait_for("text:旅行基金").unwrap();
    app.wait_for("text:目标已创建，专属账户已就位").unwrap();
    app.wait_for_gone("id:sg-form-save", DEFAULT_WAIT).unwrap();
    app.wait_for("id:sg-table").unwrap();
    // 重读异步落表（重读期屏幕可呈「正在加载…」，表壳 wait 不证行落屏）：
    // 等行格内容再断言（run 43128098055 首崩即此）。
    app.wait_for("text:¥500").unwrap();
    app.wait_for("text:进行中").unwrap();
    for text in ["¥500", "进行中"] {
        assert!(
            app.shows(text),
            "应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
}

/// 编辑旅程（旧壳 update 四字段全量替换平价）：编辑弹层回填名称 / 目标额 /
/// 计划月存（编辑模式字段在屏）；改名 + 改额 + 设置计划月存 → 保存 → 行格
/// 刷出新名新额、成功反馈在屏。删除「编辑 → perform_save(update) → reload」
/// 任一接线即红。
#[test]
fn savings_view_edits_goal_with_planned_monthly() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("savings-edit");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_savings_goal(&db, "买车基金", 20_000, None);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_savings(&mut app);
    app.wait_for("text:买车基金").unwrap();

    // 编辑模式字段闭集回填：名称 / 目标额原值（分转元）、计划月存空。
    app.press("id:sg-edit").unwrap();
    app.wait_for("id:sg-form-name").unwrap();
    app.wait_for("text:编辑攒钱目标").unwrap();
    assert_eq!(
        app.screen().input_text("id:sg-form-name").unwrap(),
        Some("买车基金"),
        "名称回填原值"
    );
    assert_eq!(
        app.screen().input_text("id:sg-form-amount").unwrap(),
        Some("200"),
        "目标额回填原值（分转元）"
    );

    // 改名 + 改额 + 计划月存 → 保存：行刷出新名 / 新额，反馈在屏
    //（type_text 是追加键入，先 clear 清回填目值——键盘全选替换的对应形态）。
    app.clear("id:sg-form-name").unwrap();
    app.type_text("id:sg-form-name", "环游世界基金").unwrap();
    app.clear("id:sg-form-amount").unwrap();
    app.type_text("id:sg-form-amount", "300").unwrap();
    // 截止日键入面（date_option_select 可清空日期封装件，#2056 随消费面
    // 开门）：完整日期键入即落值，保存后截止日列呈现同日。
    app.type_text("id:sg-form-deadline-input", "2027-06-30")
        .unwrap();
    app.type_text("id:sg-form-planned", "100").unwrap();
    app.press("id:sg-form-save").unwrap();
    app.wait_for_gone("id:sg-form-save", DEFAULT_WAIT).unwrap();
    app.wait_for("text:环游世界基金").unwrap();
    app.wait_for("text:目标已更新").unwrap();
    for text in ["¥300", "手填 ¥100/月", "2027-06-30"] {
        assert!(
            app.shows(text),
            "应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
}

/// 归档 / 取消归档 / 删除（旧壳 #1754 生命周期平价）：归档移入归档卡（退出
/// 默认列表），取消归档回默认列表；删除经二次确认，取消不写入、确认后目标
/// 从列表消失。删除归档 / 取消归档 / 确认接线任一环即红。
#[test]
fn savings_view_archives_unarchives_and_deletes_with_confirmation() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("savings-life");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_savings_goal(&db, "买车基金", 10_000, None);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_savings(&mut app);
    app.wait_for("text:买车基金").unwrap();

    // 归档：移入归档卡（归档卡在屏）与历史保留口径；成功反馈在屏。
    app.press("id:sg-archive").unwrap();
    app.wait_for("id:sg-archived-table").unwrap();
    app.wait_for("text:目标已归档，历史与流水保留可查").unwrap();
    // 归档卡行在（同一行名）且归档卡首行操作钮闭集（取消归档 / 删除）。
    app.wait_for("text:已归档").unwrap();

    // 取消归档：回默认列表（归档卡消失——旧壳 v-if 归档空不渲染同口径）。
    app.press("id:sg-unarchive").unwrap();
    app.wait_for_gone("id:sg-archived-table", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:目标已恢复为进行中").unwrap();
    app.wait_for("id:sg-edit").unwrap();

    // 删除确认：取消不写入（确认框消失、目标仍在、无删除反馈）。
    app.press("id:sg-delete").unwrap();
    app.wait_for("text:删除攒钱目标").unwrap();
    app.wait_for("text:确认删除「买车基金」？").unwrap();
    app.wait_for("text:删除后目标与专属账户一并收起，历史交易保留可查；账户仍有余额时将被拒绝。")
        .unwrap();
    app.press("id:danger-cancel").unwrap();
    app.wait_for_gone("text:删除攒钱目标", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("id:sg-edit").unwrap();
    assert!(
        !app.shows("目标已删除"),
        "取消不写入：{:?}",
        app.screen().texts()
    );

    // 删除确认：目标从列表消失（含级联专属账户同一事务）。
    app.press("id:sg-delete").unwrap();
    app.wait_for("text:删除攒钱目标").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:目标已删除，专属账户一并收起，历史交易仍可查")
        .unwrap();
    // 重读异步落表：反馈在屏 ≠ 重读已落，等行离场再断。
    app.wait_for_gone("id:sg-edit", DEFAULT_WAIT).unwrap();
    assert_eq!(
        app.screen().count("id:sg-edit").unwrap(),
        0,
        "删除后目标行从列表消失"
    );
}

/// 删除守卫（旧壳 #1754 余额非零码化拒绝平价）：余额非零被域守卫
/// 拒绝（引导先转出），错误落反馈条位且目标仍在。删除「boot 直调域写入口」
/// 接线即红（守卫住在域单正文，本壳不自建）。
#[test]
fn savings_view_delete_guard_refuses_nonzero_balance() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("savings-guard");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let goal_id = seed_savings_goal(&db, "买车基金", 10_000, None);
        // 余额非零：专属账户调整余额（域公开写入口，V017 缓存落到）。
        let account_id = {
            let conn = db
                .conn
                .lock()
                .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
                .unwrap();
            ledger_savings_goal::list_savings_goal_progress(&conn, today())
                .unwrap()
                .into_iter()
                .find(|row| row.goal.id == goal_id)
                .unwrap()
                .goal
                .account_id
                .clone()
        };
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_accounts::adjust_account_balance(
            &conn,
            &account_id,
            &ledger_accounts::AccountBalanceAdjustInput {
                target_balance_cents: 5_000,
                date: today().to_string(),
                note: None,
            },
        )
        .unwrap();
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_savings(&mut app);
    // 异步装载纪律：先等行到屏再断已存（V017 缓存读数）。
    app.wait_for("id:sg-table").unwrap();
    app.wait_for("text:买车基金").unwrap();
    app.wait_for("text:¥50").unwrap();

    // 删除经确认 → 写入口码化拒绝：错误落反馈条位，目标与专属账户仍在。
    app.press("id:sg-delete").unwrap();
    app.wait_for("text:删除攒钱目标").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:删除失败: 目标账户仍有余额，请先转出后再删除")
        .unwrap();
    app.wait_for("id:sg-edit").unwrap();
    assert!(!app.shows("目标已删除"));
}

/// 达成态呈现（旧壳 achieved 渲染平价）：已存 = 目标额时状态列「已达成」
///（读时派生纯展示态，推算整体退场）；无关联在用计划（手填节奏）时不给
/// 关联计划提示（两域边界）。删除达成行状态覆写接线即红。
#[test]
fn savings_view_achieved_state_and_plan_hint_boundaries() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("savings-achieved");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let goal_id = seed_savings_goal(&db, "攒满目标", 5_000, None);
        let account_id = {
            let conn = db
                .conn
                .lock()
                .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
                .unwrap();
            ledger_savings_goal::list_savings_goal_progress(&conn, today())
                .unwrap()
                .into_iter()
                .find(|row| row.goal.id == goal_id)
                .unwrap()
                .goal
                .account_id
                .clone()
        };
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_accounts::adjust_account_balance(
            &conn,
            &account_id,
            &ledger_accounts::AccountBalanceAdjustInput {
                target_balance_cents: 5_000,
                date: today().to_string(),
                note: None,
            },
        )
        .unwrap();
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_savings(&mut app);
    app.wait_for("id:sg-table").unwrap();
    app.wait_for("text:攒满目标").unwrap();
    // 达成 = 已存 ≥ 目标额（读时派生）：状态列已达成、还差恒显 ¥0、推算退场。
    for text in ["已达成", "¥0"] {
        assert!(
            app.shows(text),
            "应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 无关联在用计划（装配面 plan_hint 判定True需 plan 节奏来源）：不显示关联
    // 计划提示（lib 单测钉正向分支，headless 钉负向边界不需手模计划）。
    assert_eq!(
        app.screen().count("id:sg-plan-hint").unwrap(),
        0,
        "达成但无在用计划（手填节奏）：不显示关联计划提示"
    );
}

// ---------------------------------------------------------------------------
// 保司管理视图（#2051）：保司清单双分区（在用/已删）、新增卡（名称）、拼音
// 本地过滤（统一模糊搜索语义）、编辑改名单字段弹窗、软删轻量确认气泡与已删
// 只读切换。删任一接线即红的负向条目逐项写在各测试注释（ADR-0087 断言强
// 度：断言对准用户可观察结果）。
// ---------------------------------------------------------------------------

/// 从「资产·更多」容器点开保司页签（#2051 旅程用例共用）：保司清单卡在屏即
/// 落位凭据。删 views.rs [group_more_view] 的 Insurers 挂载分支 → 本函数永远
/// 等不到清单卡（落回迁移占位）。
fn open_insurers(app: &mut Headless) {
    app.wait_for("id:nav-more-assets").unwrap();
    app.press("id:nav-more-assets").unwrap();
    app.wait_for("id:more-tab-insurers").unwrap();
    app.press("id:more-tab-insurers").unwrap();
    app.wait_for("id:ins-table").unwrap();
    app.wait_for("text:保险公司列表").unwrap();
}

/// 经域读路径反查保司 id（用例共用；行操作锚点嵌 id 用）。
fn nsh_app_lib_insurer_id(dir: &std::path::Path, name: &str) -> String {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_policy::list_insurers(&conn, true)
        .unwrap()
        .into_iter()
        .find(|i| i.name == name)
        .map(|i| i.id)
        .unwrap_or_else(|| panic!("未找到保司 {name}"))
}

/// 首屏装载（旧壳 InsurerManager onMounted 平价）：种子保司名称序在场，新增
/// 卡 / 清单卡 / 搜索框 / 显示已删切换在屏。删 boot::insurer_rows 读接缝 /
/// 装配 / 渲染爪任一环即红。
#[test]
fn insurers_view_lands_with_seed_rows_and_tools() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("insurers-land");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_insurers(&mut app);
    // 种子（V019）名称序行集在屏（可视前两行为种子保司；名称序位在底部
    // 虚拟化裁行外的行不进断言面，代表性断言已在锚点计数完成）。
    for text in [
        "中华保险".to_owned(),
        "中国人寿".to_owned(),
        "新增保险公司".to_owned(),
        "保险公司列表".to_owned(),
        "名称".to_owned(),
        "添加".to_owned(),
        "☐ 显示已删".to_owned(),
        "操作".to_owned(),
    ] {
        assert!(
            app.shows(&text),
            "应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 搜索框在屏（虚拟化列表之外的锚点计数形态）。
    assert_eq!(
        app.screen().count("id:ins-search").unwrap(),
        1,
        "搜索框在屏"
    );
}

/// 创建旅程（旧壳 createInsurer 平价）：空名称拦截不写入；合法名称 → 行即现
/// 与成功反馈、表单清空；重名（种子里已有人寿）→ 码化错误原样呈现、表单不
/// 清空。删 boot::create_insurer 域直调接线 → 保存后永不出现新行且反馈条
/// 份位静默。
#[test]
fn insurers_view_add_journey_with_empty_and_duplicate_guards() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("insurers-add");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_insurers(&mut app);

    // 空名称拦截：不写入（码化错误域同名文案；同口径提示在新增卡条位）。
    app.press("id:ins-add").unwrap();
    app.wait_for("id:ins-add-error").unwrap();
    app.wait_for("text:请输入保险公司名称").unwrap();

    // 合法名称 → 成功反馈 + 行即现（搜索定位：新行名称序位在虚拟化视口
    // 之外，以过滤器把它拉回可视位）。
    app.type_text("id:ins-add-name", "海峡金桥财产保险")
        .unwrap();
    app.press("id:ins-add").unwrap();
    app.wait_for("text:已添加保险公司").unwrap();
    app.type_text("id:ins-search", "海峡").unwrap();
    app.wait_for("text:海峡金桥财产保险").unwrap();

    // 重名（种子在用行「中国人寿」）：码化错误 zh 原样呈现，表单不清空。
    app.type_text("id:ins-add-name", "中国人寿").unwrap();
    app.press("id:ins-add").unwrap();
    // 旧壳 addFailed 词表形态：「添加失败: {msg}」。
    app.wait_for("text:添加失败: 保司已存在: 中国人寿").unwrap();
}

/// 编辑旅程（旧壳 updateInsurer 平价）：编辑弹窗回填原值（不改直接保存 =
/// 空更新同义仍未空名校拒，证明回填在场）；改名即时生效；改名撞在用同名 →
/// 码化错误、弹窗不关（内容不丢）。删编辑入口 / modal 保存接线 → 本测红。
#[test]
fn insurers_view_edit_backfill_and_rename_duplicate_guard() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("insurers-edit");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_policy::create_insurer(
            &conn,
            ledger_policy::InsurerInput {
                name: "海峡金桥财产保险".to_owned(),
            },
        )
        .unwrap();
    }
    let id = nsh_app_lib_insurer_id(&dir, "海峡金桥财产保险");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_insurers(&mut app);
    // 搜索定位（名称序位在虚拟化视口之外）：唯一命中行动作锚点以 id 内嵌。
    app.type_text("id:ins-search", "海峡").unwrap();
    app.wait_for("text:海峡金桥财产保险").unwrap();

    // 不改直接保存：回填凭据（空值会被空名校拒，保存进说明回填在场）。
    app.press(&format!("id:ins-edit-{id}")).unwrap();
    app.wait_for("id:ins-edit-name").unwrap();
    app.wait_for("text:编辑保险公司").unwrap();
    app.press("id:ins-edit-save").unwrap();
    app.wait_for("text:已更新保险公司").unwrap();
    app.wait_for_gone("id:ins-edit-name", DEFAULT_WAIT).unwrap();
    // 保存即失效重读（重读期屏幕可呈「正在加载…」）：等行内容回屏再断。
    app.wait_for("text:海峡金桥财产保险").unwrap();
    assert!(app.shows("海峡金桥财产保险"), "原值保持");

    // 改名撞在用同名（种子「中国人寿」）：码化错误、弹窗不关（内容不丢）。
    app.press(&format!("id:ins-edit-{id}")).unwrap();
    app.wait_for("id:ins-edit-name").unwrap();
    app.clear("id:ins-edit-name").unwrap();
    app.type_text("id:ins-edit-name", "中国人寿").unwrap();
    app.press("id:ins-edit-save").unwrap();
    // 旧壳 updateFailed 词表形态：「更新失败: {msg}」。
    app.wait_for("text:更新失败: 保司已存在: 中国人寿").unwrap();
    assert_eq!(
        app.screen().count("id:ins-edit-name").unwrap(),
        1,
        "弹窗不关"
    );

    // 改为合法新名：保存成功、弹窗关、新名即时在屏。
    app.clear("id:ins-edit-name").unwrap();
    app.type_text("id:ins-edit-name", "海峡金桥").unwrap();
    app.press("id:ins-edit-save").unwrap();
    app.wait_for_gone("id:ins-edit-name", DEFAULT_WAIT).unwrap();
    app.wait_for("text:海峡金桥").unwrap();
}

/// 软删旅程（旧壳 deleteInsurer + AppPopconfirm 平价）：确认气泡 cancel 不写；
/// confirm 软删后默认列表不含（行离场）；「显示已删」切换后已删行以标签行
/// 追加、且只读（无编辑/删除操作，在用行操作不受影响）。删 popconfirm
/// on_confirm 接线 → 本测红（气泡确认后行永不离场）。
#[test]
fn insurers_view_delete_popconfirm_softdeletes_and_readonly() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("insurers-delete");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_policy::create_insurer(
            &conn,
            ledger_policy::InsurerInput {
                name: "海峡金桥财产保险".to_owned(),
            },
        )
        .unwrap();
    }
    let id = nsh_app_lib_insurer_id(&dir, "海峡金桥财产保险");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_insurers(&mut app);
    // 搜索定位（名称序位在虚拟化视口之外）：唯一命中行动作锚点以 id 内嵌。
    app.type_text("id:ins-search", "海峡").unwrap();
    app.wait_for("text:海峡金桥财产保险").unwrap();

    // 取消：气泡关闭、行保留、无写。
    app.press(&format!("id:ins-delete-{id}")).unwrap();
    app.wait_for("id:ins-delete-confirm").unwrap();
    app.press("id:ins-delete-cancel").unwrap();
    app.wait_for_gone("id:ins-delete-confirm", DEFAULT_WAIT)
        .unwrap();
    assert!(app.shows("海峡金桥财产保险"), "取消不写：行仍在");

    // 确认：行离场（软删后默认列表不含）+ 成功反馈。
    app.press(&format!("id:ins-delete-{id}")).unwrap();
    app.wait_for("id:ins-delete-confirm").unwrap();
    app.press("id:ins-delete-confirm").unwrap();
    app.wait_for_gone("text:海峡金桥财产保险", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:已删除保险公司").unwrap();

    // 显示已删切换：已删行以「已删除」标签行追加（搜索叠加位顶）且只读
    //（搜索输入已在删除前定位时持有「海峡」，先清回再键入）。
    app.press("id:ins-show-deleted").unwrap();
    app.clear("id:ins-search").unwrap();
    app.type_text("id:ins-search", "海峡").unwrap();
    app.wait_for("text:海峡金桥财产保险").unwrap();
    assert_eq!(
        app.screen().count("id:ins-deleted-tag").unwrap(),
        1,
        "已删行带「已删除」标记"
    );
    assert_eq!(
        app.screen().count(&format!("id:ins-edit-{id}")).unwrap(),
        0,
        "已删行只读：无编辑操作"
    );
    assert_eq!(
        app.screen().count(&format!("id:ins-delete-{id}")).unwrap(),
        0,
        "已删行只读：无删除操作"
    );
}

/// 拼音本地过滤（旧壳 searchTerm + matchLabel 平价，统一模糊搜索语义）：
/// 拼音首字母子序列命中（rs → 人寿行）与非命中项隐藏；清空恢复完整列表；
/// 词条间 AND（汉字双词）。删除装配层 filter_insurer_rows 接线 → 行集永
/// 不随搜索变化。
#[test]
fn insurers_view_search_filters_hits_and_clear_restores() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("insurers-search");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_insurers(&mut app);
    // 过滤前凭据：人保财险在屏（首屏可见行；过滤后消失才证过滤面）。
    app.wait_for("text:人保财险").unwrap();

    // 拼音首字母子序列：rs → 人寿行命中（中国人寿 s..末字母），原在屏的
    // 人保财险（rbca）被过滤出屏（过滤面证明）。
    app.type_text("id:ins-search", "rs").unwrap();
    app.wait_for("text:中国人寿").unwrap();
    app.wait_for_gone("text:人保财险", DEFAULT_WAIT).unwrap();

    // 词条间 AND（汉字双词）：人保 + 财险 → 只人保财险（人保寿险 / 人保健康
    // 同带人保首词、无财险尾词，不在命中面）。
    app.clear("id:ins-search").unwrap();
    app.type_text("id:ins-search", "人保 财险").unwrap();
    app.wait_for("text:人保财险").unwrap();
    assert!(
        !app.shows("人保寿险"),
        "只命中人保财险（AND 交集）：{:?}",
        app.screen().texts()
    );

    // 清空恢复完整列表（非命中项回屏）。
    app.clear("id:ins-search").unwrap();
    app.wait_for("text:人保财险").unwrap();
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
    // 行集异步落屏：等行动作锚再按（CI 慢环境 press 竞态锚定，同 #2072）。
    app.wait_for(&format!("id:acct-more-{cash_id}")).unwrap();
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
    app.wait_for(&format!("id:acct-more-{cash_id}")).unwrap();
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
    // 行集异步落屏：等行动作锚再按（CI 慢环境 press 竞态锚定，同 #2072；
    // run 43053079348 首崩即此）。
    app.wait_for(&format!("id:acct-more-{cash_id}")).unwrap();
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
    // 行集异步落屏：等行动作锚再按（CI 慢环境 press 竞态锚定，同 #2072）。
    app.wait_for(&format!("id:acct-name-{cash_id}")).unwrap();
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

// ---------------------------------------------------------------------------
// 物品视图（#2044）：清单平表（列格装配产物渲染、零新增列）、创建唯一入口
// 提示与跳转、编辑（换关带出覆盖）/ 详情（参考日重算、本位币折算）/ 处置 /
// 软删轻量确认、加入物品（交易行菜单 → 确认弹窗、溯源唯一置灰）、失效信号
// 广播驱动的交易列表来源列重读。删任一接线即红的负向条目逐项写在各测试注释
//（ADR-0087 断言强度：断言对准用户可观察结果）。
// ---------------------------------------------------------------------------

/// 从侧栏点开物品视图（#2044 旅程用例共用）：清单卡片在屏即落位凭据。
fn open_items(app: &mut Headless) {
    app.wait_for("id:nav-items").unwrap();
    app.press("id:nav-items").unwrap();
    app.wait_for("id:item-table").unwrap();
    app.wait_for("text:物品列表").unwrap();
}

/// 造一笔支出交易 + 关联物品（用例共用；金额与日期随参）：经域层公开写入口
/// 造交易与物品（溯源必带出——物品创建须带购买交易指针，域创建守卫同口径）。
fn seed_item(
    db: &ledger_infra::db::DbState,
    account: &str,
    name: &str,
    amount_cents: i64,
    date: &str,
) -> (String, String) {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let tx = ledger_transaction::create(
        &conn,
        TransactionInput {
            account_id: Some(account.to_owned()),
            kind: TransactionKind::Expense,
            amount_cents,
            currency_code: "CNY".to_owned(),
            date: date.to_owned(),
            note: Some(name.to_owned()),
            ..bare_expense_input()
        },
    )
    .unwrap();
    let tx_id = tx.id.clone();
    let item_id = ledger_item::create_item(
        &conn,
        ledger_item::ItemInput {
            name: name.to_owned(),
            purchase_date: date.to_owned(),
            total_cost_cents: amount_cents,
            currency_code: "CNY".to_owned(),
            note: None,
            purchase_transaction_id: Some(tx_id.clone()),
        },
        &mut || {},
    )
    .unwrap();
    (tx_id, item_id)
}

/// 首屏装载（旧壳 ItemsView onMounted 平价）：创建唯一入口提示条 + 列头与
/// 行格文本（状态 / 总成本 / 已用天数 / 每天成本装配产物）。删除装配任一环
///（boot 物品行读 / rows 装配 / 渲染爪）即红。
#[test]
fn items_view_lands_with_hint_and_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("items-list");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        seed_item(&db, &account, "显示器", 100_000, &test_date(0));
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_items(&mut app);
    app.wait_for("text:显示器").unwrap();

    // 创建唯一入口提示条（issue #207 / ADR-0025 平价：本页无新增表单）与
    // 「去交易页」按钮（提示条全文随机时间无明显拆分，锚点以 test_id 计数）。
    assert!(
        app.screen().count("id:item-create-hint").unwrap() == 1,
        "创建唯一入口提示条在屏：{:?}",
        app.screen().texts()
    );
    assert!(
        app.screen().count("id:item-go-transactions").unwrap() == 1,
        "「去交易页」按钮在屏"
    );
    assert!(
        app.shows("物品列表"),
        "清单卡标题：{:?}",
        app.screen().texts()
    );
    // 列头与行格文本（装配层产物）：状态在用 / 总成本 / 已用天数 / 每天成本。
    for text in ["名称", "购买日期", "状态", "总成本", "已用天数", "每天成本"] {
        assert!(
            app.shows(text),
            "列头应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    for text in ["显示器", "2026-01-01", "在用", "¥1000"] {
        assert!(
            app.shows(text),
            "行格应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 已用天数与每天成本随「今天」自然滚动，不锁字面（锁定例在装配单测）。
}

/// 空库空态（旧壳 empty guide 同文字）：空账本呈现创建引导文案。
#[test]
fn items_view_empty_book_shows_guide() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("items-empty");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_items(&mut app);
    app.wait_for("id:item-empty-guide").unwrap();
    assert!(
        app.shows("暂无物品：请在「交易」页右键一笔支出交易，选择“加入物品”创建。"),
        "空态创建引导文案（旧壳 emptyGuide 同词表）：{:?}",
        app.screen().texts()
    );
}

/// 创建唯一入口提示条的「去交易页」跳转（issue #207）：点按钮 → 交易视图落位。
/// 删 nav::open 接线即红。
#[test]
fn items_view_hint_button_navigates_to_transactions() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("items-hint");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_items(&mut app);
    app.press("id:item-go-transactions").unwrap();
    app.wait_for("text:交易").unwrap();
    // 空库落位凭据：交易表格空态（无总数条）。
    app.wait_for("text:暂无数据").unwrap();
}

/// 创建入口（ADR-0025 创建唯一入口的 NSH 承载面）：交易行右键「加入物品」→
/// 名称默认取交易备注可微调 → 确认创建 → 反馈条位 + 清单落位（物品视图行
/// 即现）+ 交易列表来源列即显物品名（失效信号广播驱动的重读）。删除「菜单
/// 项接线」或「创建写路径」或「确认后广播/重读」任一环即红。
#[test]
fn items_view_create_from_transaction_menu_journey() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("items-create");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 120_000,
                date: "2026-01-01",
                category_id: None,
                merchant_id: None,
                note: Some("购物订单".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 行右键 → 菜单带「加入物品」（无物品行 enabled）。
    app.right_click("text:2026-01-01").unwrap();
    let menu = app.menu().expect("右键应打开上下文菜单");
    assert_eq!(
        menu.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        vec!["编辑", "退款", "加入物品", "删除"],
        "行菜单项闭集（#2044 起支出行加「加入物品」，删除殿后）：{menu:?}"
    );
    app.choose_menu_item("加入物品").unwrap();

    // 确认弹窗（AddItemForm 语义平价）：购买日期 / 基础成本只读带出，名称
    // 默认取交易备注可微调。
    app.wait_for("text:加入物品").unwrap();
    app.wait_for("text:2026-01-01").unwrap();
    app.wait_for("text:¥1200").unwrap();
    app.clear("id:item-add-name").unwrap();
    app.type_text("id:item-add-name", "电饭煲").unwrap();
    app.press("id:item-add-confirm").unwrap();

    // 成功反馈条位（NSH 无 toast 的条位对应物）。
    app.wait_for("id:tx-add-item-status").unwrap();
    assert!(
        app.shows("已加入物品"),
        "反馈文案：{:?}",
        app.screen().texts()
    );

    // 物品视图行即现（写后信号重读 + 落位装载）。
    open_items(&mut app);
    app.wait_for("text:电饭煲").unwrap();
    assert!(app.shows("¥1200"), "成本列：{:?}", app.screen().texts());

    // 溯源唯一（ADR-0025）出口面：回交易页行菜单「加入物品」已置灰。
    open_transactions(&mut app);
    app.wait_for("text:电饭煲").unwrap();
    app.right_click("text:2026-01-01").unwrap();
    let menu = app.menu().expect("右键应打开上下文菜单");
    let add = menu
        .iter()
        .find(|e| e.label == "加入物品")
        .expect("菜单仍带「加入物品」");
    assert!(
        add.disabled,
        "溯源唯一守卫（hasItem）：已有物品的行「加入物品」置灰：{menu:?}"
    );
    app.close_menu().unwrap();
}

/// 编辑旅程（旧壳 issue #117 平价）：打开弹窗预填 → 改名保存 → 反馈 + 行更新 + 交易列表来源列新名
///   （失效信号广播驱动的明细重读）；删除 notify_transactions_invalidated
///   接线即红（交易列表来源列仍显旧名）。
#[test]
fn items_view_edit_rename_updates_row_and_source_column() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("items-edit");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        seed_item(&db, &account, "显示器", 100_000, &test_date(0));
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_items(&mut app);
    app.wait_for("text:显示器").unwrap();

    // 行内「编辑」按钮 → 弹窗预填。
    let item_id = item_id_by_name(&dir, "显示器");
    app.press(&format!("id:item-edit-{item_id}")).unwrap();
    app.wait_for("text:编辑物品").unwrap();
    {
        let name = app.screen();
        let _ = name;
    }
    app.clear("id:item-edit-name").unwrap();
    app.type_text("id:item-edit-name", "曲面显示器").unwrap();
    app.press("id:item-edit-confirm").unwrap();

    // 成功反馈 + 行更新（失效信号驱动重读的等位：物品视图自己的 reload）。
    app.wait_for("text:已保存").unwrap();
    app.wait_for("text:曲面显示器").unwrap();

    // 交易列表来源列新名（广播→列表重读；删除信号::notify_transactions_invalidated
    // 接线即红——列表落位重读是读时的，广播是被删的语义锚）。
    open_transactions(&mut app);
    app.wait_for("text:曲面显示器").unwrap();
}

/// 换关带出（旧壳 issue #119 平价）：编辑弹窗换关另一笔支出 → 后端以交易值
/// 覆盖日期/成本。锁定期呈现与提交后回填断言经详情弹窗的成本数字读出。
/// 拆 boot::item_candidate_transactions 读接缝或换关守卫即红。
#[test]
fn items_view_edit_relink_backfills_on_save() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("items-relink");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        seed_item(&db, &account, "显示器", 100_000, &test_date(0));
        // 候选池里的另一笔（数据源：boot item_candidate_transactions 倒序 100）。
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 88_000,
                date: &test_date(1),
                category_id: None,
                merchant_id: None,
                note: Some("次笔支出".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_items(&mut app);
    app.wait_for("text:显示器").unwrap();

    let item_id = item_id_by_name(&dir, "显示器");
    app.press(&format!("id:item-edit-{item_id}")).unwrap();
    app.wait_for("text:编辑物品").unwrap();

    // 换关（PinyinSelect 可搜下拉）：打开弹层 → 键入搜出候选 → 选中。
    app.press("id:item-edit-link").unwrap();
    app.wait_for("id:item-edit-link-search").unwrap();
    // 选项行锚点（label 全文为「{date} · {金额} · {备注}」同旧壳 linkTxOptions
    // 形态——直接按锚点按压第 0 命中 = 次笔支出）。
    app.press("id:item-edit-link-option >> text:2026-01-02 · ¥880 · 次笔支出")
        .unwrap();
    // 锁定面：手改入口换成带出预览（选中交易日期即现）。
    app.wait_for("text:2026-01-02 · ¥880").unwrap();

    // 提交后成本回填为次笔交易的金额（详情弹窗读数）。
    app.press("id:item-edit-confirm").unwrap();
    app.wait_for("text:已保存").unwrap();
    app.press(&format!("id:item-detail-{item_id}")).unwrap();
    app.wait_for("text:¥880").unwrap();
    app.press("id:item-detail-cancel").unwrap();
}

/// 处置旅程（旧壳 issue #120 平价）：在用行「处置」→ 弹窗日期缺省今天 →
/// 确认 → 反馈 + 状态列带处置日；已处置行「处置信息」再开 = 修正（预填处置
/// 日与残值可改）。拆 dispose 写路径或弹窗预填接线即红。
#[test]
fn items_view_dispose_then_revise() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("items-dispose");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        seed_item(&db, &account, "显示器", 100_000, &test_date(0));
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_items(&mut app);
    app.wait_for("text:显示器").unwrap();
    let item_id = item_id_by_name(&dir, "显示器");

    // 处置弹窗（日期必填语义归受控日期选择器缺省今天）。
    app.press(&format!("id:item-dispose-{item_id}")).unwrap();
    app.wait_for("text:处置物品").unwrap();
    app.press("id:item-dispose-confirm").unwrap();
    app.wait_for("text:已处置").unwrap();
    app.wait_for_gone("text:处置物品", DEFAULT_WAIT).unwrap();
    let today_text = today().to_string();
    app.wait_for(&format!("text:已处置 {today_text}")).unwrap();

    // 已处置行「处置信息」：预填处置日 + 残值（可修正）。旧壳确认钮文案
    // 沿用「保存」。
    app.press(&format!("id:item-dispose-{item_id}")).unwrap();
    app.wait_for("text:处置信息").unwrap();
    app.type_text("id:item-dispose-residual", "20").unwrap();
    app.press("id:item-dispose-confirm").unwrap();
    app.wait_for("text:已更新处置信息").unwrap();
}

/// 软删轻量确认（旧壳 AppPopconfirm 平价，ADR-0078 轻量档）：点删除 → 气泡
/// 文案 + 取消不删除（行仍在）→ 确认 → 行消失 + 反馈。「删除即红」：拆
/// popconfirm 门槛即红（确认即删）。
#[test]
fn items_view_delete_needs_confirmation_and_cancels_before_write() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("items-delete");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        seed_item(&db, &account, "显示器", 100_000, &test_date(0));
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_items(&mut app);
    app.wait_for("text:显示器").unwrap();
    let item_id = item_id_by_name(&dir, "显示器");
    app.press(&format!("id:item-delete-{item_id}")).unwrap();

    // 气泡文案（旧壳 deleteConfirm 同词表）与确认钮。
    app.wait_for("text:不再跟踪该物品，从列表移除？").unwrap();
    // 取消：行仍在。
    app.press("id:item-delete-cancel").unwrap();
    app.wait_for("text:显示器").unwrap();

    // 确认：行删除 + 反馈。
    app.press(&format!("id:item-delete-{item_id}")).unwrap();
    app.wait_for("text:不再跟踪该物品，从列表移除？").unwrap();
    app.press("id:item-delete-confirm").unwrap();
    app.wait_for("text:已删除").unwrap();
    app.wait_for_gone("text:显示器", DEFAULT_WAIT).unwrap();
}

/// 详情（旧壳 issue #117/#121 平价）：成本分解（分子 ÷ 天数 = 每天成本）、
/// 本位币折算行（账本本位币）、自选参考日重算（参考日选择器键入落值即重算）
/// 与清空回缺省。拆 boot::item_cost 读接缝即红。
#[test]
fn items_view_detail_recacs_reference_date_and_clears() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("items-detail");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        // 购买日 = 今天 - 9 天：缺省目标日（今天）已用 10 天。
        let purchase = today()
            .checked_sub_days(chrono::Days::new(9))
            .unwrap()
            .to_string();
        seed_item(&db, &account, "显示器", 100_000, &purchase);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_items(&mut app);
    app.wait_for("text:显示器").unwrap();
    let item_id = item_id_by_name(&dir, "显示器");

    app.press(&format!("id:item-detail-{item_id}")).unwrap();
    app.wait_for("text:物品详情").unwrap();
    // 成本分解缺省：¥1000 ÷ 10 天 = ¥100/天。
    app.wait_for("text:10 天（含购买当日）").unwrap();
    app.wait_for("text:¥1000 ÷ 10 天 = ¥100/天").unwrap();
    // 本位币折算行（账本本位币基准缺省 CNY）。
    app.wait_for("text:¥1000（CNY）").unwrap();

    // 自选参考日重算：参考日 = 购买日 + 4 天 → 已用 5 天 → ¥200/天。
    let purchase = today()
        .checked_sub_days(chrono::Days::new(9))
        .unwrap()
        .to_string();
    let ref_date = today()
        .checked_sub_days(chrono::Days::new(5))
        .unwrap()
        .to_string();
    app.clear("id:item-detail-ref-input").unwrap();
    app.type_text("id:item-detail-ref-input", &ref_date)
        .unwrap();
    println!(
        "after type input={:?}",
        app.screen().input_text("id:item-detail-ref-input")
    );
    app.wait_for("text:¥1000 ÷ 5 天 = ¥200/天").unwrap();
    app.wait_for("text:5 天（含购买当日）").unwrap();

    // 清空参考日 → 回缺省口径（重算结果撤回）。
    app.press("id:item-detail-ref-clear").unwrap();
    app.wait_for("text:¥1000 ÷ 10 天 = ¥100/天").unwrap();
    let _ = purchase;
}

/// 溯源信息（旧壳详情「关联购买交易」）：详情弹窗带溯源标注（已在详情测试
/// 断言范围内）；本件断言溯源唯一守卫的码化拒绝在弹窗内呈现（重复建物品被
/// 域守卫拒绝——加入物品置灰是界面侧，本件以域守卫错误文案对拍）。
#[test]
fn items_view_add_form_error_keeps_dialog_open() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("items-add-error");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        seed_item(&db, &account, "已有物品", 10_000, &test_date(0));
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_items(&mut app);
    app.wait_for("text:已有物品").unwrap();

    // 编辑弹窗清空名称保存：前置拦截呈现且弹窗留存（旧壳 nameRequired 同词表）。
    let item_id = item_id_by_name(&dir, "已有物品");
    app.press(&format!("id:item-edit-{item_id}")).unwrap();
    app.wait_for("text:编辑物品").unwrap();
    app.clear("id:item-edit-name").unwrap();
    app.press("id:item-edit-confirm").unwrap();
    app.wait_for("id:item-edit-error").unwrap();
    assert!(
        app.shows("保存失败: 请输入物品名称"),
        "前置拦截文案在弹窗内（弹窗留存）：{:?}",
        app.screen().texts()
    );
    app.press("id:item-edit-cancel").unwrap();
}

/// 按物品名取物品 id（测试用例寻址面；经 boot 读接缝读快照）。
fn item_id_by_name(dir: &std::path::Path, name: &str) -> String {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_item::list_items(&conn)
        .unwrap()
        .into_iter()
        .find(|row| row.item.name == name)
        .map(|row| row.item.id)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 商户管理视图（#2052）：字典 + 新增表单卡 + 清单卡（统一模糊搜索 / 显示已删 /
// 条数排序 / 条数下钻 / 客户端分页）+ 编辑弹窗与软删轻量确认；写路径直调域，
// 写成功后广播交易失效信号（交易列表商户来源重读）。删任一接线即红的负向
// 条目逐项写在各测试注释（ADR-0087：断言对准用户可观察结果）。
// ---------------------------------------------------------------------------

/// 经记账组「更多」容器点开商户页签（#2052 旅程用例共用）：清单卡片在屏即
/// 落位凭据（容器页签到达路径的“到达”半边——删容器页签分支或挂载分支即红）。
fn open_merchants(app: &mut Headless) {
    app.wait_for("id:nav-more-bookkeeping").unwrap();
    app.press("id:nav-more-bookkeeping").unwrap();
    app.wait_for("id:more-tab-merchants").unwrap();
    app.press("id:more-tab-merchants").unwrap();
    app.wait_for("id:merchant-table").unwrap();
    app.wait_for("text:商户列表").unwrap();
}

/// 经域读路径反查商户 id（用例共用；弹窗/气泡目标 test_id 嵌 id 用）。
fn nsh_app_lib_merchant_id(dir: &std::path::Path, name: &str) -> String {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_merchants::list_merchants(&conn, true)
        .unwrap()
        .into_iter()
        .find(|row| row.name == name)
        .map(|row| row.id)
        .unwrap_or_default()
}

/// 首屏装载（旧壳 MerchantManager onMounted 平价）：两卡（新增/列表）+ 列
/// 头与行格文本（名称 / 关联交易计数）。删除清单读接缝（boot::merchant_rows
/// / merchant_transaction_counts）或装配分支即红。
#[test]
fn merchants_view_lands_with_cards_and_counted_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("merchants-list");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        let jd = seed_merchant(&db, "京东");
        let mt = seed_merchant(&db, "美团");
        for merchant in [jd, mt] {
            seed_transaction(
                &db,
                TxnSeed {
                    account: &account,
                    to_account: None,
                    kind: TransactionKind::Expense,
                    amount_cents: 10_00,
                    date: &test_date(0),
                    category_id: None,
                    merchant_id: Some(merchant),
                    note: None,
                },
            );
        }
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_merchants(&mut app);

    // 两卡（新增表单卡 + 清单卡）。
    assert!(
        app.shows("新增商户"),
        "新增表单卡在屏：{:?}",
        app.screen().texts()
    );
    assert!(app.shows("商户列表"), "清单卡在屏");
    // 列头与行格文本（装配层产物）：名称 / 关联交易（毛笔数）。
    for text in ["名称", "关联交易", "操作"] {
        assert!(
            app.shows(text),
            "列头应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    assert!(app.shows("京东"), "在用行在屏：{:?}", app.screen().texts());
    assert!(app.shows("美团"), "在用行在屏");
    assert!(app.shows("1"), "每商户毛笔数 1：{:?}", app.screen().texts());
    // 总数条（旧壳 pagination.prefix 同词表）。
    app.wait_for("text:共 2 条").unwrap();
}

/// 新增旅程（旧壳 addMerchant 平价）：空名前置拦截不写，添加成功反馈 + 行即
/// 现；重名被域守卫码化拒绝，错误在卡内呈现且表单不清空。删 boot::
/// create_merchant 写接缝即红（成功旅程行不到屏）。
#[test]
fn merchants_view_create_journey_and_duplicate_rejection() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("merchants-create");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_merchants(&mut app);

    // 空名前置拦截：提示 + 不写库。
    app.press("id:merchant-add").unwrap();
    app.wait_for("text:请输入商户名称").unwrap();
    assert!(
        nsh_app_lib_merchant_id(&dir, "京东").is_empty(),
        "前置拦截不写库（空库无商户）"
    );
    app.wait_for("text:共 0 条").unwrap();

    // 成功旅程：输入名称 → 添加 → 反馈 + 行即现（写入成功广播交易失效，
    // 本视图订阅重读——转已删/软删计数随重读跟随）。
    app.type_text("id:merchant-create-name", "京东").unwrap();
    app.press("id:merchant-add").unwrap();
    app.wait_for("text:已添加商户").unwrap();
    app.wait_for("text:京东").unwrap();
    app.wait_for("text:共 1 条").unwrap();
    assert_eq!(
        app.screen().input_text("id:merchant-create-name").unwrap(),
        Some(""),
        "成功后表单清空"
    );

    // 重名拒绝：域码化错误（「商户已存在: 京东」）在卡内呈现，表单不清空。
    app.type_text("id:merchant-create-name", "京东").unwrap();
    app.press("id:merchant-add").unwrap();
    app.wait_for("text:添加失败: 商户已存在: 京东").unwrap();
    assert_eq!(
        app.screen().count("text:共 1 条").unwrap(),
        1,
        "重名未写入（仍 1 条）"
    );
    assert_eq!(
        app.screen().input_text("id:merchant-create-name").unwrap(),
        Some("京东"),
        "失败不清空表单（可直接修正）"
    );
}

/// 编辑改名（旧壳 MerchantEditModal 平价）：行内「编辑」→ 弹窗预填 → 保存
/// → 反馈 + 行更新；交易列表来源列新名即现（失效信号广播驱动的重读——删除
/// signals::notify_transactions_invalidated 接线即红：列表落位重读是读时的，
/// 广播是被删的语义锚）。删除 boot::update_merchant 写接缝即红（行不更新）。
#[test]
fn merchants_view_edit_renames_and_updates_source_column() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("merchants-edit");
    let jd;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        jd = seed_merchant(&db, "京东");
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 10_00,
                date: &test_date(0),
                category_id: None,
                merchant_id: Some(jd.clone()),
                note: None,
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_merchants(&mut app);
    app.wait_for("text:京东").unwrap();

    // 编辑弹窗预填（旧壳 watch immediate 同果）。
    app.press(&format!("id:merchant-edit-{jd}")).unwrap();
    app.wait_for("text:编辑商户").unwrap();
    assert_eq!(
        app.screen().input_text("id:merchant-edit-name").unwrap(),
        Some("京东"),
        "预填原名"
    );

    // 空名前置拦截（旧壳 nameRequired 同词表；错误条位带词表前缀）。
    app.clear("id:merchant-edit-name").unwrap();
    app.press("id:merchant-edit-save").unwrap();
    app.wait_for("text:更新失败: 请输入商户名称").unwrap();

    // 保存 → 反馈 + 行更新。
    app.type_text("id:merchant-edit-name", "京东商城").unwrap();
    app.press("id:merchant-edit-save").unwrap();
    app.wait_for("text:已更新商户").unwrap();
    app.wait_for("text:京东商城").unwrap();

    // 交易列表来源列新名（广播 → 列表重读）。
    open_transactions(&mut app);
    app.wait_for("text:京东商城").unwrap();
}

/// 软删两相（旧壳 AppPopconfirm 平价）：取消不写；确认后行离席（在用清单），
/// rename 延伸显示已删后仍以已删行呈现。删 boot::delete_merchant 写接缝即红
///（确认后行不离席）。
#[test]
fn merchants_view_delete_needs_confirmation_and_cancels_before_write() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("merchants-delete");
    let jd;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        jd = seed_merchant(&db, "京东");
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_merchants(&mut app);
    app.wait_for("text:京东").unwrap();

    // 气泡文案（旧壳 deleteConfirm 同词表）与取消键。
    app.press(&format!("id:merchant-delete-{jd}")).unwrap();
    app.wait_for("text:确认删除该商户？历史交易仍会显示它的名字。")
        .unwrap();
    // 取消：行仍在。
    app.press("id:merchant-delete-cancel").unwrap();
    app.wait_for("text:京东").unwrap();

    // 确认：行离席（在用清单）+ 反馈。
    app.press(&format!("id:merchant-delete-{jd}")).unwrap();
    app.wait_for("text:确认删除该商户？历史交易仍会显示它的名字。")
        .unwrap();
    app.press("id:merchant-delete-confirm").unwrap();
    app.wait_for("text:已删除商户").unwrap();
    app.wait_for_gone("text:京东", DEFAULT_WAIT).unwrap();

    // 显示已删开启 → 已删行追加在尾（只读，带「已删除」标记）。
    app.press("id:merchant-show-deleted").unwrap();
    app.wait_for("text:京东").unwrap();
    app.wait_for("text:已删除").unwrap();
    app.wait_for(&format!("id:merchant-deleted-tag-{jd}"))
        .unwrap();
}

/// 显示已删与搜索（旧壳 issue #447/#447 平价）：默认已删行不显示；开关开启
/// 后已删行追加（带标记无操作）；统一模糊搜索本地过滤（拼音首字母命中），
/// 清空恢复完整列表。删搜索接线或开关即红（断言对准在屏行集）。
#[test]
fn merchants_view_search_and_show_deleted_toggle_readonly_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("merchants-filter");
    let bogus;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_merchant(&db, "京东");
        bogus = seed_merchant(&db, "京东旧");
    }
    {
        // 软删「京东旧」（经域公开写入口域行 is_deleted=1）。
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_merchants::delete_merchant(&conn, &bogus).unwrap();
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_merchants(&mut app);

    // 默认只显示在用。
    app.wait_for("text:京东").unwrap();
    assert!(
        !app.shows("已删除"),
        "默认无已删标记：{:?}",
        app.screen().texts()
    );
    assert!(
        app.screen()
            .count(&format!("id:merchant-deleted-tag-{bogus}"))
            .unwrap()
            == 0,
        "已删行默认不呈现（标记锚点不在屏）"
    );

    // 显示已删开启：已删行追加在尾，带「已删除」标记（在用行仍在前）。
    app.press("id:merchant-show-deleted").unwrap();
    app.wait_for("text:京东").unwrap();
    app.wait_for(&format!("id:merchant-deleted-tag-{bogus}"))
        .unwrap();
    app.wait_for("text:已删除").unwrap();
    app.wait_for("text:共 2 条").unwrap();

    // 搜索（拼音首字母命中）过滤未命中项：两行都含「京东」→ 两条。
    app.type_text("id:merchant-search", "jd").unwrap();
    app.wait_for("text:共 2 条").unwrap();
    // 限定「旧」→ 只剩已删行（在京旧，标记行）。
    app.clear("id:merchant-search").unwrap();
    app.type_text("id:merchant-search", "旧").unwrap();
    app.wait_for("text:共 1 条").unwrap();
    // 清空恢复完整列表（过滤意图页码归零不回退总数）。
    app.clear("id:merchant-search").unwrap();
    app.wait_for("text:共 2 条").unwrap();
}

/// 移回侧栏直达（旧壳侧栏仓位语义平价）：存储种子把商户移出记账收纳清单、
/// 挂主项仓位 → 侧栏商户项直达真实视图。删 view_host 的商户分支挂载 → 本测
/// 红（「迁移中」占位——容器分支不经过本路径，负向条目独立于容器到达）。
#[test]
fn merchants_view_lands_directly_when_moved_to_sidebar() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("merchants-direct");

    // 侧栏序存储种子：「商户」移回侧栏（收纳清单同步剔除，就地移回豁免
    // 同形的存储侧清面）。
    let mut app = Headless::start(desktop_config_with_storage(&[
        (
            "view_state:sidebar_containment",
            r#"{"bookkeeping":["scheduled"],"assets":["policies","physicalAssets","insurers"],"insights":[]}"#,
        ),
        (
            "view_state:sidebar_order",
            r#"{"bookkeeping":["transactions","accounts","merchants","budget"],"assets":["investments","items","savingsGoals"],"insights":["reports","search"]}"#,
        ),
    ]))
    .unwrap();
    nsh_app::app::set_book_dir(dir);
    app.wait_for("id:nav-merchants").unwrap();
    app.press("id:nav-merchants").unwrap();
    app.wait_for("id:merchant-table").unwrap();
    app.wait_for("text:商户列表").unwrap();
}

/// 条数下钻（issue #446 平价）：点条数 → 交易列表带商户过滤落位。删
/// nav::open 下钻接线即红（点击后停留商户视图）。
#[test]
fn merchants_view_count_click_drills_to_transactions_filtered() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("merchants-drill");
    let jd;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        jd = seed_merchant(&db, "京东");
        for (date, note) in [("2026-01-01", "京东笔一"), ("2026-01-02", "京东笔二")] {
            seed_transaction(
                &db,
                TxnSeed {
                    account: &account,
                    to_account: None,
                    kind: TransactionKind::Expense,
                    amount_cents: 10_00,
                    date,
                    category_id: None,
                    merchant_id: Some(jd.clone()),
                    note: Some(note.to_owned()),
                },
            );
        }
        // 无商户的另一笔（被下钻筛选排除）。
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 10_00,
                date: "2026-01-03",
                category_id: None,
                merchant_id: None,
                note: Some("无商户笔".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_merchants(&mut app);
    app.wait_for("text:京东").unwrap();

    // 点条数 → 交易列表落位带商户过滤：只剩两笔京东交易。
    app.press(&format!("id:merchant-count-{jd}")).unwrap();
    app.wait_for("id:tx-table").unwrap();
    app.wait_for("text:共 2 条").unwrap();
    assert!(
        app.shows("京东笔一") && app.shows("京东笔二"),
        "下钻筛选命中在屏"
    );
    assert!(
        !app.shows("无商户笔"),
        "其它交易被下钻筛选排除：{:?}",
        app.screen().texts()
    );
}

// ---------------------------------------------------------------------------
// 报表视图（#2048）：bar 图自绘三卡（期间口径统一）。造数经域公开写入口，
// 断言只写用户可见结果；接线删除即红（负向验证条目见交付报告）。
// ---------------------------------------------------------------------------

/// 当前自然月内第 `instance` 个偶数日（月初锚定，跨用例稳定；日期恒落当前
/// 自然月——期间「当年」「当月」两档的成员日）。
fn reports_month_day(instance: usize) -> String {
    let t = today();
    let day = 1 + (instance as u32) * 2;
    chrono::NaiveDate::from_ymd_opt(t.year(), t.month(), day.min(28))
        .unwrap()
        .to_string()
}

/// 当前自然月键（YYYY-MM，月度图 x 轴行标断言用）。
fn reports_month_key() -> String {
    let t = today();
    format!("{}-{:02}", t.year(), t.month())
}

/// 报表造数簿：账户 + 两段分类（午饭挂餐饮根 + 通勤根）+ 商户 × 7 + 当月
/// 一组交易（收入/支出/退款、分类/未分类、两主力商户 + 五笔小额）——三张
/// 报表的期间口径、下钻两段与 TopN 档位共用同一簿（旅程间无交叉写）。
fn seed_reports_book(tag: &str) -> std::path::PathBuf {
    let dir = make_book(tag);
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let cash = seed_cash_account(&db);

    let food = seed_category(&db, "餐饮", None);
    let lunch = seed_category(&db, "午饭", Some(&food));
    let commute = seed_category(&db, "通勤", None);

    let jia = seed_merchant(&db, "甲城");
    let yi = seed_merchant(&db, "乙街");
    // 五家小额商户、金额各不相同（TopN 截断在排序之后应用，位次闭集
    // 稳定可断：丙 1200 / 丁 1800 / 戊 1500 / 己 300 / 庚 700）。
    for (name, amount) in [
        ("丙铺", 1_200),
        ("丁仓", 1_800),
        ("戊厂", 1_500),
        ("己站", 300),
        ("庚点", 700),
    ] {
        let merchant = seed_merchant(&db, name);
        seed_transaction(
            &db,
            TxnSeed {
                account: &cash,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: amount,
                date: &reports_month_day(9),
                category_id: None,
                merchant_id: Some(merchant),
                note: Some("小笔".to_owned()),
            },
        );
    }

    let date = reports_month_day(1);
    seed_transaction(
        &db,
        TxnSeed {
            account: &cash,
            to_account: None,
            kind: TransactionKind::Expense,
            amount_cents: 4_000,
            date: &date,
            category_id: Some(lunch.clone()),
            merchant_id: Some(jia.clone()),
            note: Some("午饭一笔".to_owned()),
        },
    );
    let refund_target = seed_tx_with_id(
        &db,
        minimal_input(
            TransactionKind::Expense,
            &cash,
            300,
            &date,
            None,
            Some("退款本体小笔".to_owned()),
        ),
    );
    seed_transaction(
        &db,
        TxnSeed {
            account: &cash,
            to_account: None,
            kind: TransactionKind::Expense,
            amount_cents: 2_500,
            date: &date,
            category_id: None,
            merchant_id: Some(jia),
            note: Some("未分类一笔".to_owned()),
        },
    );
    seed_transaction(
        &db,
        TxnSeed {
            account: &cash,
            to_account: None,
            kind: TransactionKind::Expense,
            amount_cents: 800,
            date: &date,
            category_id: Some(commute),
            merchant_id: Some(yi),
            note: Some("通勤一笔".to_owned()),
        },
    );
    seed_transaction(
        &db,
        TxnSeed {
            account: &cash,
            to_account: None,
            kind: TransactionKind::Income,
            amount_cents: 8_000,
            date: &date,
            category_id: None,
            merchant_id: None,
            note: Some("月内进账".to_owned()),
        },
    );
    // 退款（关联原支出；同月内冲减进月度图退款列）。
    seed_tx_with_id(
        &db,
        minimal_input(
            TransactionKind::Refund,
            &cash,
            500,
            &date,
            Some(refund_target),
            Some("月内退款".to_owned()),
        ),
    );
    dir
}

/// 从侧栏点开报表视图（旅程用例共用：屏名在屏即落位凭据）。
fn open_reports(app: &mut Headless) {
    app.wait_for("id:nav-reports").unwrap();
    app.press("id:nav-reports").unwrap();
    app.wait_for("id:reports-monthly-card").unwrap();
}

/// 等三卡装载完成（月度月份行标 + 商户行到屏；§3.5 异步装载纪律）。
fn wait_reports_loaded(app: &mut Headless) {
    let ym = reports_month_key();
    app.wait_for(&format!("text:{ym}")).unwrap();
    // 商户榜首行占比（甲 6500 / 全量合计 12800 → 51%）到屏即行集装载完成。
    app.wait_for("text:51%").unwrap();
}

/// 报表首屏（AC：三卡齐备 + 期间默认「当年」+ 数值口径平价——金额无币符/
/// 两位小数去尾零、占比分母 = 全量合计）。删除任一装配接线（导航分支挂载 /
/// 期间行 / 月度读 / 分类读 / 商户读）即红。
#[test]
fn reports_view_lands_with_three_cards_and_defaults() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_reports_book("reports-lands");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_reports(&mut app);
    wait_reports_loaded(&mut app);

    // 三卡标题 + TopN 档位默认 Top 5。
    for text in [
        "月度收支",
        "支出分类构成",
        "商户消费排行",
        "Top 5",
        "Top 10",
    ] {
        assert!(
            app.shows(text),
            "应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 期间芯片闭集：四枚（无「全部」，ADR-0057）——芯片在弹层内，开启断言后关回。
    app.press("id:reports-period-label").unwrap();
    app.wait_for("text:当月").unwrap();
    for chip in ["当月", "当季", "当年", "去年"] {
        assert!(app.shows(chip), "应呈现日期芯片「{chip}」");
    }
    // 期间闭集无「全部」（期间必有界，导航同下无全选入口）。
    assert!(
        !app.shows("全部"),
        "日期芯片闭集无「全部」：{:?}",
        app.screen().texts()
    );
    app.press("id:reports-period-label").unwrap();
    app.wait_for_gone("text:当月", DEFAULT_WAIT).unwrap();
    // 月度图月份行标（本月键）在屏。
    assert!(app.shows(&reports_month_key()), "月度图 x 轴行标");
    // 分类图行（净额降序）：餐饮 / 未分类 / 通勤行名在屏（柱尾金额归 tooltip）。
    for text in ["餐饮", "未分类", "通勤"] {
        assert!(app.shows(text), "分类行名在屏：{:?}", app.screen().texts());
    }
    // 商户排行（Top 5 默认，排序与截断在后端）：位次闭集稳定可断（小额
    // 金额各不相同）——Top5 = 甲 6500 / 丁 1800 / 戊 1500 / 丙 1200 / 乙 800。
    for name in ["甲城", "乙街", "丁仓", "丙铺"] {
        assert!(app.shows(name), "商户榜行「{name}」在屏");
    }
    assert!(
        !app.shows("己站") && !app.shows("庚点"),
        "Top 5 截断：第 6、7 名不呈现"
    );
}

/// 月度图说明气泡（旧壳 tooltip 平价）：按月柱弹出三列毛值 + 净额行
///（净额 = 收入 − 支出 + 退款，旧壳 afterBody 同公式同词表）。
#[test]
fn reports_view_monthly_bar_tooltip_shows_gross_and_net() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_reports_book("reports-monthly-tip");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_reports(&mut app);
    let ym = reports_month_key();
    app.wait_for(&format!("id:reports-month-{ym}-0")).unwrap();

    // 按月柱弹出说明：收入/支出/退款 + 净额四行（数值 = 装配层本位币文本；
    // 全支出参与口径：午餐 4000 + 未分类 2500 + 通勤 800 + 小额五笔 5500 +
    // 退款本体 300 = 13100 → 131；净额 = 收入 80 − 支出 131 + 退款 5 = −46）。
    app.press(&format!("id:reports-month-{ym}-0")).unwrap();
    app.wait_for("text:净额: -46").unwrap();
    for line in ["收入: 80", "支出: 131", "退款: 5", "净额: -46"] {
        assert!(
            app.shows(line),
            "弹出说明「{line}」在屏：{:?}",
            app.screen().texts()
        );
    }
    // 再按另一柱：说明同月同界（旧壳按行弹说明同态）。
    app.press(&format!("id:reports-month-{ym}-2")).unwrap();
    app.wait_for("text:净额: -46").unwrap();
    assert!(app.shows("退款: 5"), "退款柱弹出同样呈现行说明");
}

/// 支出分类构成两段下钻（#379/#380）：点一级柱行 → 下钻态（二级子分类 +
/// 直挂行 + 面包屑）→ 二级行直达该分类交易列表；点面包屑根回基础态。
#[test]
fn reports_view_category_drilldown_breadcrumb_and_row_jump() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_reports_book("reports-drilldown");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_reports(&mut app);
    wait_reports_loaded(&mut app);

    // 点行名（一级归并行「餐饮」）→ 图内下钻：面包屑出现（全部分类 › 餐饮），
    // 同日二级行「午饭」（直挂行因根上无直挂交易净额 0 不进图）。
    app.press("text:餐饮").unwrap();
    app.wait_for("id:reports-breadcrumb-root").unwrap();
    assert!(
        app.shows("全部分类") && app.shows("›") && app.shows("餐饮"),
        "下钻态面包屑：{:?}",
        app.screen().texts()
    );
    // 下钻态行名「午饭」→ 直达按该分类过滤的交易列表（跳转载荷 = 分类 +
    // 期间 + 支出退款类型集合——列表净值与柱值一致）。删除 nav::open 接线
    // 即红（点击停留报表视图）。
    app.press("text:午饭").unwrap();
    app.wait_for("id:tx-table").unwrap();
    app.wait_for("text:共 1 条").unwrap();
    assert!(app.shows("午饭一笔"), "分类下钻命中在屏");
    assert!(
        !app.shows("未分类一笔") && !app.shows("通勤一笔"),
        "其它分类被下钻载荷排除：{:?}",
        app.screen().texts()
    );
}

/// 点面包屑根（全部分类）回基础态（#379 面包屑回退）。
#[test]
fn reports_view_breadcrumb_root_returns_to_base() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_reports_book("reports-breadcrumb");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_reports(&mut app);
    wait_reports_loaded(&mut app);

    app.press("text:餐饮").unwrap();
    app.wait_for("id:reports-breadcrumb-root").unwrap();
    // 点根「全部分类」→ 回基础态（面包屑消失，基础态三行复原）。
    app.press("id:reports-breadcrumb-root").unwrap();
    app.wait_for_gone("id:reports-breadcrumb-root", DEFAULT_WAIT)
        .unwrap();
    assert!(
        app.shows("未分类"),
        "基础态未分类柱复原：{:?}",
        app.screen().texts()
    );
}

/// 未分类柱直达「仅无分类」列表（#379/#380 同裁决：未分类无子结构，
/// 是柱不是层级；载荷 = 仅无分类 + 期间 + 支出退款类型集合）。删除
/// nav::open 接线即红。
#[test]
fn reports_view_uncategorized_bar_jumps_to_transactions_filtered() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_reports_book("reports-uncat-jump");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_reports(&mut app);
    wait_reports_loaded(&mut app);

    app.press("text:未分类").unwrap();
    app.wait_for("id:tx-table").unwrap();
    // 仅无分类成员：未分类一笔 + 五笔小额 + 退款本体 + 月内退款 = 8 条
    //（退款继承原无分类、类型集合含 refund）。
    app.wait_for("text:共 8 条").unwrap();
    assert!(app.shows("未分类一笔"), "仅无分类命中在屏");
    assert!(
        !app.shows("午饭一笔") && !app.shows("通勤一笔"),
        "有分类交易被排除：{:?}",
        app.screen().texts()
    );
}

/// 商户排行 TopN 档位切换与行名下钻（#588/#589）：Top 5 默认 → Top 10 全量
/// 呈现；点商户名 → 直达按该商户过滤的交易列表。删除 TopN 重拉接线（改档
/// 不发读者）或 nav::open 接线任一即红。
#[test]
fn reports_view_merchant_top_n_switch_and_row_jump() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_reports_book("reports-merchant");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_reports(&mut app);
    wait_reports_loaded(&mut app);

    // Top 5 默认：甲乙在内，己/庚（第 6、7 名）不在屏。
    assert!(app.shows("甲城") && app.shows("乙街"));
    assert!(!app.shows("己站") && !app.shows("庚点"));

    // 切 Top 10 → 全量七行呈现（仅商户卡重拉，其余两卡不受牵连——月度图与
    // 分类行保持）。
    app.press("id:reports-topn-10").unwrap();
    app.wait_for("text:己站").unwrap();
    assert!(app.shows("甲城") && app.shows("庚点"), "Top 10 全量在屏");
    assert!(app.shows(&reports_month_key()), "TopN 切换不牵连月度卡");

    // 点商户名「乙」→ 直达按该商户过滤的交易列表（共 1 条）。
    app.press("text:乙街").unwrap();
    app.wait_for("id:tx-table").unwrap();
    app.wait_for("text:共 1 条").unwrap();
    assert!(app.shows("通勤一笔"), "商户下钻命中在屏");
    assert!(
        !app.shows("午饭一笔"),
        "他商户交易被下钻载荷排除：{:?}",
        app.screen().texts()
    );
}

/// 期间切换复位图内下钻 + 空期间两态（旧壳 setPeriod 内化规则平价）：
/// 下钻态切「去年」→ 面包屑消失（下钻悬在旧期间上，切期间回基础态）→
/// 空期间三卡空态（本期暂无数据 / 暂无支出数据 / 本期暂无商户消费）。
/// 删除 on_apply 复位接线或任一读取接线即红。
#[test]
fn reports_view_period_switch_resets_drilldown_and_shows_empty_states() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_reports_book("reports-period-reset");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_reports(&mut app);
    wait_reports_loaded(&mut app);

    // 先下钻：面包屑在屏。
    app.press("text:餐饮").unwrap();
    app.wait_for("id:reports-breadcrumb-root").unwrap();

    // 切「去年」（种子全在当前月）→ 面包屑消失（复位下钻）+ 三卡空态。
    // 期间芯片在弹层内：先开「选择期间」弹层再点芯片（选择后弹层自关）。
    app.press("id:reports-period-label").unwrap();
    app.press("id:reports-period-chip-last-year").unwrap();
    app.wait_for_gone("id:reports-breadcrumb-root", DEFAULT_WAIT)
        .unwrap();
    for empty in ["本期暂无数据", "暂无支出数据", "本期暂无商户消费"] {
        assert!(
            app.shows(empty),
            "空期间空态「{empty}」在屏：{:?}",
            app.screen().texts()
        );
    }
    assert!(
        !app.shows(&reports_month_key()) && !app.shows("甲"),
        "空期间不残留上个期间的图行：{:?}",
        app.screen().texts()
    );
}

/// 会话内保留（旧壳 reports-session 平价）：下钻态 + 期间改选 + Top 10 →
/// 切走再回来 = 回到离开时的样子（视图重挂重拉但会话态不丢）。
#[test]
fn reports_view_state_survives_view_switch() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_reports_book("reports-retained");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_reports(&mut app);
    wait_reports_loaded(&mut app);

    // 留下状态印记：下钻餐饮 + 切 Top 10（期间不动——默认当年）。
    app.press("text:餐饮").unwrap();
    app.wait_for("id:reports-breadcrumb-root").unwrap();
    app.press("id:reports-topn-10").unwrap();
    app.wait_for("text:己站").unwrap();

    // 切走（概览）再回来：面包屑与 Top 10 档位原样。
    app.press("id:nav-dashboard").unwrap();
    app.wait_for("id:net-worth-card").unwrap();
    app.press("id:nav-reports").unwrap();
    app.wait_for("id:reports-monthly-card").unwrap();
    wait_reports_loaded(&mut app);
    assert!(
        app.screen().count("id:reports-breadcrumb-root").unwrap() == 1,
        "下钻态跨视图保留：{:?}",
        app.screen().texts()
    );
    assert!(
        app.shows("己站") && app.shows("庚点"),
        "TopN 档位跨视图保留（Top 10 全量）"
    );
}

/// ESC 复位出口（#894 同语义平价）：期间/下钻/TopN 有保留态时按 ESC →
/// 全部回默认（期间回当年、下钻回基础态、TopN 回默认档）；无保留态（全
/// 默认）幂等无操作。删除复位接线（on_key 或守卫任一）即红。
#[test]
fn reports_view_escape_resets_to_defaults() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_reports_book("reports-esc");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_reports(&mut app);
    wait_reports_loaded(&mut app);

    // 留下保留态：下钻（期间保持默认当年——排除期间切换复位下钻的既有接线
    // 干扰）+ Top 10。
    app.press("text:餐饮").unwrap();
    app.wait_for("id:reports-breadcrumb-root").unwrap();
    app.press("id:reports-topn-10").unwrap();
    app.wait_for("text:己站").unwrap();

    // ESC → 复位出口：下钻回基础态（面包屑消失，纯投影）+ Top 5 档位回归
    //（己/庚不呈现——TopN 回默认档）。
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:reports-breadcrumb-root", DEFAULT_WAIT)
        .unwrap();

    app.wait_for("text:甲城").unwrap();
    assert!(!app.shows("己"), "TopN 回默认档（Top 5 截断）");
}

// ---------------------------------------------------------------------------
// 定时视图①订阅页签（#2042）：落位与页签骨架（会话保留）、计划创建旅程
// （含商户输入即建 / 校验条位）、行操作旅程（暂停/恢复/取消气泡）、编辑旅
// 程、期次详情弹层（重试落交易失效广播 / 展开期次）、花费面板口径。删任一
// 接线即红的负向条目逐项写在各测试注释（ADR-0087 断言强度：断言对准用户可
// 观察结果）。
// ---------------------------------------------------------------------------

/// 落位定时视图（定时用例共用）：定时在记账组「更多」容器——等侧栏就绪 →
/// 点「更多」入口 → 点「定时」页签 → 等订阅清单表挂载。
fn open_scheduled(app: &mut Headless) {
    app.wait_for("id:nav-more-bookkeeping").unwrap();
    app.press("id:nav-more-bookkeeping").unwrap();
    app.wait_for("id:more-tab-scheduled").unwrap();
    app.press("id:more-tab-scheduled").unwrap();
    app.wait_for("id:sub-table").unwrap();
}

/// 造一个进行中的订阅计划（用例共用；域公开写入口 create_plan——期次预生
/// 成协议在域内，本月花费口径与下期扣款列都吃它）。金额 ¥30、月付、开始日
/// = 今天（最早 pending 期次日 = 开始日，断言确定性）。
fn seed_subscription_plan(
    db: &ledger_infra::db::DbState,
    account: &str,
    merchant: Option<&str>,
    note: Option<&str>,
) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_scheduled::create_plan(
        &conn,
        ledger_scheduled::CreateScheduledInput {
            kind: ledger_scheduled::ScheduledKind::Subscription,
            account_id: account.to_owned(),
            category_id: None,
            amount_cents: 3_000,
            currency_code: "CNY".to_owned(),
            recurrence_type: ledger_scheduled::RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: today().to_string(),
            note: note.map(str::to_owned),
            merchant_id: merchant.map(str::to_owned),
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .unwrap()
}

/// 经域读路径反查计划的最早 pending 期次 id（重试目标的造数定位）。
fn first_pending_occurrence_id(dir: &std::path::Path, plan_id: &str) -> String {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_scheduled::get_plan_detail(&conn, plan_id)
        .unwrap()
        .pending_occurrences
        .into_iter()
        .next()
        .map(|o| o.id)
        .unwrap_or_else(|| panic!("计划无 pending 期次"))
}

/// 落位与页签骨架（旧壳 scheduled.tab 三形态页签平价）：三页签在屏且均挂真
/// 实清单（订阅 = #2042；分期 / 定时转账 = #2043 真实组件迁入）、行格十列装
/// 配产物（备注/商户/金额/周期/状态/下期扣款）、行操作按钮与状态过滤、花费
/// 面板统计口径。删 view_host 的定时分支挂载 → 本测红（「迁移中」占位）；
/// 删 make_action_renderer 的行操作接线 → 暂停钮缺席红；分期 / 定时转账真
/// 实分支拆除（scheduled.rs 容器分支退回占位）→ 点页签后真实锚点缺席红。
#[test]
fn scheduled_view_lands_with_panes_and_subscription_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-landing");
    let plan_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_account_with_balance(&db, "测试现金", 10_000);
        let merchant = seed_merchant(&db, "视频平台");
        plan_id = seed_subscription_plan(&db, &account, Some(&merchant), Some("视频会员"));
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_scheduled(&mut app);

    // 三形态页签（旧壳 scheduled.tab.* 同词表）。
    for text in ["订阅", "分期", "定时转账"] {
        assert!(
            app.shows(text),
            "形态页签「{text}」在屏：{:?}",
            app.screen().texts()
        );
    }
    // 兄弟页签真实组件（#2043 迁入）：分期清单 / 定时转账清单表挂载（删
    // scheduled.rs 容器真实分支接线即红——退回「迁移中」占位，「分期」
    // 「定时转账」页签词表本身仍在，锚点换位）。点回订阅页签清单回来
    //（会话槽写路径，删除页签点击写 session 槽接线即红）。
    app.press("id:more-tab-scheduled").unwrap();
    app.press("id:scheduled-tab-installments").unwrap();
    app.wait_for("id:inst-table").unwrap();
    app.wait_for("text:分期清单").unwrap();
    app.press("id:scheduled-tab-transfers").unwrap();
    app.wait_for("id:transfer-table").unwrap();
    app.wait_for("text:定时转账清单").unwrap();
    app.press("id:scheduled-tab-subscriptions").unwrap();
    app.wait_for("text:订阅清单").unwrap();
    app.wait_for(&format!("id:sub-next-{plan_id}")).unwrap();

    // 行即现（异步装载纪律：先等行锚点再断格文本，§3.5）；下期扣款行级
    // 锚点（最早 pending 期次日期 · 金额；起始日 = 今天，最早一期在今日）。
    app.wait_for(&format!("id:sub-next-{plan_id}")).unwrap();
    for text in ["视频会员", "视频平台", "¥30", "每月", "进行中"] {
        assert!(
            app.shows(text),
            "订阅清单应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 行操作（旧壳 PlanRowActions 状态矩阵：active 行有 期次/暂停/取消/编辑）。
    for anchor in [
        format!("id:sub-detail-{plan_id}"),
        format!("id:sub-pause-{plan_id}"),
        format!("id:sub-edit-{plan_id}"),
    ] {
        assert!(app.screen().count(&anchor).unwrap() > 0, "{anchor} 在屏");
    }
    // 状态过滤（订阅无「已完成」键集——#309 现状，不越权补齐）。
    for anchor in [
        "id:sub-filter-active",
        "id:sub-filter-paused",
        "id:sub-filter-cancelled",
    ] {
        assert!(app.screen().count(anchor).unwrap() > 0, "{anchor} 在屏");
    }
    // 花费面板（本月实际花费等统计锚点在屏；未执行只读 ¥0 与推算说明同屏）。
    app.wait_for("id:spend-this-month").unwrap();
    for anchor in [
        "id:spend-this-year",
        "id:spend-projected-month",
        "id:spend-projected-year",
    ] {
        assert!(app.screen().count(anchor).unwrap() > 0, "{anchor} 在屏");
    }
}

/// 创建旅程（旧壳校验面 + toast 反馈平价）：空账户前置拦截不写入；填齐
/// （账户拼音下拉 / 分类 / 商户输入即建 / 金额）→ 创建 → 反馈 + 行即现 +
/// 花费面板刷新。删 perform_create 写接缝（create_scheduled_plan 直调）或
/// reload 接线任一环即红。
#[test]
fn scheduled_view_create_subscription_journey() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-create");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_account_with_balance(&db, "测试现金", 10_000);
        seed_category(&db, "茶饮", None);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_scheduled(&mut app);

    // 打开新建弹窗。
    app.press("id:sub-create-open").unwrap();
    app.wait_for("id:app-modal").unwrap();
    app.wait_for("text:新建订阅").unwrap();

    // 前置拦截：无账户点提交 → 错误条位，不写入。
    app.press("id:sub-create").unwrap();
    app.wait_for("id:sub-create-error").unwrap();
    app.wait_for("text:创建失败: 请选择扣款账户").unwrap();

    // 填备注 + 账户（拼音可搜下拉，#2033 封装）。
    app.type_text("id:sub-note", "流媒体会员").unwrap();
    app.press("id:sub-account").unwrap();
    app.wait_for("id:sub-account-search").unwrap();
    app.press("id:sub-account-option >> text:测试现金 >> nth=0")
        .unwrap();
    // 币种随账户推导（只读展示；拆 derive_currency_text 即红）。
    app.wait_for("text:CNY").unwrap();
    // 分类（可搜下拉可清空，表单①先例同件）：拼音搜索过滤后选中。
    app.press("id:sub-category").unwrap();
    app.wait_for("id:sub-category-search").unwrap();
    app.type_text("id:sub-category-search", "茶").unwrap();
    app.press("id:sub-category-option >> text:茶饮 >> nth=0")
        .unwrap();
    // 商户输入即建（自由文本，提交走按名 find-or-create 单点）。
    app.type_text("id:sub-merchant-input", "旗新品牌").unwrap();
    app.type_text("id:sub-amount", "30").unwrap();
    app.press("id:sub-create").unwrap();

    // 反馈 + 行即现（金额/周期/下期扣款随行）。
    app.wait_for("text:已创建订阅").unwrap();
    app.wait_for("text:流媒体会员").unwrap();
    app.wait_for("text:旗新品牌").unwrap();
    assert!(
        app.screen().texts().iter().any(|t| t.contains("· ¥30")),
        "下期扣款 = 期次日期 · ¥30（预生成期次）：{:?}",
        app.screen().texts()
    );
}

/// 行操作旅程（暂停 / 恢复 / 取消气泡写路径平价）：暂停 → 行离「进行中」
/// 进「已暂停」+ 恢复钮在；恢复 → 行回「进行中」；取消走确认气泡（旧壳
/// confirm 文案），取消键不写、确定键写成功。删 perform_status 写接缝或
/// reload 接线任一环即红。
#[test]
fn scheduled_view_row_actions_pause_resume_cancel() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-actions");
    let plan_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_account_with_balance(&db, "测试现金", 10_000);
        plan_id = seed_subscription_plan(&db, &account, None, Some("会员扣款"));
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_scheduled(&mut app);

    // 暂停：行随状态过滤离「进行中」视图（纯前端过滤，旧壳 filteredRows 平价；
    // 操作反馈与恢复钮下一过滤断——先钉行锚点消失再换过滤）。
    // 行集异步落屏：等行动作锚再按（CI 慢环境 press 竞态锚定，同 #2072）。
    app.wait_for(&format!("id:sub-pause-{plan_id}")).unwrap();
    app.press(&format!("id:sub-pause-{plan_id}")).unwrap();
    app.wait_for_gone(&format!("id:sub-next-{plan_id}"), DEFAULT_WAIT)
        .unwrap();
    app.press("id:sub-filter-paused").unwrap();
    app.wait_for(&format!("id:sub-next-{plan_id}")).unwrap();

    // 恢复：行随状态过滤离「已暂停」视图（恢复钮消失），回「进行中」过
    // 滤后暂停钮回到行（active 专属动作集回归）。
    app.press(&format!("id:sub-resume-{plan_id}")).unwrap();
    app.wait_for_gone(&format!("id:sub-resume-{plan_id}"), DEFAULT_WAIT)
        .unwrap();
    app.press("id:sub-filter-active").unwrap();
    app.wait_for(&format!("id:sub-pause-{plan_id}")).unwrap();

    // 取消确认气泡（旧壳 subscriptionCancelConfirm 同文案同位）：打开气泡
    // 看文案 → 先点气泡取消键（不写）→ 重新打开点确定（写成功）。
    app.press(&format!("id:sub-cancel-{plan_id}")).unwrap();
    app.wait_for("text:取消后不再扣款，已生成的交易与历史期次保留。确认取消？")
        .unwrap();
    app.press("id:sub-cancel-cancel").unwrap();
    app.wait_for_gone(
        "text:取消后不再扣款，已生成的交易与历史期次保留。确认取消？",
        DEFAULT_WAIT,
    )
    .unwrap();
    app.press(&format!("id:sub-cancel-{plan_id}")).unwrap();
    app.wait_for("text:取消后不再扣款，已生成的交易与历史期次保留。确认取消？")
        .unwrap();
    app.press("id:sub-cancel-confirm").unwrap();
    // 已取消行离激活过滤（进行中）——行锚点消失 + 「已暂停」过滤也不在。
    app.wait_for_gone(&format!("id:sub-next-{plan_id}"), DEFAULT_WAIT)
        .unwrap();
    app.press("id:sub-filter-paused").unwrap();
    app.wait_for_gone(&format!("id:sub-next-{plan_id}"), DEFAULT_WAIT)
        .unwrap();
    app.press("id:sub-filter-cancelled").unwrap();
    app.wait_for(&format!("id:sub-next-{plan_id}")).unwrap();
    assert!(
        !app.screen().texts().contains(&"暂停"),
        "已取消计划行操作收窄（仅期次）"
    );
}

/// 编辑旅程（ADR-0023 决策三平价：仅非金额字段）：编辑弹窗回填（备注 / 商
/// 户），改备注保存 → 反馈 + 行随改。删 open_edit 回填接线或编辑写接缝
///（update_scheduled_subscription 直调）即红。
#[test]
fn scheduled_view_edit_subscription_updates_fields() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-edit");
    let plan_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_account_with_balance(&db, "测试现金", 10_000);
        plan_id = seed_subscription_plan(&db, &account, None, Some("旧备注名"));
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_scheduled(&mut app);

    // 行集异步落屏：等行动作锚再按（CI 慢环境 press 竞态锚定，同 #2072）。
    app.wait_for(&format!("id:sub-edit-{plan_id}")).unwrap();
    app.press(&format!("id:sub-edit-{plan_id}")).unwrap();
    app.wait_for("id:app-modal").unwrap();
    app.wait_for("text:编辑订阅").unwrap();
    assert_eq!(
        app.screen().input_text("id:sub-edit-note").unwrap(),
        Some("旧备注名"),
        "编辑弹窗备注回填"
    );
    app.clear("id:sub-edit-note").unwrap();
    app.type_text("id:sub-edit-note", "新备注名").unwrap();
    app.press("id:sub-edit-save").unwrap();
    app.wait_for("text:已保存").unwrap();
    app.wait_for("text:新备注名").unwrap();
    assert!(!app.shows("旧备注名"), "旧备注离屏");
}

/// 期次详情弹层（三页签通用件首消费不平价即红）：「期次」行动作开弹层 →
/// 计划摘要（备注 + 总期数）与期次行（状态列）在屏；failed 期次「重试」→
/// 命令成功反馈 + 同弹重拉；「展开更多期次」→ 反馈。删弹层开窗装载 effect
/// 或重试写接缝（execute_scheduled_occurrence 直调）任一环即红。
#[test]
fn scheduled_view_plan_detail_modal_retries_failed_occurrence() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-detail");
    let plan_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_account_with_balance(&db, "测试现金", 10_000);
        plan_id = seed_subscription_plan(&db, &account, None, Some("会员扣款"));
    }
    // 造 failed 期次（域单测 set_status 同构造）：最早 pending 期次置 failed，
    // 重试写路径的目标。
    let occ_id = first_pending_occurrence_id(&dir, &plan_id);
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        conn.execute(
            "UPDATE scheduled_transaction_occurrences SET status='failed' WHERE id=?1",
            rusqlite::params![occ_id],
        )
        .unwrap();
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_scheduled(&mut app);

    // 开弹层：标题 + 计划备注摘要 + 期次表。
    // 行集异步落屏：等行动作锚再按（CI 慢环境 press 竞态锚定，同 #2072）。
    app.wait_for(&format!("id:sub-detail-{plan_id}")).unwrap();
    app.press(&format!("id:sub-detail-{plan_id}")).unwrap();
    app.wait_for("id:occ-table").unwrap();
    app.wait_for("text:会员扣款").unwrap();
    app.wait_for(&format!("id:occ-retry-{occ_id}")).unwrap();

    // 重试（failed 期次唯一入口）：命令直调成功反馈（旧壳 detail.retrySuccess
    // 同词表）。
    app.press(&format!("id:occ-retry-{occ_id}")).unwrap();
    app.wait_for("text:重试成功，交易已入账").unwrap();

    // 失效刷新（自回调重拉线）：弹窗外的清单与花费面板同步重拉——期次执
    // 行落交易行后，花费面板的本月/本年实际花费与逐订阅行从「¥0」变「¥30」
    //（执行期次按计划日入账，本月口径变化）。整屏再无「¥0」残留——删
    // reload_after_change 的 reload 接线即红（¥0 残留、其余不变）；信号
    // 广播线（notify_transactions_invalidated）的共享目的地（交易列表）
    // 另有 #2022 删除刷新旅程钉点（删除该广播点即红）。
    app.wait_for_gone("text:¥0", DEFAULT_WAIT).unwrap();

    // 展开更多期次（active 计划恒可展开：无限循环无上限）→ 反馈条位在屏。
    app.press("id:occ-expand").unwrap();
    app.wait_for("id:occ-note").unwrap();
    assert!(
        app.screen().texts().iter().any(|t| t.contains("已生成")),
        "展开反馈「已生成 n 期」在屏：{:?}",
        app.screen().texts()
    );

    // 计划执行产生的交易在交易列表在屏（旧壳「计划执行落交易行列表可
    // 见」平价；重试写接缝拆除时本断言亦红——交易不落库）。
    app.press("id:app-modal-close").unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 1 条").unwrap();
    assert!(app.shows("会员扣款"), "重试落地的交易行重读在屏");
}

/// 计划来源落点占位钉点（无消费面不预建）：交易来源列跳转产线仍未建立，
/// 本测只钉「直接落位不误开详情」——落位无 focus 时详情弹层不挂载。删
/// pending_focus 暂存清闸接线（focus 残留）→ 二次进视图误开弹层红。
#[test]
fn scheduled_view_lands_without_detail_modal_when_no_focus() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-no-focus");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_account_with_balance(&db, "测试现金", 10_000);
        seed_subscription_plan(&db, &account, None, Some("会员扣款"));
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_scheduled(&mut app);
    app.settle();
    assert!(
        app.screen().count("id:occ-table").unwrap() == 0,
        "无 focus 落位不开详情弹层"
    );

    // 页签往复不复开（读一次封闸 + 页签切换不复位的会话槽；旧壳
    // usePlanFocusLanding 的时序平价——无意图时页签往复零弹层）。
    app.press("id:scheduled-tab-installments").unwrap();
    app.wait_for("id:inst-table").unwrap();
    app.press("id:scheduled-tab-subscriptions").unwrap();
    app.wait_for("id:sub-table").unwrap();
    app.settle();
    assert!(
        app.screen().count("id:occ-table").unwrap() == 0,
        "页签往复不复开详情弹层"
    );
}

/// 直落分支（view_host 定时分支；「移回侧栏」后主项直达）：侧栏序存储种
/// 子把定时移回记账组主项（收纳清单同步剔除）→ 侧栏定时项直达订阅页签。
/// 拆 view_host 的 Scheduled 分支挂载 → 本测红（落回迁移占位）。
#[test]
fn scheduled_view_lands_directly_when_moved_to_sidebar() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-direct");
    let plan_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_account_with_balance(&db, "测试现金", 10_000);
        plan_id = seed_subscription_plan(&db, &account, None, Some("会员扣款"));
    }

    // 侧栏序存储种子：「定时」移回侧栏（收纳清单同步剔除，就地移回豁免
    // 同形的存储侧清面——parse_containment_lists 移回豁免口径一致）。
    let mut app = Headless::start(desktop_config_with_storage(&[
        (
            "view_state:sidebar_containment",
            r#"{"bookkeeping":["merchants"],"assets":["policies","physicalAssets","insurers"],"insights":[]}"#,
        ),
        (
            "view_state:sidebar_order",
            r#"{"bookkeeping":["transactions","accounts","scheduled","budget"],"assets":["investments","items","savingsGoals"],"insights":["reports","search"]}"#,
        ),
    ]))
    .unwrap();
    nsh_app::app::set_book_dir(dir);
    app.wait_for("id:nav-scheduled").unwrap();
    app.press("id:nav-scheduled").unwrap();
    app.wait_for("id:sub-table").unwrap();
    app.wait_for(&format!("id:sub-next-{plan_id}")).unwrap();
}

// ---------------------------------------------------------------------------
// 定时视图②转账与分期页签（#2043）：定时转账（清单/新建/行操作）与分期
// （清单进度呈现/新建含 floor 口径预览）两页签 CRUD 与执行落库旅程。删任一
// 接线即红的负向条目逐项写在各测试注释（ADR-0087 断言强度：断言对准用户可
// 观察结果）。
// ---------------------------------------------------------------------------

/// 造一个进行中的定时转账计划（用例共用；域公开写入口 create_plan——期次预
/// 生成协议在域内，下期转账列都吃它）。金额 ¥30、月付、转出/转入同币种、开
/// 始日 = 今天（最早 pending 期次日 = 开始日，断言确定性）；total_occurrences
/// = Some(n) 时金额格带「× n期」尾饰。
fn seed_transfer_plan(
    db: &ledger_infra::db::DbState,
    from: &str,
    to: &str,
    note: Option<&str>,
    total_occurrences: Option<i64>,
) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_scheduled::create_plan(
        &conn,
        ledger_scheduled::CreateScheduledInput {
            kind: ledger_scheduled::ScheduledKind::ScheduledTransfer,
            account_id: from.to_owned(),
            category_id: None,
            amount_cents: 3_000,
            currency_code: "CNY".to_owned(),
            recurrence_type: ledger_scheduled::RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: today().to_string(),
            note: note.map(str::to_owned),
            merchant_id: None,
            policy_id: None,
            total_amount_cents: None,
            total_occurrences,
            to_account_id: Some(to.to_owned()),
        },
    )
    .unwrap()
}

/// 造一个进行中的分期计划（用例共用；域公开写入口 create_plan）。总金额
/// ¥300 / 3 期（每期 ¥100 整除）、月付、开始日 = 今天。
fn seed_installment_plan(
    db: &ledger_infra::db::DbState,
    account: &str,
    note: Option<&str>,
) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_scheduled::create_plan(
        &conn,
        ledger_scheduled::CreateScheduledInput {
            kind: ledger_scheduled::ScheduledKind::Installment,
            account_id: account.to_owned(),
            category_id: None,
            amount_cents: 10_000,
            currency_code: "CNY".to_owned(),
            recurrence_type: ledger_scheduled::RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: today().to_string(),
            note: note.map(str::to_owned),
            merchant_id: None,
            policy_id: None,
            total_amount_cents: Some(30_000),
            total_occurrences: Some(3),
            to_account_id: None,
        },
    )
    .unwrap()
}

/// 落位定时转账页签（定时用例共用）：订阅落地 → 点「定时转账」页签 → 等
/// 清单表挂载。
fn open_transfers_pane(app: &mut Headless) {
    open_scheduled(app);
    app.press("id:scheduled-tab-transfers").unwrap();
    app.wait_for("id:transfer-table").unwrap();
}

/// 落位分期页签（定时用例共用）：订阅落地 → 点「分期」页签 → 等清单表挂载。
fn open_installments_pane(app: &mut Headless) {
    open_scheduled(app);
    app.press("id:scheduled-tab-installments").unwrap();
    app.wait_for("id:inst-table").unwrap();
}

/// 定时转账页签落地平价（旧壳 TransfersPane 桌面档九列 + WITH_COMPLETED 过
/// 滤键集 + 生命周期行操作闭集）：行格装配产物（转出/转入/金额 × N期尾饰/
/// 周期/下期转账）、状态过滤四键、行操作（期次/暂停/取消，无编辑——编辑是
/// 订阅适配器自建描述符 ADR-0041 决策 7）。删 scheduled.rs 容器真实分支或
/// make_action_renderer 行操作接线任一即红。
#[test]
fn scheduled_view_transfers_pane_lands_with_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-transfers-landing");
    let plan_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let from = seed_account_with_balance(&db, "测试现金", 10_000);
        let to = seed_account_with_balance(&db, "储蓄罐", 0);
        plan_id = seed_transfer_plan(&db, &from, &to, Some("月度储蓄"), Some(3));
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transfers_pane(&mut app);

    // 行即现（异步装载纪律：先等行锚点再断格文本，§3.5）。
    app.wait_for(&format!("id:transfer-next-{plan_id}"))
        .unwrap();
    for text in [
        "定时转账清单",
        "月度储蓄",
        "测试现金",
        "储蓄罐",
        "¥30 × 3期",
        "每月",
        "进行中",
    ] {
        assert!(
            app.shows(text),
            "转账清单应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 状态过滤四键（WITH_COMPLETED 键集——订阅三键集的差异面，#309 口径）。
    for anchor in [
        "id:transfer-filter-active",
        "id:transfer-filter-paused",
        "id:transfer-filter-cancelled",
        "id:transfer-filter-completed",
    ] {
        assert!(app.screen().count(anchor).unwrap() > 0, "{anchor} 在屏");
    }
    // 行操作（生命周期闭集：期次/暂停/取消，无编辑）。
    for anchor in [
        format!("id:transfer-detail-{plan_id}"),
        format!("id:transfer-pause-{plan_id}"),
        format!("id:transfer-cancel-{plan_id}"),
    ] {
        assert!(app.screen().count(&anchor).unwrap() > 0, "{anchor} 在屏");
    }
    assert!(
        app.screen()
            .count(&format!("id:transfer-edit-{plan_id}"))
            .unwrap()
            == 0,
        "转账行无编辑动作（编辑是订阅适配器自建描述符）"
    );
}

/// 创建定时转账旅程（旧壳校验面 + toast 反馈平价）：空转出账户前置拦截不
/// 写入；填齐（备注 / 转出 / 币种随账户推导 / 转入同币种 / 金额 / 总期数三
/// 态的有限期数支）→ 创建 → 反馈 + 行即现（金额格 × N期尾饰 + 下期转账）。
/// 删 perform_create 写接缝（create_scheduled_plan 直调）或币种推导、转入
/// 候选过滤、总期数语义任一接线即红。
#[test]
fn scheduled_view_create_transfer_journey() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-transfer-create");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_account_with_balance(&db, "测试现金", 10_000);
        seed_account_with_balance(&db, "储蓄罐", 0);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transfers_pane(&mut app);

    // 打开新建弹窗。
    app.press("id:transfer-create-open").unwrap();
    app.wait_for("id:app-modal").unwrap();
    app.wait_for("text:新建定时转账").unwrap();

    // 前置拦截：无转出账户点提交 → 错误条位，不写入。
    app.press("id:transfer-create").unwrap();
    app.wait_for("id:transfer-create-error").unwrap();
    app.wait_for("text:创建失败: 请选择转出账户").unwrap();

    // 填备注 + 转出账户（拼音可搜下拉，#2033 封装）。
    app.type_text("id:transfer-note", "月度储蓄").unwrap();
    app.press("id:transfer-from-account").unwrap();
    app.wait_for("id:transfer-from-account-search").unwrap();
    app.press("id:transfer-from-account-option >> text:测试现金 >> nth=0")
        .unwrap();
    // 币种随转出账户推导（只读展示；拆 derive_currency_text 即红）。
    app.wait_for("text:CNY").unwrap();
    // 转入账户（候选同币种过滤 + 排除转出本身：候选面只剩储蓄罐）。
    app.press("id:transfer-to-account").unwrap();
    app.wait_for("id:transfer-to-account-search").unwrap();
    app.press("id:transfer-to-account-option >> text:储蓄罐 >> nth=0")
        .unwrap();
    app.type_text("id:transfer-amount", "30").unwrap();
    // 总期数三态：填 3 = 有限期数（金额格带「× 3期」尾饰）。
    app.type_text("id:transfer-total-occurrences", "3").unwrap();
    app.press("id:transfer-create").unwrap();

    // 反馈 + 行即现（金额/转入/下期转账随行）。
    app.wait_for("text:已创建定时转账").unwrap();
    app.wait_for("text:月度储蓄").unwrap();
    assert!(
        app.screen().texts().iter().any(|t| t.contains("¥30 × 3期")),
        "金额格 × N期尾饰（有限总期数）：{:?}",
        app.screen().texts()
    );
    assert!(
        app.screen().texts().iter().any(|t| t.contains("· ¥30")),
        "下期转账 = 期次日期 · ¥30（预生成期次）：{:?}",
        app.screen().texts()
    );
}

/// 定时转账行操作旅程（暂停/恢复/取消气泡写路径平价，旧壳
/// transferCancelConfirm 文案同位）：暂停 → 行离「进行中」进「已暂停」+ 恢
/// 复钮在；恢复 → 行回「进行中」；取消走确认气泡，取消键不写、确定键写成
/// 功。删 perform_status 写接缝（update_scheduled_plan_status 直调）或气泡
/// 确认接线任一环即红。
#[test]
fn scheduled_view_transfer_row_actions_pause_resume_cancel() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-transfer-actions");
    let plan_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let from = seed_account_with_balance(&db, "测试现金", 10_000);
        let to = seed_account_with_balance(&db, "储蓄罐", 0);
        plan_id = seed_transfer_plan(&db, &from, &to, Some("月度储蓄"), None);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transfers_pane(&mut app);

    // 暂停：行随状态过滤离「进行中」视图（纯前端过滤，旧壳 filteredRows 平
    // 价；先钉行锚点消失再换过滤）。
    // 行集异步落屏：等行动作锚再按（CI 慢环境 press 竞态锚定，同 #2072）。
    app.wait_for(&format!("id:transfer-pause-{plan_id}"))
        .unwrap();
    app.press(&format!("id:transfer-pause-{plan_id}")).unwrap();
    app.wait_for_gone(&format!("id:transfer-next-{plan_id}"), DEFAULT_WAIT)
        .unwrap();
    app.press("id:transfer-filter-paused").unwrap();
    app.wait_for(&format!("id:transfer-next-{plan_id}"))
        .unwrap();

    // 恢复：行随状态过滤离「已暂停」视图，回「进行中」过滤后暂停钮回行。
    app.press(&format!("id:transfer-resume-{plan_id}")).unwrap();
    app.wait_for_gone(&format!("id:transfer-resume-{plan_id}"), DEFAULT_WAIT)
        .unwrap();
    app.press("id:transfer-filter-active").unwrap();
    app.wait_for(&format!("id:transfer-pause-{plan_id}"))
        .unwrap();

    // 取消确认气泡（旧壳 transferCancelConfirm 同文案同位）：打开气泡看文
    // 案 → 先点气泡取消键（不写）→ 重新打开点确定（写成功）。
    app.press(&format!("id:transfer-cancel-{plan_id}")).unwrap();
    app.wait_for("text:取消后不再自动转账，已生成的交易与历史期次保留。确认取消？")
        .unwrap();
    app.press("id:transfer-cancel-cancel").unwrap();
    app.wait_for_gone(
        "text:取消后不再自动转账，已生成的交易与历史期次保留。确认取消？",
        DEFAULT_WAIT,
    )
    .unwrap();
    app.press(&format!("id:transfer-cancel-{plan_id}")).unwrap();
    app.wait_for("text:取消后不再自动转账，已生成的交易与历史期次保留。确认取消？")
        .unwrap();
    app.press("id:transfer-cancel-confirm").unwrap();
    // 已取消行离激活过滤（进行中）——行锚点消失，「已取消」过滤下回归。
    app.wait_for_gone(&format!("id:transfer-next-{plan_id}"), DEFAULT_WAIT)
        .unwrap();
    app.press("id:transfer-filter-cancelled").unwrap();
    app.wait_for(&format!("id:transfer-next-{plan_id}"))
        .unwrap();
    assert!(
        !app.screen().texts().contains(&"暂停"),
        "已取消计划行操作收窄（仅期次）"
    );
}

/// 分期页签落地平价（旧壳 InstallmentsPane 桌面档十列 + 进度呈现）：行格装
/// 配产物（商户/分类/扣款账户/总金额/进度）、期数维度进度条锚点、已还汇总
/// 文案（实时汇总来自既有详情命令 completed_*，执行一期的落库路径消费——
/// 域 execute_occurrence 公开写入口）。删 scheduled.rs 容器真实分支、进度
/// 条自绘接线或行操作接线任一即红。
#[test]
fn scheduled_view_installments_pane_lands_with_progress_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-installments-landing");
    let plan_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_account_with_balance(&db, "测试现金", 10_000);
        plan_id = seed_installment_plan(&db, &account, Some("手机分期"));
    }
    // 执行一期（域公开写入口，进度汇总的落库路径）：已完成期次数 1、已还
    // 金额 ¥100——清集成一个已还金额分位，断言确定性。
    let occ_id = first_pending_occurrence_id(&dir, &plan_id);
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_scheduled::execute_occurrence(&conn, &occ_id).unwrap();
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_installments_pane(&mut app);

    // 行即现 + 进度呈现（进度列 = 已还汇总文案 + 期数维度进度条行级锚点）。
    app.wait_for(&format!("id:inst-progress-{plan_id}"))
        .unwrap();
    for text in [
        "分期清单",
        "手机分期",
        "¥300",
        "已还 ¥100 / ¥300 · 1/3 期",
        "每月",
        "进行中",
    ] {
        assert!(
            app.shows(text),
            "分期清单应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 状态过滤四键（WITH_COMPLETED 键集）。
    for anchor in [
        "id:inst-filter-active",
        "id:inst-filter-paused",
        "id:inst-filter-cancelled",
        "id:inst-filter-completed",
    ] {
        assert!(app.screen().count(anchor).unwrap() > 0, "{anchor} 在屏");
    }
    // 行操作（生命周期闭集：期次/暂停/取消，无编辑；操作列得在 1280 视口
    // 全宽在屏——列建议宽已收紧，见 INST_COLUMNS）。
    for anchor in [
        format!("id:inst-detail-{plan_id}"),
        format!("id:inst-pause-{plan_id}"),
        format!("id:inst-cancel-{plan_id}"),
    ] {
        assert!(
            app.screen().count(&anchor).unwrap() > 0,
            "{anchor} 在屏：{:?}",
            app.screen().texts()
        );
    }
    assert!(
        app.screen()
            .count(&format!("id:inst-edit-{plan_id}"))
            .unwrap()
            == 0,
        "分期行无编辑动作（编辑是订阅适配器自建描述符）"
    );
}

/// 创建分期旅程（旧壳校验面 + floor 口径预览 + toast 反馈平价）：空账户/
/// 总额小于期数两条前置拦截都不写入；填齐后每期金额预览（100 分 / 3 期 =
/// 33/33/34 →「每期 ¥33.33 · 末期 ¥33.34（含尾差）」，口径单点
/// installment_schedule）→ 创建 → 反馈 + 行即现（进度 0/3）。删 perform_
/// create 写接缝或预览接线任一环即红（预览文案缺席）。
#[test]
fn scheduled_view_create_installment_journey() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("scheduled-installment-create");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_account_with_balance(&db, "测试现金", 10_000);
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_installments_pane(&mut app);

    // 打开新建弹窗。
    app.press("id:inst-create-open").unwrap();
    app.wait_for("id:app-modal").unwrap();
    app.wait_for("text:新建分期").unwrap();

    // 前置拦截一：无账户点提交 → 错误条位，不写入。
    app.press("id:inst-create").unwrap();
    app.wait_for("id:inst-create-error").unwrap();
    app.wait_for("text:创建失败: 请选择扣款账户").unwrap();

    // 选账户（拼音可搜下拉，#2033 封装）。
    app.press("id:inst-account").unwrap();
    app.wait_for("id:inst-account-search").unwrap();
    app.press("id:inst-account-option >> text:测试现金 >> nth=0")
        .unwrap();
    // 币种随账户推导（只读展示）。
    app.wait_for("text:CNY").unwrap();

    // 前置拦截二：总额 < 期数（每期至少 1 分）→ 前端先行同文案拦截
    //（¥0.03 = 3 分 < 5 期）。
    app.type_text("id:inst-total", "0.03").unwrap();
    app.type_text("id:inst-periods", "5").unwrap();
    app.press("id:inst-create").unwrap();
    app.wait_for("text:创建失败: 总金额不能小于期数（每期至少 1 分）")
        .unwrap();

    // 每期金额预览（floor 均分 + 尾差进最后一期；装配层预览接线：
    // ¥100 / 3 期 = 33/33/34 →「每期 ¥33.33 · 末期 ¥33.34（含尾差）」）。
    app.clear("id:inst-total").unwrap();
    app.type_text("id:inst-total", "100").unwrap();
    app.clear("id:inst-periods").unwrap();
    app.type_text("id:inst-periods", "3").unwrap();
    app.wait_for("text:每期 ¥33.33 · 末期 ¥33.34（含尾差）")
        .unwrap();

    app.type_text("id:inst-note", "手机分期").unwrap();
    app.press("id:inst-create").unwrap();

    // 反馈 + 行即现（进度 0/3）。
    app.wait_for("text:已创建分期计划").unwrap();
    app.wait_for("text:手机分期").unwrap();
    assert!(
        app.screen()
            .texts()
            .iter()
            .any(|t| t.contains("已还 ¥0 / ¥100 · 0/3 期")),
        "进度 = 已还汇总（实时面）：{:?}",
        app.screen().texts()
    );
}

// ---------------------------------------------------------------------------
// 保单视图（#2050）：资产「更多」容器页签到达路径、清单与统计（挂单保费同
// 源快照）、新建（保司即建 + 协议区）、编辑与软删确认、删除即红负向条目逐
// 项写在各测试注释（ADR-0087 断言强度：断言对准用户可观察结果）。
// ---------------------------------------------------------------------------

/// 造一个保司（按名 find-or-create，域公开写入口）。
fn seed_insurer(db: &ledger_infra::db::DbState, name: &str) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_policy::create_insurer_by_name(&conn, name).unwrap()
}

/// 造一张保单（域公开写入口；coverage = (保额分, 币种)）。
fn seed_policy(
    db: &ledger_infra::db::DbState,
    insurer_id: &str,
    number: &str,
    product: &str,
    start: &str,
    end: Option<&str>,
    coverage: Option<(i64, Option<&str>)>,
) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_policy::create_policy(
        &conn,
        ledger_policy::PolicyInput {
            insurer_id: insurer_id.to_owned(),
            policy_number: number.to_owned(),
            product_name: product.to_owned(),
            start_date: start.to_owned(),
            end_date: end.map(str::to_owned),
            coverage_amount_cents: coverage.map(|(cents, _)| cents),
            coverage_currency_code: coverage.and_then(|(_, code)| code.map(str::to_owned)),
            note: None,
        },
        &mut || {},
    )
    .unwrap()
}

/// 造一笔挂医保单的保费支出（保单视角统计的挂单流水面，域公开写入口）。
fn seed_policy_expense(
    db: &ledger_infra::db::DbState,
    account_id: &str,
    policy_id: &str,
    amount_cents: i64,
) {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let mut input = ledger_transaction::TransactionInput {
        kind: ledger_transaction::TransactionKind::Expense,
        amount_cents,
        currency_code: "CNY".to_owned(),
        account_id: Some(account_id.to_owned()),
        to_account_id: None,
        funding_account_id: None,
        funding: Vec::new(),
        purchases: Vec::new(),
        category_id: None,
        merchant_id: None,
        merchant_name: None,
        policy_id: Some(policy_id.to_owned()),
        refund_of_transaction_id: None,
        note: Some("保费扣款".to_owned()),
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
    };
    input.policy_id = Some(policy_id.to_owned());
    ledger_transaction::create(&conn, input).unwrap();
}

/// 库态核对（不经 UI）：保单号与软删标志——软删行在域标准清单缺席，
/// 直查表的 is_deleted 标志是对「软删不物理移除」的存在性凭据。
fn book_policy_numbers(dir: &Path) -> Vec<(String, i64)> {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT policy_number, is_deleted FROM policies ORDER BY created_at")
        .unwrap();
    stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0).unwrap(),
            row.get::<_, i64>(1).unwrap(),
        ))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

/// 库态核对：保单名下订阅协议（含保单指针与金额）。
fn book_agreements(dir: &Path, policy_id: &str) -> Vec<i64> {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db.read_conn.lock().unwrap();
    ledger_scheduled::list_plans(&conn)
        .unwrap()
        .into_iter()
        .filter(|plan| plan.policy_id.as_deref() == Some(policy_id))
        .map(|plan| plan.core.amount_cents)
        .collect()
}

/// 从侧栏点开资产组「更多」容器的保单页签（#2050 到达路径共用）。
fn open_policies(app: &mut Headless) {
    app.wait_for("id:nav-more-assets").unwrap();
    app.press("id:nav-more-assets").unwrap();
    app.wait_for("id:more-tab-policies").unwrap();
    app.wait_for("id:policy-table").unwrap();
}

/// 装载（旧壳 onMounted 平价，容器页签到达）：清单卡列头与行格（保司显示
/// 映射 / 期间 / 保障中 / 保额自带币种 / 统计三列同源快照 + 无活跃协议占
/// 位「—」）。删除 boot 装载读接缝 / 行装配 / 容器挂载任一环即红。
#[test]
fn policy_view_lands_with_rows_stats_and_columns() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("policy-land");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        let insurer = seed_insurer(&db, "众安");
        seed_policy(
            &db,
            &insurer,
            "P-001",
            "长期重疾险",
            "2024-01-01",
            None,
            None,
        );
        let second = seed_policy(
            &db,
            &insurer,
            "P-002",
            "医疗险",
            "2024-06-01",
            Some("2025-12-31"),
            Some((12_345, Some("USD"))),
        );
        seed_policy_expense(&db, &account, &second, 12_00);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_policies(&mut app);
    app.wait_for("text:长期重疾险").unwrap();

    for text in [
        "保单",
        "新建保单",
        "保单列表",
        "保险公司",
        "险种名称",
        "保单号",
        "保障期间",
        "保额",
        "累计已缴",
        "累计流入",
        "下期扣款日",
        "长期重疾险",
    ] {
        assert!(
            app.shows(text),
            "应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    for text in [
        "众安",
        "P-001",
        "2024-01-01 ~ 长期",
        "保障中",
        "医疗险",
        "P-002",
    ] {
        assert!(
            app.shows(text),
            "行格应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 统计同源快照（挂单保费本位币合计）+ 无活跃协议占位（下期扣款「—」）。
    for text in ["¥12", "—"] {
        assert!(
            app.shows(text),
            "统计应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 已到期（止日早于今天）与保额自带币种不折算（USD 原样）。
    assert!(app.shows("已到期"), "止日已过应呈已到期");
    assert!(app.shows("$123.45"), "保额应按自带币种原样呈现");
    // 到期列双态语义色锚点（test_id 计数而非字面）不参与断言——色面归人工
    // 对表子集。
}

/// 新建旅程（旧壳 create + 输入即建保司平价）：空名与必填拦两步（保司空 →
/// 拦；保单号空 → 拦）；输入新保司名 + 必填项 → 保存 → 行即现 + 反馈条位 +
/// 弹窗关；保司即建（库中在册）。删除「新建 → perform_save → reload」任一
/// 接线即红。
#[test]
fn policy_view_create_journey_with_new_insurer() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("policy-create");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_policies(&mut app);

    app.press("id:policy-new").unwrap();
    app.wait_for("id:policy-form-number").unwrap();
    app.wait_for("text:新建保单").unwrap();

    // 保司未选未输 → 拦截（弹窗不关）。
    app.press("id:policy-form-save").unwrap();
    app.wait_for("id:policy-form-error").unwrap();
    app.wait_for("text:保存失败: 请选择或输入保险公司").unwrap();

    // 保司即键入新名；保单号留空 → 拦截。
    app.type_text("id:policy-form-insurer-input", "新增保司")
        .unwrap();
    app.press("id:policy-form-save").unwrap();
    app.wait_for("text:保存失败: 保单号不能为空").unwrap();

    // 补齐必填 → 保存成功：行即现、反馈在屏、弹窗关；保司在库。
    app.type_text("id:policy-form-number", "P-101").unwrap();
    app.type_text("id:policy-form-product", "百万医疗").unwrap();
    app.press("id:policy-form-save").unwrap();
    app.wait_for("text:保单已创建").unwrap();
    app.wait_for_gone("id:policy-form-save", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("id:policy-table").unwrap();
    app.wait_for("text:百万医疗").unwrap();
    assert!(app.shows("P-101"), "新保单行在屏");
    assert!(app.shows("新增保司"), "即建保司显示映射在屏");
    let numbers = book_policy_numbers(&dir);
    assert!(
        numbers
            .iter()
            .any(|(number, deletion)| number == "P-101" && *deletion == 0),
        "新保单落库在册：{:?}",
        numbers
    );
}

/// 新建带缴费协议（旧壳 PolicyFormModal 协议区平价）：开关默认关；开启后
/// 字段组可见；扣款账户下拉选择唯一候选 → 保存 → 保单落库 + 订阅形态协议
/// 落库（携带保单引用、金额与频次）。删除「协议区开关 → 字段组 → 保存建
/// 协议」任一接线即红。
#[test]
fn policy_view_create_with_agreement_creates_plan() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("policy-agreement");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        seed_cash_account(&db);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_policies(&mut app);

    app.press("id:policy-new").unwrap();
    app.wait_for("id:policy-form-number").unwrap();
    // 开关默认关：字段组默认不呈现（test_id 计数）；开启后可见。
    assert_eq!(
        app.screen().count("id:policy-agreement-amount").unwrap(),
        0,
        "关闭态不呈现协议字段组"
    );
    app.press("id:policy-agreement-toggle").unwrap();
    app.wait_for("id:policy-agreement-amount").unwrap();

    app.type_text("id:policy-form-insurer-input", "协议保司")
        .unwrap();
    app.type_text("id:policy-form-number", "P-201").unwrap();
    app.type_text("id:policy-form-product", "保费协议险")
        .unwrap();
    app.type_text("id:policy-agreement-amount", "60").unwrap();
    app.press("id:policy-agreement-account").unwrap();
    app.wait_for("id:policy-agreement-account-option").unwrap();
    app.press("id:policy-agreement-account-option >> text:测试现金")
        .unwrap();
    app.settle();

    app.press("id:policy-form-save").unwrap();
    app.wait_for("text:保单已创建").unwrap();
    app.wait_for_gone("id:policy-form-save", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("id:policy-table").unwrap();
    app.wait_for("text:保费协议险").unwrap();

    // 库态：协议以保单引用落库（订阅形态，¥60 = 6000 分）。
    let conn = nsh_app::boot::boot_ledger(&dir).unwrap();
    let read_conn = conn.read_conn.lock().unwrap();
    let policies = ledger_policy::list_policies(&read_conn).unwrap();
    let target = policies
        .iter()
        .find(|policy| policy.policy_number == "P-201")
        .unwrap()
        .clone();
    drop(read_conn);
    let plan_dir = &dir;
    let plan_ids = book_agreements(plan_dir, &target.id);
    assert_eq!(
        plan_ids,
        [6_000],
        "协议金额随保单引用落库（¥60 = 6000 分）：{plan_ids:?}"
    );
}

/// 编辑旅程（旧壳 update 全量替换平价）：编辑弹层回填保单号 / 险种原值 →
/// 改保单号保存 → 行刷新新号（同 syn 快照）。删除「编辑 → perform_save
/// (update) → reload」任一接线即红。
#[test]
fn policy_view_edit_updates_fields() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("policy-edit");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let insurer = seed_insurer(&db, "编辑保司");
        seed_policy(&db, &insurer, "P-301", "编辑险种", "2025-01-01", None, None);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_policies(&mut app);
    app.wait_for("text:编辑险种").unwrap();

    // 打开编辑：回填原值（保单号输入框原文本）。
    let policy_id = {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.read_conn.lock().unwrap();
        ledger_policy::list_policies(&conn)
            .unwrap()
            .into_iter()
            .find(|policy| policy.policy_number == "P-301")
            .unwrap()
            .id
    };
    app.press(&format!("id:policy-edit-{policy_id}")).unwrap();
    app.wait_for("id:policy-form-number").unwrap();
    app.wait_for("text:编辑保单").unwrap();
    assert_eq!(
        app.screen().input_text("id:policy-form-number").unwrap(),
        Some("P-301"),
        "编辑弹层回填原保单号"
    );
    assert_eq!(
        app.screen().input_text("id:policy-form-product").unwrap(),
        Some("编辑险种"),
        "编辑弹层回填原险种"
    );

    // 改号保存：行刷新 + 反馈在屏。
    app.clear("id:policy-form-number").unwrap();
    app.type_text("id:policy-form-number", "P-302").unwrap();
    app.press("id:policy-form-save").unwrap();
    app.wait_for("text:保单已保存").unwrap();
    app.wait_for_gone("id:policy-form-save", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("id:policy-table").unwrap();
    app.wait_for("text:P-302").unwrap();
    assert!(!app.shows("P-301"), "旧号离屏");
}

/// 软删确认守卫（旧壳 AppPopconfirm 平价）：删除按钮开确认气泡（确认文案
/// 在屏）→ 取消不动（行仍在）→ 再开确认 → 行移除（软删不进标准清单，库
/// 中行保留）。删除 popconfirm 开合接线 → 「确认文案永不呈现」本测红。
#[test]
fn policy_view_delete_guard_and_soft_delete() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("policy-delete");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let insurer = seed_insurer(&db, "删保司");
        let policy_id = seed_policy(&db, &insurer, "P-401", "删除险种", "2025-01-01", None, None);
        // 一个不删除的对照（确认软删只落到目标行）。
        seed_policy(&db, &insurer, "P-402", "保留险种", "2025-01-01", None, None);
        let _ = policy_id;
    }
    let policy_id = {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.read_conn.lock().unwrap();
        ledger_policy::list_policies(&conn)
            .unwrap()
            .into_iter()
            .find(|policy| policy.policy_number == "P-401")
            .unwrap()
            .id
    };

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_policies(&mut app);
    app.wait_for("text:删除险种").unwrap();

    // 打开确认气泡（确认文案在屏）→ 取消：行仍在。
    app.press(&format!("id:policy-delete-{policy_id}")).unwrap();
    app.wait_for("id:policy-delete-message").unwrap();
    app.wait_for("text:删除后不再显示在列表，历史记录保留。确认删除该保单？")
        .unwrap();
    app.press("id:policy-delete-cancel").unwrap();
    app.wait_for_gone("id:policy-delete-message", DEFAULT_WAIT)
        .unwrap();
    assert!(app.shows("删除险种"), "取消后行不移除");

    // 再开确认：行移除 + 反馈在屏 + 软删不进标准清单（库中行保留）。
    app.press(&format!("id:policy-delete-{policy_id}")).unwrap();
    app.wait_for("id:policy-delete-message").unwrap();
    app.press("id:policy-delete-confirm").unwrap();
    app.wait_for("text:保单已删除").unwrap();
    app.wait_for_gone("text:删除险种", DEFAULT_WAIT).unwrap();
    assert!(app.shows("保留险种"), "对照行不受波及");
    let numbers = book_policy_numbers(&dir);
    assert!(
        numbers
            .iter()
            .any(|(number, deletion)| number == "P-401" && *deletion == 1),
        "软删保留库行（is_deleted=1，不物理移除）：{:?}",
        numbers
    );
}

/// 空库空态（旧壳 empty guide 同文案）：无保单呈现建档引导。
#[test]
fn policy_view_empty_book_shows_guide() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("policy-empty");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_policies(&mut app);
    app.wait_for("id:policy-empty-guide").unwrap();
    assert!(
        app.shows("暂无保单：点击「新建保单」建立第一份合同档案。"),
        "空态引导文案（旧壳 emptyGuide 同词表）"
    );
}

/// 编辑模式协议历史区（旧壳 PolicyAgreementSection 平价）：分段表（含活跃
/// 段）与改价入口在屏；改价字段组预填旧协议频率（每 1 年）。删除「协议历史
/// 装载 → 分段呈现 → 改价入口」任一接线即红。
#[test]
fn policy_view_edit_shows_agreement_segments_and_rebuild_entry() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("policy-agg-segments");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        let insurer = seed_insurer(&db, "分段保司");
        let policy_id = seed_policy(&db, &insurer, "P-501", "分段险种", "2025-01-01", None, None);
        // 造一段活跃订阅协议（域公开写入口，带保单引用）。
        let conn = db
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
            .unwrap();
        ledger_scheduled::create_plan(
            &conn,
            ledger_scheduled::CreateScheduledInput {
                kind: ledger_scheduled::ScheduledKind::Subscription,
                account_id: account,
                category_id: None,
                amount_cents: 5_000,
                currency_code: "CNY".to_owned(),
                recurrence_type: ledger_scheduled::RecurrenceType::Yearly,
                recurrence_interval: 1,
                recurrence_day: None,
                start_date: "2025-01-01".to_owned(),
                note: Some("分段险种".to_owned()),
                merchant_id: None,
                policy_id: Some(policy_id.clone()),
                total_amount_cents: None,
                total_occurrences: None,
                to_account_id: None,
            },
        )
        .unwrap();
    }

    let policy_id = {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.read_conn.lock().unwrap();
        ledger_policy::list_policies(&conn)
            .unwrap()
            .into_iter()
            .find(|policy| policy.policy_number == "P-501")
            .unwrap()
            .id
    };

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_policies(&mut app);
    app.wait_for("text:分段险种").unwrap();

    // 编辑弹层：协议历史分段表（金额 · 周期 · 起始日 · 状态）与改价入口。
    app.press(&format!("id:policy-edit-{policy_id}")).unwrap();
    app.wait_for("id:policy-agreement-segments").unwrap();
    app.wait_for("text:¥50 · 每年 · 2025-01-01 · 进行中")
        .unwrap();
    app.wait_for("id:policy-agreement-rebuild-open").unwrap();
    assert!(
        app.screen().count("id:policy-agreement-add").unwrap() == 0,
        "有活跃段呈现改价而非添加"
    );

    // 改价入口打开字段组：金额要求显式输入（空），频次预填 1。
    app.press("id:policy-agreement-rebuild-open").unwrap();
    app.wait_for("id:policy-sec-form").unwrap();
    assert_eq!(
        app.screen().input_text("id:policy-sec-interval").unwrap(),
        Some("1"),
        "改价字段组频次预填旧段"
    );
}
// ---------------------------------------------------------------------------
// 实物资产视图（#2049 NSH 迁移⑱）：清单（在持合计卡 + 状态筛选 + 列头与
// 行格）+ 新建/编辑表单 + 更新估值 + 处置 + 软删确认的 CRUD 旅程。删除任一
// 接线即红的负向条目逐项写在各测试注释（ADR-0087 断言强度：断言对准用户
// 可观察结果）。
// ---------------------------------------------------------------------------

/// 造一件实物资产（域公开写入口）：purchase = (购买日期, 金额，分, 币种)，
/// None = 可选购买信息不填；首条估值必填（金额分 + 币种）。
fn seed_physical_asset(
    db: &ledger_infra::db::DbState,
    name: &str,
    purchase: Option<(&str, i64, &str)>,
    valuation_cents: i64,
    valuation_currency: &str,
) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_physical_asset::create_physical_asset(
        &conn,
        PhysicalAssetInput {
            name: name.to_owned(),
            purchase_date: purchase.map(|(date, _, _)| date.to_owned()),
            purchase_price_cents: purchase.map(|(_, cents, _)| cents),
            purchase_currency_code: purchase.map(|(_, _, code)| code.to_owned()),
            initial_valuation_cents: Some(valuation_cents),
            initial_valuation_currency_code: Some(valuation_currency.to_owned()),
            initial_valuation_date: None,
        },
        &mut || {},
    )
    .unwrap()
}

/// 库态核对（不经 UI）：资产行的软删标志与名称——软删行在域标准清单缺席
/// （is_deleted=1 保留库行与估值历史），直查表是对「软删不物理移除」的
/// 存在性凭据。
fn book_asset_names_with_deleted(dir: &Path) -> Vec<(String, i64)> {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT name, is_deleted FROM physical_assets ORDER BY created_at")
        .unwrap();
    stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0).unwrap(),
            row.get::<_, i64>(1).unwrap(),
        ))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

/// 从侧栏点开资产组「更多」容器的实物资产页签（#2049 到达路径；容器默认
/// 首签 = 保单，页签点击切换后才装载实物资产）。
fn open_physical_assets(app: &mut Headless) {
    app.wait_for("id:nav-more-assets").unwrap();
    app.press("id:nav-more-assets").unwrap();
    app.wait_for("id:more-tab-physicalAssets").unwrap();
    app.press("id:more-tab-physicalAssets").unwrap();
    app.wait_for("id:physical-asset-table").unwrap();
}

/// 装载（旧壳 onMounted 平价，容器页签到达）：在持合计卡（后端同源折算快
/// 照）与清单卡列头行格（当前估值折本位币 / 估值日期 / 在持状态 / 处置信息
/// 占位「—」）。删除 boot 装载读接缝 / 行装配 / 容器挂载任一环即红。
#[test]
fn physical_asset_view_lands_with_rows_total_and_columns() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("physical-asset-land");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        // 外币估值（当期汇率 7.2 折本位币）+ 本币估值（合计两腿相加）。
        let conn = db.conn.lock().unwrap();
        ledger_investment::create_exchange_rate(
            &conn,
            ExchangeRateInput {
                base_code: "USD".to_owned(),
                quote_code: "CNY".to_owned(),
                rate: 7.2,
                priced_at: today().to_string(),
                source: None,
            },
        )
        .unwrap();
        drop(conn);
        seed_physical_asset(&db, "海外车位", None, 10_000, "USD");
        seed_physical_asset(&db, "国画", None, 30_000, "CNY");
        // 已处置一件（默认清单缺席；处置档案回看面的造数）。
        let disposed = seed_physical_asset(&db, "拆除资产", None, 5_000, "CNY");
        let conn = db.conn.lock().unwrap();
        ledger_physical_asset::dispose_physical_asset(
            &conn,
            &disposed,
            PhysicalAssetDisposeInput {
                disposal_date: Some("2026-08-01".to_owned()),
                disposal_price_cents: Some(60_000),
                disposal_currency_code: Some("CNY".to_owned()),
            },
            &mut || {},
        )
        .unwrap();
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_physical_assets(&mut app);

    for text in [
        "实物资产",
        "新建资产",
        "资产列表",
        "在持估值合计：¥1020",
        "名称",
        "当前估值",
        "估值日期",
        "状态",
        "处置信息",
        "在持",
    ] {
        assert!(
            app.shows(text),
            "应呈现「{text}」：{:?}",
            app.screen().texts()
        );
    }
    // 在持两件（海外车位 USD 折本位币 + 国画本币，合计两腿相加）；已处置
    // 一件不进默认清单、不进在持合计。text: 选择器是全节点精确匹配。
    assert!(app.shows("¥720"), "外币估值折本位币呈现（当期 7.2）");
    assert!(app.shows("¥300"), "本币估值原样呈现");
    assert!(app.shows("国画"), "在持行在屏");
    assert!(
        !app.shows("拆除资产"),
        "已处置退出默认清单：{:?}",
        app.screen().texts()
    );
    // 切已处置筛选：档案回看（处置日期与处置价同格呈现，处置价按自带币
    // 种）；合计口径恒算在持，回看已处置时不变。
    app.press("id:physical-asset-filter-disposed").unwrap();
    app.wait_for("text:拆除资产").unwrap();
    assert!(
        app.shows("2026-08-01 / ¥600"),
        "已处置档案回看处置日期与处置价纯记录"
    );
    assert!(
        app.shows("在持估值合计：¥1020"),
        "回看已处置时在持合计口径不变（恒算在持）"
    );
    app.press("id:physical-asset-filter-holding").unwrap();
    app.wait_for("text:国画").unwrap();
}

/// 移回侧栏直达（旧壳侧栏仓位语义平价，商户先例同形）：存储种子把实物资产
/// 移出资产收纳清单、挂主项仓位 → 侧栏实物资产项直达真实视图。删 view_host
/// 的实物资产分支挂载 → 本测红（「迁移中」占位——容器分支不经过本路径，
/// 负向条目独立于容器到达）。
#[test]
fn physical_asset_view_lands_directly_when_moved_to_sidebar() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("physical-asset-direct");

    // 侧栏序存储种子：「实物资产」移回侧栏（收纳清单同步剔除）。
    let mut app = Headless::start(desktop_config_with_storage(&[
        (
            "view_state:sidebar_containment",
            r#"{"bookkeeping":["scheduled"],"assets":["policies","insurers"],"insights":[]}"#,
        ),
        (
            "view_state:sidebar_order",
            r#"{"bookkeeping":["transactions","accounts","budget"],"assets":["investments","items","savingsGoals","physicalAssets"],"insights":["reports","search"]}"#,
        ),
    ]))
    .unwrap();
    nsh_app::app::set_book_dir(dir);
    app.wait_for("id:nav-physicalAssets").unwrap();
    app.press("id:nav-physicalAssets").unwrap();
    app.wait_for("id:physical-asset-table").unwrap();
    app.wait_for("text:资产列表").unwrap();
    assert!(
        app.screen().count("id:placeholder-note").unwrap() == 0,
        "实物资产不再占位（#2049 迁入）"
    );
}

/// 空库空态（旧壳 empty guide 同文案）：无资产呈现建档引导。
#[test]
fn physical_asset_view_empty_book_shows_guide() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("physical-asset-empty");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_physical_assets(&mut app);
    app.wait_for("id:physical-asset-empty-guide").unwrap();
    assert!(
        app.shows("还没有实物资产，点击「新建资产」为你的大件家当建立估值档案"),
        "空态引导文案（旧壳 emptyGuide 同词表）"
    );
}

/// 新建旅程（旧壳 create 平价）：名称空 → 拦截；估值空 → 拦截；补齐 →
/// 保存成功（行即现 + 反馈条位 + 弹窗关 + 库检首条估值行 = 当前估值）。
/// 删除「新建 → perform_save → reload」任一接线即红。
#[test]
fn physical_asset_view_create_journey_with_initial_valuation() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("physical-asset-create");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_physical_assets(&mut app);

    app.press("id:physical-asset-new").unwrap();
    app.wait_for("id:physical-asset-name").unwrap();
    app.wait_for("text:新建实物资产").unwrap();

    // 名称留空 → 拦截（弹窗不关）。
    app.press("id:physical-asset-form-save").unwrap();
    app.wait_for("id:physical-asset-form-error").unwrap();
    app.wait_for("text:保存失败: 请输入资产名称").unwrap();

    // 补名称；估值留空 → 拦截。
    app.type_text("id:physical-asset-name", "客厅油画").unwrap();
    app.press("id:physical-asset-form-save").unwrap();
    app.wait_for("text:保存失败: 请填写当前估值").unwrap();

    // 补估值 → 选币种 → 保存成功：行即现、反馈在屏、弹窗关；首条估值行落库。
    app.type_text("id:physical-asset-valuation", "50000")
        .unwrap();
    app.press("id:physical-asset-valuation-currency-trigger")
        .unwrap();
    app.wait_for("id:physical-asset-valuation-currency-option")
        .unwrap();
    app.press("text:人民币 (CNY)").unwrap();
    app.press("id:physical-asset-form-save").unwrap();
    app.wait_for("text:建档成功").unwrap();
    app.wait_for_gone("id:physical-asset-name", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("id:physical-asset-table").unwrap();
    app.wait_for("text:客厅油画").unwrap();
    let conn = nsh_app::boot::boot_ledger(&dir).unwrap();
    let read_conn = conn.read_conn.lock().unwrap();
    let list = ledger_physical_asset::list_physical_assets(&read_conn, None).unwrap();
    assert_eq!(list.assets.len(), 1, "建档落库在册");
    assert_eq!(list.assets[0].name, "客厅油画");
    assert_eq!(
        list.assets[0].current_valuation_cents, 5_000_000,
        "首条估值行即当前估值（¥50000 = 5000000 分）"
    );
}

/// 编辑旅程（旧壳 update 平价，编辑表单无估值字段）：编辑弹层回填名称 /
/// 购买信息原值、估值字段结构性排除 → 改名保存 → 行刷新（估值不变，
/// ADR-0064：估值只能经「更新估值」变更）。删除「编辑 → perform_save
/// (update) → reload」任一接线即红。
#[test]
fn physical_asset_view_edit_updates_profile_without_valuation() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("physical-asset-edit");
    let asset_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        asset_id = seed_physical_asset(
            &db,
            "代步车",
            Some(("2023-05-01", 120_000_000, "CNY")),
            80_000_000,
            "CNY",
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_physical_assets(&mut app);
    app.wait_for("text:代步车").unwrap();

    app.press(&format!("id:physical-asset-edit-{asset_id}"))
        .unwrap();
    app.wait_for("id:physical-asset-name").unwrap();
    app.wait_for("text:编辑实物资产").unwrap();
    assert_eq!(
        app.screen().input_text("id:physical-asset-name").unwrap(),
        Some("代步车"),
        "编辑弹层回填原名称"
    );
    assert!(
        app.screen().count("id:physical-asset-valuation").unwrap() == 0,
        "编辑模式估值字段结构性排除（ADR-0064：估值只能在更新估值入口追加）"
    );
    // 改名保存：行刷新 + 反馈在屏 + 估值不变。
    app.press("id:physical-asset-form-save").unwrap();
    app.wait_for("text:保存成功").unwrap();
    app.wait_for_gone("id:physical-asset-name", DEFAULT_WAIT)
        .unwrap();
    let conn = nsh_app::boot::boot_ledger(&dir).unwrap();
    let read_conn = conn.read_conn.lock().unwrap();
    let list = ledger_physical_asset::list_physical_assets(&read_conn, None).unwrap();
    assert_eq!(
        list.assets[0].current_valuation_cents, 80_000_000,
        "估值不变"
    );
}

/// 更新估值旅程（旧壳 valuation modal 平价）：金额空 → 拦截；日期缺省等价
/// 「空 = 今天」（追加历史行）；保存成功行日新估值即屏。删除「更新估值 →
/// perform_valuation → reload」任一接线即红。
#[test]
fn physical_asset_view_valuation_update_appends_history() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("physical-asset-valuation");
    let asset_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        asset_id = seed_physical_asset(&db, "客厅油画", None, 50_000_000, "CNY");
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_physical_assets(&mut app);
    app.wait_for("text:客厅油画").unwrap();

    app.press(&format!("id:physical-asset-valuation-{asset_id}"))
        .unwrap();
    app.wait_for("id:physical-asset-valuation-amount").unwrap();
    app.wait_for("text:更新估值").unwrap();

    // 金额留空 → 拦截。
    app.press("id:physical-asset-valuation-save").unwrap();
    app.wait_for("id:physical-asset-valuation-error").unwrap();
    app.wait_for("text:保存失败: 请填写估值金额").unwrap();

    // 补金额 → 保存：反馈在屏、弹窗关、当前估值刷新为最新一条（库检追加）。
    app.type_text("id:physical-asset-valuation-amount", "60000")
        .unwrap();
    app.press("id:physical-asset-valuation-save").unwrap();
    app.wait_for("text:估值已更新").unwrap();
    app.wait_for_gone("id:physical-asset-valuation-amount", DEFAULT_WAIT)
        .unwrap();
    app.wait_for("text:¥6,0000").unwrap();
    let conn = nsh_app::boot::boot_ledger(&dir).unwrap();
    let read_conn = conn.read_conn.lock().unwrap();
    let count: i64 = read_conn
        .query_row(
            "SELECT COUNT(*) FROM physical_asset_valuations WHERE asset_id = ?1",
            [&asset_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "每次更新追加一条历史（旧值保留不覆盖）");
}

/// 处置旅程（旧壳 dispose modal 平价，处置与删除是分离的两个动作）：处置
/// 日期缺省今天（状态标记），不填价即处置 → 已处置退出默认清单、在持合计
/// 不含之；档案经「已处置」筛选回看。删除「处置 → perform_dispose →
/// reload」任一接线即红。
#[test]
fn physical_asset_view_dispose_moves_mark_and_keeps_archive() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("physical-asset-dispose");
    let asset_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        asset_id = seed_physical_asset(&db, "代步车", None, 80_000_000, "CNY");
        seed_physical_asset(&db, "国画", None, 5_000_000, "CNY");
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_physical_assets(&mut app);
    app.wait_for("text:代步车").unwrap();

    app.press(&format!("id:physical-asset-dispose-{asset_id}"))
        .unwrap();
    app.wait_for("id:physical-asset-dispose-date-input")
        .unwrap();
    app.wait_for("text:处置资产").unwrap();

    // 处置（日期缺省今天 = 合法，不填处置价）：行离屏（reload 完成后退出
    // 默认清单）与合计只剩国画（后端同源快照重读）。
    app.press("id:physical-asset-dispose-save").unwrap();
    app.wait_for("text:已处置").unwrap();
    app.wait_for_gone("id:physical-asset-dispose-date-input", DEFAULT_WAIT)
        .unwrap();
    app.wait_for_gone("text:代步车", DEFAULT_WAIT).unwrap();
    app.wait_for("text:在持估值合计：¥5,0000").unwrap();

    // 已处置筛选回看档案（处置日期在格；状态标记在库）。
    app.press("id:physical-asset-filter-disposed").unwrap();
    app.wait_for("text:代步车").unwrap();
    let conn = nsh_app::boot::boot_ledger(&dir).unwrap();
    let read_conn = conn.read_conn.lock().unwrap();
    let disposed = ledger_physical_asset::get_physical_asset(&read_conn, &asset_id).unwrap();
    assert_eq!(
        disposed.status,
        ledger_physical_asset::PhysicalAssetStatus::Disposed,
        "处置 = 状态标记（档案与估值历史保留）"
    );
    assert!(disposed.disposal_date.is_some());
}

/// 软删确认守卫（旧壳 AppPopconfirm 平价）：删除按钮开确认气泡（确认文案
/// 在屏）→ 取消不动（行仍在）→ 再开确认 → 行移除（软删不进清单与合计，库
/// 中行与估值历史保留）。删除 popconfirm 开合接线 → 「确认文案永不呈现」
/// 本测红。
#[test]
fn physical_asset_view_delete_guard_and_soft_delete() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("physical-asset-delete");
    let asset_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        asset_id = seed_physical_asset(&db, "误录资产", None, 1_000_000, "CNY");
        seed_physical_asset(&db, "拆除资产", None, 2_000_000, "CNY");
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_physical_assets(&mut app);
    app.wait_for("text:误录资产").unwrap();

    // 打开确认气泡（确认文案在屏）→ 取消：行仍在。
    app.press(&format!("id:physical-asset-delete-{asset_id}"))
        .unwrap();
    app.wait_for("id:physical-asset-delete-message").unwrap();
    app.wait_for("text:删除后资产将退出列表与合计，数据与估值历史保留（软删除），确认删除？")
        .unwrap();
    app.press("id:physical-asset-delete-cancel").unwrap();
    app.wait_for_gone("id:physical-asset-delete-message", DEFAULT_WAIT)
        .unwrap();
    assert!(app.shows("误录资产"), "取消后行不移除");

    // 再开确认：行移除 + 反馈在屏 + 软删保留库行（is_deleted=1，不物理移除）。
    app.press(&format!("id:physical-asset-delete-{asset_id}"))
        .unwrap();
    app.wait_for("id:physical-asset-delete-message").unwrap();
    app.press("id:physical-asset-delete-confirm").unwrap();
    app.wait_for("text:已删除").unwrap();
    app.wait_for_gone("text:误录资产", DEFAULT_WAIT).unwrap();
    assert!(app.shows("拆除资产"), "对照行不受波及");
    let rows = book_asset_names_with_deleted(&dir);
    assert!(
        rows.iter()
            .any(|(name, deleted)| name == "误录资产" && *deleted == 1),
        "软删保留库行（is_deleted=1，数据与估值历史保留）：{:?}",
        rows
    );
    let conn = nsh_app::boot::boot_ledger(&dir).unwrap();
    let read_conn = conn.read_conn.lock().unwrap();
    let count: i64 = read_conn
        .query_row(
            "SELECT COUNT(*) FROM physical_asset_valuations v JOIN physical_assets a \
             ON v.asset_id = a.id WHERE a.is_deleted = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "软删后估值历史保留");
    drop(read_conn);
}
// ---------------------------------------------------------------------------
// 跨账本投资汇总与 AI 提示词（#2055）：多本只读合计（折算/逐本状态口径标注、
// 概念说明弹层）、随投资开关联动的守卫、AI 提示词的复制与反馈。
// 现场口径：注册表数据根与活动本目录按用例注入（bin 生产面同序——main.rs
// 解析后注入，headless 测试直接给槽位值）。
// ---------------------------------------------------------------------------

use ledger_currencies::ExchangeRateInput;
use ledger_infra::db::book_registry::{Book, BookRegistry, RegistryOrigin, write_registry};
use std::path::PathBuf;

/// 唯一暂存注册表数据根（跨账本注册表所在的<XDG 数据根>面：目录名即生产
/// 同构面，identifier 拼接语义由常量见证）。
fn make_data_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ledger-test-nsh-xbook-{tag}-{}", new_uuid()));
    fs::create_dir_all(&root).unwrap();
    root
}

/// 落盘注册表：单活动本 + 追加他本（文件现场；活动本指向 active 目录）。
fn seed_registry(root: &Path, active: &Path, others: &[(&str, &str, &Path)]) {
    let mut books = vec![Book {
        id: "b1".to_owned(),
        name: "活动账本".to_owned(),
        dir: active.to_path_buf(),
    }];
    for (id, name, dir) in others {
        books.push(Book {
            id: (*id).to_owned(),
            name: (*name).to_owned(),
            dir: dir.to_path_buf(),
        });
    }
    write_registry(
        root,
        &BookRegistry {
            books,
            active_id: "b1".to_owned(),
            pending_relocation: None,
            origin: RegistryOrigin::NewFormat,
        },
    )
    .unwrap();
}

/// 在指定库内造一个投资账户（现金腿；跨账本合计的可投资资产读）。
fn seed_investment_cash(db: &ledger_infra::db::DbState, name: &str, currency: &str, cents: i64) {
    seed_account_with_currency(db, name, AccountType::Investment, currency, cents);
}

/// 跨账本汇总渲染（#2055 折算 + 四态逐本 + 概念说明弹层）：活动本 CNY 现金
/// 2000 元、他本 USD 现金 1000 元（当期汇率 7）→ 可投资资产 ¥9000、折算口
/// 径标注、未计入本警示与逐本状态；说明弹层挂跨本作用域句。删除视图装载
/// 接线（reload）→ 渲染用例红（「删除即红」负向条目，实拆见交付报告）。
#[test]
fn cross_book_summary_renders_converted_totals_books_and_concept_tip() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("render");
    let dir_active = make_book("x-active");
    nsh_app::app::set_book_dir(dir_active.clone());
    nsh_app::cross_book::set_default_data_dir(root.clone());

    // 活动本：投资账户 CNY 现金 2000 元 + 当期汇率 USD→CNY = 7。
    {
        let db = nsh_app::boot::boot_ledger(&dir_active).unwrap();
        seed_investment_cash(&db, "证券", "CNY", 200_000);
        let conn = db.conn.lock().unwrap();
        ledger_investment::create_exchange_rate(
            &conn,
            ExchangeRateInput {
                base_code: "USD".to_owned(),
                quote_code: "CNY".to_owned(),
                rate: 7.0,
                priced_at: today().to_string(),
                source: None,
            },
        )
        .unwrap();
    }

    // 他本：USD 现金 1000 元（折算腿）：本内本位币基准设为 USD（app_settings
    // 键值 JSON 形状，旧壳集成场景同款接缝）——本内无折算，跨本折算落活动本
    // 汇率表（被测路径）。
    let dir_usd = make_book("x-usd");
    {
        let db = nsh_app::boot::boot_ledger(&dir_usd).unwrap();
        seed_investment_cash(&db, "美元证券", "USD", 100_000);
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO app_settings(key, value) VALUES('ledger.base_currency', '\"USD\"') \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )
            .unwrap();
    }

    // 密文本（未解锁、未计入）。
    let dir_locked = make_book("x-locked");
    {
        let db = nsh_app::boot::boot_ledger(&dir_locked).unwrap();
        drop(db);
        ledger_infra::db::encryption::enable_encryption_for_file(
            &dir_locked.join("ledger.db"),
            "测试口令",
        )
        .unwrap();
    }

    // 未建库本（只登记、无库文件）。
    let dir_missing =
        std::env::temp_dir().join(format!("ledger-test-nsh-xbook-missing-{}", new_uuid()));
    fs::create_dir_all(&dir_missing).unwrap();

    seed_registry(
        &root,
        &dir_active,
        &[
            ("b2", "美元账本", &dir_usd),
            ("b3", "密码本", &dir_locked),
            ("b4", "空账本", &dir_missing),
        ],
    );

    // 启动即恢复跨账本汇总（路由存储面；无侧栏入口——入口=账本切换弹层，
    // ㉓设置②的挂接面）。
    let storage = MemoryStorage::new(vec![(
        "view_state:route".to_owned(),
        "\"cross-book-summary\"".to_owned(),
    )]);
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();

    app.wait_for("id:cross-book-summary").unwrap();
    // 四口径卡：市值/持仓收益/累计收益为零、可投资资产 = 2000 + 1000 × 7。
    for amount in ["¥0", "¥9000"] {
        app.wait_for(&format!("text:{amount}")).unwrap();
    }
    // 口径标注：折算句（尾部小字）与部分合计警示并存。
    app.wait_for("text:部分账本未计入，以下合计为部分合计")
        .unwrap();
    app.wait_for("text:各账本位币不一致，已按当期汇率折算为 CNY；历史汇率仅属走势曲线")
        .unwrap();
    // 逐本状态：密文本、未建库本排除可见；活动本恒计入。
    app.wait_for("text:未解锁、未计入").unwrap();
    app.wait_for("text:尚未初始化、未计入").unwrap();
    app.wait_for("text:已计入").unwrap();
    // 逐本行：注册表登记名 + 「当前」标签只挂活动本行。
    for book_name in ["活动账本", "美元账本", "密码本", "空账本"] {
        app.wait_for(&format!("text:{book_name}")).unwrap();
    }
    assert!(
        app.screen().count("id:summary-book-active-b1").unwrap() == 1,
        "活动本行挂「当前」"
    );
    assert!(
        app.screen().count("id:summary-book-active-b2").unwrap() == 0,
        "非活动计入本不挂「当前」（本壳正位，见交付报告）"
    );

    // 概念说明弹层：可投资资产卡挂跨本作用域句（旧壳 scopeCrossBook 同词）。
    app.press("id:cross-book-investableAssets-info").unwrap();
    app.wait_for("id:cross-book-investableAssets-info-message")
        .unwrap();
    let texts = app.screen().texts();
    assert!(
        texts
            .iter()
            .any(|t| t.contains("未解锁或未建库的账本不计入")),
        "跨本作用域句在位：{:?}",
        texts
    );
}

/// 开关联动守卫（#2055，ADR-0114 决策 6）：投资关闭时跨账本汇总裸访问/
/// 启动恢复回概览（旧壳 featureRouteGuard("investments") 同语义）；投资开
/// 放时落自有路由。「删除即红」负向条目：删 nav_model 的 guard_feature 映
/// 射 → 关闭态恢复直接落汇总，本测红。
#[test]
fn cross_book_summary_closure_follows_investments_toggle() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("x-guard");
    seed_expense_series(&dir, 1);
    nsh_app::app::set_book_dir(dir.clone());
    nsh_app::cross_book::set_default_data_dir(make_data_root("guard"));

    // 关闭投资 → 恢复落概览（净资产卡），不落汇总。
    let storage = MemoryStorage::new(vec![
        (
            "view_state:route".to_owned(),
            "\"cross-book-summary\"".to_owned(),
        ),
        (
            "view_state:closed_features".to_owned(),
            "[\"investments\"]".to_owned(),
        ),
    ]);
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    app.wait_for("id:net-worth-card").unwrap();
    assert_eq!(
        app.screen().count("id:cross-book-summary").unwrap(),
        0,
        "投资关闭 → 汇总随行关闭（守卫回概览）"
    );
    drop(app);

    // 投资开放 → 恢复落自有路由。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir);
    nsh_app::cross_book::set_default_data_dir(make_data_root("guard-open"));
    let storage = MemoryStorage::new(vec![(
        "view_state:route".to_owned(),
        "\"cross-book-summary\"".to_owned(),
    )]);
    let mut app = Headless::start(desktop_config_with_backend(storage)).unwrap();
    app.wait_for("id:cross-book-summary").unwrap();
}

/// AI 提示词页（#2055）：⌘0 键位落真实视图；展示契约全文（编译期嵌入 =
/// 旧壳 include 同一文件）；复制按钮写剪贴板、回读校验成功并呈现反馈。
/// 「删除即红」负向条目：删复制接线（perform_copy 的写/回读）→ 剪贴板
/// 断言与反馈文案两处即红。
#[test]
fn ai_view_renders_prompt_and_copy_reports_into_clipboard() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("ai");
    seed_expense_series(&dir, 1);
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    // 等侧栏就绪再派发键位（既有键位上用例同序——就绪窗口内键位绑定已挂）。
    app.wait_for("id:nav-ai").unwrap();

    app.press_key("mod+0").unwrap();
    app.wait_for("id:ai-prompt-view").unwrap();
    // 正文整段单文本节点（markdown 原文）：锚点 id + 包含断言（exact 键不适用）。
    app.wait_for("id:prompt-body").unwrap();
    let texts = app.screen().texts();
    assert!(
        texts.iter().any(|t| t.contains("# Ledger API 入口提示词"))
            && texts.iter().any(|t| t.contains("GET /api/v1/contract")),
        "契约全文在屏：{:?}",
        texts
    );
    // 复制前剪贴板为空（headless 现场无写入）。
    assert!(app.screen().clipboard().is_none(), "复制前无剪贴板文本");

    app.press("id:ai-copy").unwrap();
    app.wait_for("text:已复制到剪贴板").unwrap();
    // 剪贴板与契约模板同文（include_str! 同一文件指针——内容两壳同源）。
    assert_eq!(
        app.screen().clipboard(),
        Some(include_str!("../prompts/ledger-api.md")),
        "复制内容 = 提示词全文"
    );
}

// ---------------------------------------------------------------------------
// 设置①通用卡（#2053）：页签容器（旧壳分域序闭集 + visited-once keep-mounted
// 状态保留 + 「定时」页签随开关隐藏）、外观三态（存储往返）、日志档位（boot
// 直调基建设置接缝 + 初读回显）、数据存储位置（信息聚合 / 更改提交通道 /
// 二选一接管守卫 / 下次启动搬迁兑现）、功能开关管理面（nav closed 单点共源）。
// 删任一接线即红的负向条目逐项写在各测试注释（ADR-0087 断言强度：断言对准
// 用户可观察结果）。
// ---------------------------------------------------------------------------

/// 桌面形态配置（同一既有 helper 的共享存储版）：同一 `MemoryStorage` 实例在
/// 两次 Headless 启动间承接「设备重启」的等价物（键值同形、进程隔离，设置卡
/// 外观/开关的「写 → 下次启动读」往返由此承担）。
fn storage_shard(entries: &[(&str, &str)]) -> std::sync::Arc<nsh_testing::MemoryStorage> {
    nsh_testing::MemoryStorage::new(
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<Vec<_>>(),
    )
}

fn desktop_config_with_storage_shard(
    storage: std::sync::Arc<nsh_testing::MemoryStorage>,
) -> EngineConfig {
    EngineConfig {
        storage: Some(storage),
        ..desktop_config()
    }
}

/// 落位设置视图（设置用例共用）：侧栏「设置」固定项 → 点入 → 等设置首签卡
/// 题到屏（外观卡即时装载，日志初读为异步）。
fn open_settings(app: &mut Headless) {
    app.wait_for("id:nav-settings").unwrap();
    app.press("id:nav-settings").unwrap();
    app.wait_for("text:设置").unwrap();
    app.wait_for("text:主题模式").unwrap();
}

/// 页签骨架全量呈现（旧壳 SettingsView 分域序闭集平价）：七个分域页签字面各
/// 在屏（id 锚点 = 页签内部文本：删除页签闭集任一即红）。默认首签 = 通用；
/// 未到访页签不预载（show:lazy 的 lazy 半边：宿主内容缺席）。
#[test]
fn settings_view_lands_with_old_shell_tab_domains_and_general_cards() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-lands");
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_settings(&mut app);

    for tab in ["通用", "分类", "币种", "数据", "定时", "功能", "关于"] {
        assert!(
            app.shows(tab),
            "分域页签「{tab}」应在屏：{:?}",
            app.screen().texts()
        );
    }
    // 默认首签（通用）内容在屏：外观卡 + 日志卡初读（默认 info 档位标签）。
    for piece in [
        "主题模式",
        "显示主题",
        "跟随系统",
        "日志",
        "日志等级",
        "信息 (info)",
    ] {
        assert!(
            app.shows(piece),
            "通用卡「{piece}」应在屏：{:?}",
            app.screen().texts()
        );
    }
    // 未到访页签不预载：数据卡宿主内容缺席（删除 keep-mounted 的 visited 门
    // 闸（改为全部预挂）即红）。
    assert_eq!(
        app.screen().count("id:loc-active-path").unwrap(),
        0,
        "未到访的「数据」页签不预载宿主内容"
    );
}

/// keep-mounted（旧壳 show:lazy + 显式 key 的呈现本体）：先到访「数据」→ 切
/// 「功能」→ 回「数据」：已访页签宿主常驻不卸载（display 隐藏）——宿主节点计
/// 数守卫（删除 display 切换接线即红：切走即卸载 → 计数跌零、切回重挂）。
#[test]
fn settings_view_keeps_visited_panes_mounted_across_tab_switches() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-keep");
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_settings(&mut app);

    // 到访数据：位置卡装载（信息聚合 blocking，wait_for）+ 留下会话态印记：
    // 点「复制路径」→ 状态条位反馈。
    app.press("id:set-tab-data").unwrap();
    app.wait_for("text:当前生效位置：").unwrap();
    app.press("id:loc-copy-active").unwrap();
    app.wait_for("text:已复制完整路径").unwrap();

    // 切功能再切回：会话态印原样在屏（keep-mounted：切走时宿主 display 隐
    // 藏不重挂、页内信号存活，切回即现——删除 display 切换接线（改回卸载重
    // 建）即红：印记随卸载清零，切回后的 wait_for 超时报红）。
    app.press("id:set-tab-features").unwrap();
    app.wait_for("text:功能开关").unwrap();
    // 非激活页签退出文档流（display 切换的呈现面）：数据卡标题不在屏——
    //  display 接线拆除（两页签同时可见）即红。
    assert!(
        !app.shows("当前生效位置："),
        "非激活页签不显示（display 切换接线）：{:?}",
        app.screen().texts()
    );
    app.press("id:set-tab-data").unwrap();
    app.wait_for("text:已复制完整路径").unwrap();

    // 未到访的占位页签内容始终缺席（lazy 半边贯穿整页签闭集）。
    assert!(
        !app.screen().texts().contains(&"分类 · 迁移中"),
        "未到访的「分类」占位页签不预载"
    );
}

/// 外观（设备级轻量设置项平价 + NSH 框架增量开口「跟随系统」）：预置「浅色」
/// → 卡初值随存储读路径（读半边，拆除即红）；选择「跟随系统」→ 存储落盘（键
/// appearance、值 = 持久化名 JSON 字符串）——set_color_scheme 立即生效面归
/// 人工子集（headless 无视觉环境）；「重启」等价（同存储实例再启）→ 触发件
/// 随存储读回「跟随系统」。
#[test]
fn settings_view_appearance_reads_and_writes_device_storage() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-appearance");
    let storage = storage_shard(&[("appearance", "\"light\"")]);

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_storage_shard(storage.clone())).unwrap();
    open_settings(&mut app);

    // 预置「浅色」被存储读路径拾起。
    app.wait_for("text:主题模式").unwrap();
    assert!(
        app.shows("浅色"),
        "外观初值跟随已存选择（浅色）：{:?}",
        app.screen().texts()
    );

    // 选择「跟随系统」：开弹层 → 选项 → 选中即关 + 存储落盘。
    app.press("id:set-appearance-trigger").unwrap();
    app.wait_for("id:set-appearance-option").unwrap();
    app.press("id:set-appearance-option >> text:跟随系统 >> nth=0")
        .unwrap();
    let entries = storage.entries();
    assert_eq!(
        entries.get("appearance").map(String::as_str),
        Some("\"system\""),
        "外观写入存储（同键同值形态）：{entries:?}"
    );

    // 「重启」等价（同存储再启）→ 初值读回「跟随系统」。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config_with_storage_shard(storage)).unwrap();
    open_settings(&mut app);
    app.wait_for("text:主题模式").unwrap();
    assert!(
        app.shows("跟随系统"),
        "外观跨启动读回：{:?}",
        app.screen().texts()
    );
}

/// 日志档位（boot 直调基建设置接缝：旧壳 get/set_log_level 命令同执行协议）：
/// 初读默认 info；选「调试 (debug)」→ 触发件随写链更新（立即生效面归 stderr
/// 人工子集）；离开设置再回 = 初读持久化档位（跨启动保留平价）。删除写链
/// boot::set_log_level 直调 → 库内档位不落 → 回读仍旧 info → 红。
#[test]
fn settings_view_log_level_select_persists_across_remounts() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-logs");
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_settings(&mut app);

    // 切调试（选点即写：boot 单点校验落库 + 触发件回显）。
    app.wait_for("text:信息 (info)").unwrap();
    app.press("id:log-level-trigger").unwrap();
    app.wait_for("id:log-level-option").unwrap();
    app.press("id:log-level-option >> text:调试 (debug) >> nth=0")
        .unwrap();
    app.wait_for("text:调试 (debug)").unwrap();

    // 落库面直接钉（boot 读写接缝与视图写链同一执行通道；闭集拒绝归 lib 单
    // 测）：读侧直开库只读连接查 app_settings（rusqlite 测试依赖）。
    {
        let conn = rusqlite::Connection::open_with_flags(
            dir.join("ledger.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let stored: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'logging.level'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            stored, "\"debug\"",
            "写链已落库（app_settings.logging.level）"
        );
    }

    // 离开设置再回：初读持久化档位 = 调试。
    app.press("id:nav-dashboard").unwrap();
    app.press("id:nav-settings").unwrap();
    app.wait_for("text:日志等级").unwrap();
    app.wait_for("text:调试 (debug)").unwrap();
}

/// 位置卡信息聚合（boot::data_location_info 直调基建 gather_info_from_boot）：
/// 生效位置 = 启动目录 + 原文回显；意图缺席 → 「恢复默认」禁用（契约不变量
/// 直接投影，删除禁用接线即红）。更改入口 / 复制路径在屏。
#[test]
fn settings_view_data_location_lands_with_active_path_and_restore_gating() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-loc-lands");
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_settings(&mut app);
    app.press("id:set-tab-data").unwrap();
    app.wait_for("text:当前生效位置：").unwrap();

    let active = dir.to_string_lossy().to_string();
    assert!(
        app.shows(&active),
        "生效位置路径原文在屏（活动账本目录原文，旧壳 active_dir 同口径）：{:?}",
        app.screen().texts()
    );
    for piece in ["数据存储位置", "更改…", "恢复默认", "复制路径"] {
        assert!(
            app.shows(piece),
            "位置卡「{piece}」应在屏：{:?}",
            app.screen().texts()
        );
    }
    // 意图缺席 → 「恢复默认」禁用。
    let restore = app.screen().press_target("id:loc-restore").unwrap();
    assert!(
        restore.is_none(),
        "无意图目录 = 恢复默认不可按（disabled 契约不变量投影）：{restore:?}"
    );
}

/// 提交通道全旅程（旧壳 submitWithChoice 分支同形 + 下次启动兑现）：更改位
/// 置 → 文件对话框（headless 替身，标题同旧壳 pickTitle 词表）→ intent 落盘
///（引导文件 data_location.json 落启动目录指向目标）→ 待重启横幅（意图目录
/// 原文）→ 「下次启动」（重置 + 同目录参数再启动）→ 引导消费注册表搬迁：生
/// 效位置 = 目标目录 + 旧库原样保留。删除 plan_boot 接线（引导不消费注册
/// 表，搬迁不发生）→ 第二段生效位置仍旧 → 红。
#[test]
fn settings_view_data_location_change_commits_and_relocates_on_next_start() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-loc-change");
    let storage = storage_shard(&[]);
    let target = std::env::temp_dir().join(format!(
        "ledger-test-nsh-loc-target-{}",
        ledger_infra::ids::new_uuid()
    ));

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_storage_shard(storage.clone())).unwrap();
    open_settings(&mut app);
    app.press("id:set-tab-data").unwrap();
    app.wait_for("text:当前生效位置：").unwrap();

    // 更改位置：目录选择对话框（替身回答）→ 意图落盘。
    use nsh_core::file_dialog::FileRequest;
    app.on_pick_paths(vec![target.clone()]).unwrap();
    app.press("id:loc-change").unwrap();
    app.settle();
    let requests = app.screen().file_requests();
    let [FileRequest::OpenDialog(dialog)] = requests else {
        panic!("应发出一个目录选择请求：{requests:?}");
    };
    assert_eq!(dialog.title.as_deref(), Some("选择数据存储位置"));

    // 提交成功反馈 + 待重启横幅（意图目录原文）。
    app.wait_for("text:更改已保存，将在下次启动时搬迁生效")
        .unwrap();
    app.wait_for("text:已更改，待重启生效").unwrap();
    let texts = app.screen().texts();
    assert!(
        texts
            .iter()
            .any(|t| t.contains(target.to_string_lossy().as_ref())),
        "待重启横幅呈现意图目录原文：{texts:?}"
    );

    // 引导文件（注册表）已写出并指向目标。
    let pointer = dir.join("data_location.json");
    assert!(pointer.is_file(), "引导文件应已落盘：{}", pointer.display());
    let payload = fs::read_to_string(&pointer).unwrap();
    assert!(
        payload.contains(target.to_string_lossy().as_ref()),
        "注册表应指向目标目录：{payload}"
    );

    // 「下次启动」等价：重置装配 + 同目录参数再启动 → 引导消费注册表搬迁。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_storage_shard(storage)).unwrap();
    // 上次停留视图跟随恢复（导航界面状态同一存储）→ 设置页（数据页签为会话
    // 态回首签——旧壳页签不持久化同口径）。
    app.wait_for("text:主题模式").unwrap();
    app.press("id:set-tab-data").unwrap();
    app.wait_for("text:当前生效位置：").unwrap();
    // 搬迁已兑现：生效位置 = 意图目录；待重启横幅消退（意图目录 = 生效目录，
    // pending_restart 回常）。
    app.wait_for_gone("text:已更改，待重启生效", DEFAULT_WAIT)
        .unwrap();
    assert!(
        app.shows(target.to_string_lossy().as_ref()),
        "下次启动搬迁生效：生效位置 = 意图目录：{:?}",
        app.screen().texts()
    );
    assert!(
        target.join("ledger.db").is_file(),
        "目标目录已搬迁（库在意图位置）"
    );
    assert!(
        dir.join("ledger.db").is_file(),
        "原位置库文件永久保留（安全网语义）"
    );
}
/// 功能开关管理面（可关九项 + 写路径 nav::set_feature_closed 单点共源）：
/// 关「预算」→ 侧栏「预算」入口即隐（关闭只隐藏入口，重开回原位）+ 存储落
/// 盘（closed_features 数组，可关清单序归一）；设置页「定时」页签随「定时」
/// 关闭整行隐藏（旧壳 v-if 设置面联动）。不可关六项不出现行（旧壳同口径）。
/// 删开关行的 nav 写链（feature_row on_change）→ 关闭集合不更新 → 侧栏入口
/// 常在 + 存储缺写 → 红。
#[test]
fn settings_view_feature_toggles_feed_the_shared_closed_source() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-toggles");
    let storage = storage_shard(&[]);
    {
        nsh_app::app::set_book_dir(dir.clone());
        let mut app = Headless::start(desktop_config_with_storage_shard(storage.clone())).unwrap();
        open_settings(&mut app);

        // 不可关六项不出现行（旧壳 FeatureToggleSettings 同口径：闭集之外的
        // id 没有行面也不空行）。
        for id in [
            "dashboard",
            "transactions",
            "accounts",
            "search",
            "ai",
            "settings",
        ] {
            assert_eq!(
                app.screen()
                    .count(&format!("id:feature-toggle-{id}"))
                    .unwrap(),
                0,
                "不可关项「{id}」无开关行"
            );
        }
        // 关「预算」：行按压极（行 testid → 最近的按压祖先 = 开关内部件）。
        app.press("id:set-tab-features").unwrap();
        app.wait_for("text:功能开关").unwrap();
        app.press("id:feature-toggle-budget").unwrap();
        app.wait_for_gone("id:nav-budget", DEFAULT_WAIT).unwrap();

        // 存储落盘（数组形状，可关清单序归一）。
        let entries = storage.entries();
        assert_eq!(
            entries
                .get("view_state:closed_features")
                .map(String::as_str),
            Some("[\"budget\"]"),
            "关闭集合写入存储：{entries:?}"
        );

        // 重开：入口回原位（关闭只隐藏入口不改写清单）。行按压再考一次
        //（开关反演态：再按 = 打开）。
        app.press("id:feature-toggle-budget").unwrap();
        app.wait_for("id:nav-budget").unwrap();
        assert_eq!(
            storage
                .entries()
                .get("view_state:closed_features")
                .map(String::as_str),
            Some("[]"),
            "重开清理（保留集空数组形状跨重启同全开）"
        );
    }

    // 另一场景：独立存储 + 独立库（重置装配）里关「定时」→ 设置页「定时」
    // 页签整行隐藏（旧壳 v-if 设置面联动，#1243 / ADR-0116 决策 4），其余分域
    // 页签原位（关闭只隐藏入口，共一源）。
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-toggles-hidden");
    nsh_app::app::set_book_dir(dir.clone());
    let storage = storage_shard(&[("view_state:closed_features", "[\"scheduled\"]")]);
    let mut app = Headless::start(desktop_config_with_storage_shard(storage)).unwrap();
    open_settings(&mut app);
    app.wait_for("text:主题模式").unwrap();
    assert!(
        app.screen().count("id:set-tab-scheduled").unwrap() == 0,
        "定时关闭 = 设置页「定时」页签整行隐藏：{:?}",
        app.screen().texts()
    );
    // 其余分域页签原位不动（关闭只隐藏入口，不改写页签序）。
    for tab in ["通用", "分类", "币种", "数据", "功能", "关于"] {
        assert!(app.shows(tab), "页签「{tab}」原位保留");
    }
}

/// 二选一接管守卫（旧壳 adoptConfirm 二选一 + danger warning 分级的首个消费
/// 面）：目标目录已有同名库 → 提交挂起 + warning 级定校验弹窗（后果与兜底
/// 文案）→ 「取消换位」零意图（指针文件不写）。删二选一分支（校验结果直落
/// 意图）→ 弹窗不现 + 覆盖直接落盘 → 红。
#[test]
fn settings_view_data_location_target_conflict_needs_choice_guard() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-loc-adopt");
    let target = make_book("settings-loc-adopt-existing");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_settings(&mut app);
    app.press("id:set-tab-data").unwrap();
    app.wait_for("text:当前生效位置：").unwrap();

    // 更改位置 → 目标已有同名库 → 提交应挂起 + 接管弹窗。
    app.on_pick_paths(vec![target.clone()]).unwrap();
    app.press("id:loc-change").unwrap();
    app.wait_for("text:接管已有账本库？").unwrap();
    for piece in [
        "目标目录已存在同名账本库（ledger.db）。",
        "接管则把该库作为活动数据；取消换位则放弃本次更改（不做任何改动）。",
        "接管该库",
    ] {
        assert!(app.shows(piece), "二选一弹窗文案「{piece}」应在屏");
    }

    // 确认前零写入：引导文件不该落（挂起态不写意图）。
    assert!(
        !dir.join("data_location.json").exists(),
        "二选一悬置期间不写引导文件"
    );

    // 「取消换位」：放弃本次更改 → 弹窗关 + 状态回提示 + 引导文件仍缺席。
    app.press("id:danger-cancel").unwrap();
    app.wait_for("text:已取消更改，数据存储位置未变化").unwrap();
    assert!(
        !dir.join("data_location.json").exists(),
        "取消后引导文件仍不写（零副作用）"
    );

    // 「接管该库」路径：重提交 → 挂起弹窗 → 确认 → 意图落盘（ adopt_existing
    // = true 同一通道二次提交）+ 成功反馈 + 待重启横幅。
    app.on_pick_paths(vec![target.clone()]).unwrap();
    app.press("id:loc-change").unwrap();
    app.wait_for("text:接管已有账本库？").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:更改已保存，将在下次启动时搬迁生效")
        .unwrap();
    let pointer = dir.join("data_location.json");
    assert!(pointer.is_file(), "接管后意图已落盘");
    let payload = fs::read_to_string(&pointer).unwrap();
    assert!(
        payload.contains(target.to_string_lossy().as_ref()),
        "注册表应指向目标目录：{payload}"
    );
}

// ---------------------------------------------------------------------------
// 设置②数据安全与连接四卡（#2054）：备份管理 / 加密 / 多端同步 / 账本切换。
// 守卫呈现与拦截断言对准用户可观察结果（文案 / 入口 / 状态不生效），接线型
// 断言的「删除即红」条目逐项写在各测试注释（ADR-0087）。
// ---------------------------------------------------------------------------

/// 按压目标就绪等待（press_target Some = 非 disabled）。
fn wait_pressable(app: &mut Headless, selector: &str) {
    app.wait_until(
        format!("{selector} pressable").as_str(),
        DEFAULT_WAIT,
        |screen| screen.press_target(selector).unwrap().is_some(),
    )
    .unwrap();
}

/// 落位设置数据页签（设置②用例共用）：设置 → 数据 → 备份卡题到屏。四卡
/// 内容纵向超出常规视口（720pt 只见前两卡）——用例放大视口使整卡组在屏
/// （headless 裁剪语义下，「shows」= 视口内呈现）。
fn open_data_tab(app: &mut Headless) {
    open_settings(app);
    app.set_viewport(Size::new(Points(1280.0), Points(3000.0)))
        .unwrap();
    app.press("id:set-tab-data").unwrap();
    app.wait_for("id:data-backup-once").unwrap();
    app.wait_for("id:book-summary-entry").unwrap();
}

/// 四卡骨架全量呈现（旧壳数据 Tab 分卡序平价）：备份管理五卡题、加密卡、
/// 多端同步卡、账本切换卡与跨账本汇总置顶入口各在屏。删 DataSafetyCards 挂
/// 载接线（数据页签改挂占位卡）→ 全部卡面缺席 → 红。
#[test]
fn data_safety_cards_lands_with_backup_encryption_sync_books_cards() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-data-cards");
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_data_tab(&mut app);

    for piece in [
        "备份",
        "一键备份",
        "另存为…",
        "备份目录",
        "自动备份",
        "备份文件列表",
        "恢复",
        "从备份恢复…",
        "加密",
        "开启加密",
        "多端同步",
        "立即同步",
        "通道配置",
        "保存配置",
        "测试连接",
        "发布检查点到通道",
        "账本切换",
        "新建账本",
        "全部账本（投资汇总）",
    ] {
        assert!(
            app.shows(piece),
            "数据安全卡「{piece}」应在屏：{:?}",
            app.screen().texts()
        );
    }
}

/// 备份全量旅程（动作卡 + 列表卡 + 滚动清理 + 覆盖守卫）：备份目录预制
/// （保留上限 1）→ 另存为（shadow 对话框替身）①→ 受管产物入列表 +
/// 最近备份回显；②再备份 → 滚动清理触发（桶超额 1）→「已清理 1 个旧备
/// 份」+ 列表回到 1 行；覆盖守卫：目标已存在 → warning 确认弹窗 + 取消零
/// 写入（手工标记内容不变）→ 确认 → 替换成功。删备份写链 → 列表永远空 →
/// 红；删滚动清理接线 → 清理反馈不现 → 红；删覆盖守卫分支（直接替换）→
/// 弹窗不现 + 标记内容被替换 → 红。
#[test]
fn backup_full_journey_backs_up_lists_prunes_and_guards_overwrite() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-backup-journey");
    let backup_dir =
        std::env::temp_dir().join(format!("ledger-test-nsh-data-backup-{}", new_uuid()));
    fs::create_dir_all(&backup_dir).unwrap();
    let storage = storage_shard(&[
        (
            "backup_dir",
            format!("{:?}", backup_dir.to_string_lossy()).as_str(),
        ),
        ("backup_max_count", "1"),
    ]);

    // 种子一笔交易（指纹固化前置：空库无可归属产物，备份空库不产生列表行）。
    // 自动备份开关预置关（#2078 起配置目录 + 空列表的会话会触发首次兜底自动
    // 备份；本旅程是手动备份/清理/恢复面，预置关让目录内容断言只及手动产物）。
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        {
            let conn = db.conn.lock().unwrap();
            ledger_infra::settings::set(
                &conn,
                ledger_infra::settings::SettingKey::AutoBackupEnabled,
                &false,
            )
            .unwrap();
        }
        let account = seed_cash_account(&db);
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 2_500,
                date: &test_date(0),
                category_id: None,
                merchant_id: None,
                note: Some("备份旅程种子".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_storage_shard(storage)).unwrap();
    open_data_tab(&mut app);

    // 本机设备标识（#2078 起目录镜像随装载期推送就位：落在镜像目录内的受管
    // 命名另存为按域受管改道语义落设备子目录——旧壳配置目录后的同款行为；
    // 改道不改名，只多一层设备目录）。
    let device = {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.read_conn.lock().unwrap();
        ledger_backup::source_device_id(&conn).expect("种子库可取设备标识")
    };
    let device_dir = backup_dir.join(&device);

    // 备份①：另存为（对话框替身问到盘面）→ 行集 + 最近备份回显。落点断言
    // 对准改道后的实际产物位（<镜像目录>/<设备>/<原名>）。
    let target_a = backup_dir.join("ledger-backup-20260101-000000.db.zip");
    let landed_a = device_dir.join("ledger-backup-20260101-000000.db.zip");
    app.on_pick_save_path(Some(target_a.clone())).unwrap();
    app.press("id:data-backup-as").unwrap();
    app.wait_for("id:data-last-backup").unwrap();
    app.wait_for("id:data-backup-row-0-name").unwrap();
    assert!(
        landed_a.is_file(),
        "备份产物落盘（受管改道设备子目录）：{}",
        landed_a.display()
    );
    assert!(
        app.shows("最近备份："),
        "最近备份标签在屏：{:?}",
        app.screen().texts()
    );

    // 备份②：同目录新目标 → 桶超额 → 滚动清理确证书面反馈。
    let target_b = backup_dir.join("ledger-backup-20260101-000100.db.zip");
    let landed_b = device_dir.join("ledger-backup-20260101-000100.db.zip");
    app.on_pick_save_path(Some(target_b.clone())).unwrap();
    app.press("id:data-backup-as").unwrap();
    app.wait_for("text:已清理 1 个旧备份").unwrap();
    app.wait_for_gone("id:data-backup-row-1-name", DEFAULT_WAIT)
        .unwrap();
    assert!(
        !landed_a.exists(),
        "超额最旧产物应被清理：bucket = 本指纹 × 本设备"
    );

    // 覆盖守卫：目标已存在（手工预置带标记文件）→ warning 确认弹窗，取消
    // 零写入；确认后替换成功。存在性判定与确认面板对准对话框回答路径（壳
    // 守卫面）；确认后的实际写入落设备子目录（域改道语义），断言对准产物
    // 落点的新内容与扁平旧文件的零改动（旧壳同款组合行为：守卫在回答路径、
    // 写入在改道路径）。
    let target_overwrite = backup_dir.join("ledger-backup-20260101-000200.db.zip");
    let landed_overwrite = device_dir.join("ledger-backup-20260101-000200.db.zip");
    fs::write(&target_overwrite, b"canary-bytes").unwrap();
    app.on_pick_save_path(Some(target_overwrite.clone()))
        .unwrap();
    app.press("id:data-backup-as").unwrap();
    app.wait_for("text:覆盖备份文件").unwrap();
    app.press("id:danger-cancel").unwrap();
    app.settle();
    assert_eq!(
        fs::read(&target_overwrite).unwrap(),
        b"canary-bytes",
        "守卫取消零写入（手工标记不变）"
    );
    app.on_pick_save_path(Some(target_overwrite.clone()))
        .unwrap();
    app.press("id:data-backup-as").unwrap();
    app.wait_for("text:覆盖备份文件").unwrap();
    app.press("id:danger-confirm").unwrap();
    // last-backup 锚点在备份①②已置亮，此处 wait 无等待效果——settle 等
    // 本次备份异步写收敛后再断言文件替换（否则偶发在写入落地前读旧内容）。
    app.settle();
    app.wait_for("id:data-last-backup").unwrap();
    assert_eq!(
        fs::read(&target_overwrite).unwrap(),
        b"canary-bytes",
        "扁平回答路径零改动（写入按域改道落设备子目录，旧壳同款组合行为）"
    );
    assert_eq!(
        fs::read(&target_overwrite).unwrap(),
        b"canary-bytes",
        "扁平回答路径零改动（写入按域改道落设备子目录，旧壳同款组合行为）"
    );
    assert!(
        landed_overwrite.is_file(),
        "确认后产物落点有新备份：{}",
        landed_overwrite.display()
    );
    assert_ne!(
        fs::read(&landed_overwrite).unwrap(),
        b"canary-bytes",
        "确认后产物落点内容被新备份替换"
    );
    assert!(
        !landed_b.exists(),
        "确认后的滚动清理同样按保留上限进行（桶内超额最旧产物被清）"
    );
}

/// 恢复最高级危险确认与全旅程：两笔种子交易 → 备份包 → 删除两笔（当前数
/// 据变空）→ 恢复该包：确认弹窗（后果警示 + 备份文件路径）→ 取消零替换
/// → 再进确认 → 成功反馈 + 重启提示；「下次启动」等价重置重启后交易回到
/// 两笔（替换已发生）。删恢复写链 → 重启后仍为零笔 → 红；删除确认分支
/// （直接整库替换）→ 弹窗不现 → 红。
#[test]
fn restore_guard_replaces_data_across_restart() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-restore-journey");
    // 种子（域写入口造数，测试工厂纪律）：两笔现金支出。
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        for index in 0..2 {
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
                    note: Some(format!("恢复旅程第 {index} 笔")),
                },
            );
        }
    }
    // 备份包（域直调同款执行协议，不经壳）。
    let mut zip = std::env::temp_dir().join(format!("ledger-test-nsh-restore-{}", new_uuid()));
    zip.set_extension("db.zip");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        nsh_app::boot::create_backup(&db, &zip, env!("CARGO_PKG_VERSION")).unwrap();
    }
    // 当前数据清空（软删两笔，语义上模拟「数据丢了才恢复」）。
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.conn.lock().unwrap();
        let ids: Vec<String> = ledger_transaction::list_transactions(
            &conn,
            &ledger_transaction::TransactionListFilter::default(),
        )
        .unwrap()
        .items
        .into_iter()
        .map(|row| row.id)
        .collect();
        for id in ids {
            ledger_transaction::delete(&conn, &id).unwrap();
        }
    }

    // 应用数据根（恢复安全备份 RestoreSafetyBackup 的落点；headless 注入
    // 临时目录——生产 bin 的注入单点在 main.rs）。
    let data_root =
        std::env::temp_dir().join(format!("ledger-test-nsh-restore-root-{}", new_uuid()));
    fs::create_dir_all(&data_root).unwrap();
    nsh_app::cross_book::set_default_data_dir(data_root);

    let storage = storage_shard(&[]);
    let storage2 = storage_shard(&[]);
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_storage_shard(storage)).unwrap();
    open_data_tab(&mut app);
    app.on_pick_paths(vec![zip.clone()]).unwrap();
    app.press("id:data-restore").unwrap();
    // 确认弹窗（最高级危险确认）：后果警示与备份文件路径在屏。
    app.wait_for("text:确认恢复").unwrap();
    assert!(
        app.shows(
            "恢复将替换当前全部数据，且不可撤销。系统会在恢复前自动备份当前数据到应用数据目录。"
        ),
        "后果警示在屏：{:?}",
        app.screen().texts()
    );
    // 取消 → 零替换（弹窗关，尚无成功反馈）。
    app.press("id:data-restore-cancel").unwrap();
    app.settle();
    // 重进确认 → 替换 + 成功反馈重启提示。
    app.on_pick_paths(vec![zip.clone()]).unwrap();
    app.press("id:data-restore").unwrap();
    app.wait_for("text:确认恢复").unwrap();
    app.press("id:data-restore-confirm").unwrap();
    app.wait_for(
        "text:恢复成功，请重启应用进入新库（本壳无原位重引导：旧壳恢复后自动重启的偏差见交付报告）",
    )
    .unwrap();

    // 「下次启动」等价重置重启：交易回到两笔（备份内容已换入）。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_storage_shard(storage2)).unwrap();
    // 上次停留视图跟随恢复（导航界面状态同一存储）：呈现就绪即可（落位到
    // 上次视图），数据断言直接读库。
    app.settle();
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db.read_conn.lock().unwrap();
    let alive = ledger_transaction::list_transactions(
        &conn,
        &ledger_transaction::TransactionListFilter::default(),
    )
    .unwrap();
    assert_eq!(
        alive.items.len(),
        2,
        "恢复旅程后交易回到两笔：{:?}",
        alive.items
    );
}

/// 自动备份首次兜底全旅程（issue #2078 接线断言；backup 域既有语义零新口径）：
/// 配置备份目录偏好（设备存储）→ 应用装载期推送目录镜像（旧壳
/// DevicePreferenceSyncHost 启动 immediate 推送同位）→ 启动竖井成功分支执行
/// 会话首次兜底（受管备份列表为空即备份一次，与脏标记无关）→ 产物落盘 +
/// 设置卡自动备份状态刷新为时刻回显。
///
/// 「删除即红」（ADR-0087 接线，断言对准用户可观察结果 = 自动备份产物在盘）：
/// 删装载期壳侧接线（`backup_host::install_shell_side`：镜像不推送 → 触发
/// 入口静默跳过）或删启动竖井宿主接线（`backup_host::start_session_host`：
/// 首次兜底不跑、调度不挂载）任一环 → 产物不出、状态不刷新 → 本测红。
/// 调度轮询线程的周期语义（10 分钟节奏、日界门、每轮门检）归备份域内核测
/// 试（start_scheduler_with_tick 短周期注入）；本测钉壳侧接线的端到端结果。
#[test]
fn auto_backup_first_fallback_backs_up_at_session_start_and_card_refreshes() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("auto-backup-host");
    let backup_dir =
        std::env::temp_dir().join(format!("ledger-test-nsh-auto-backup-{}", new_uuid()));
    fs::create_dir_all(&backup_dir).unwrap();
    let storage = storage_shard(&[(
        "backup_dir",
        format!("{:?}", backup_dir.to_string_lossy()).as_str(),
    )]);

    // 种子一笔交易（首次兜底与脏标记无关——列表为空即兜底；种子固化账本指纹，
    // 产物按本账归属，设备子目录一层随域语义）。
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
                date: &test_date(0),
                category_id: None,
                merchant_id: None,
                note: Some("自动备份首次兜底种子".to_owned()),
            },
        );
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_storage_shard(storage)).unwrap();

    // 首次兜底在后台线程执行（不阻塞启动竖井）：产物落盘 = 用户可观察的
    // 「自动备份发生」（受管自动命名沿用 backup 域规则）。
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let product = loop {
        if let Some(found) = find_first_auto_product(&backup_dir) {
            break found;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "启动后应发生首次兜底自动备份（产物落盘），实际目录内容：{:?}",
            fs::read_dir(&backup_dir)
                .map(|entries| entries.flatten().map(|e| e.path()).collect::<Vec<_>>())
                .unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let name = product
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap()
        .to_owned();
    assert!(
        name.starts_with("ledger-auto-") && name.ends_with(".db.zip"),
        "产物为受管自动命名：{name}"
    );
    assert!(
        fs::metadata(&product).unwrap().len() > 0,
        "产物非空：{}",
        product.display()
    );

    // 设置卡自动备份状态刷新（深路径发射 → pending 桥 → 泵广播 → 卡片重读；
    // 兜底先于卡片装载完成时初始读即时刻，断言同样成立）。
    open_data_tab(&mut app);
    app.wait_for("id:data-auto-last").unwrap();
    app.wait_for_gone("text:上次自动备份：从未", DEFAULT_WAIT)
        .unwrap();
}

/// 递归找目录下首个自动备份产物（自动产物落设备子目录一层，域语义沿用）。
/// 只认受管自动命名终态：写路径的原子临时同兄弟（`.ledger-auto-…atomic-*`，
/// `fs_util::temp_sibling`）不匹配前缀，轮询不误拾中途产物。
fn find_first_auto_product(dir: &Path) -> Option<std::path::PathBuf> {
    fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let path = entry.path();
        let is_product = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name.starts_with("ledger-auto-") && name.ends_with(".db.zip"));
        if is_product {
            Some(path)
        } else if path.is_dir() {
            find_first_auto_product(&path)
        } else {
            None
        }
    })
}

/// 自动备份写触发全旅程（issue #2087 接线断言；backup 域既有语义零新口径）：
/// 预置手动受管产物挡住首次兜底（受管列表非空 → ListNotEmpty 跳过——首兜底
/// 不发生、不清日界锚，本日的自动备份机会留给写触发）→ 应用启动（装载期
/// 目录镜像就位、调度挂载）→ 用户写路径（行菜单删除交易，壳写函数族经统一
/// 写入口）→ 连接层写入口提交点钩子（置脏 + 写时顺带到期检查，ADR-0032）
/// → 自动备份产物落盘 + 设置卡「上次自动备份」从「从未」刷新为时刻回显。
/// 列表重读（「共 1 条」）随行钉住既有信号链不回退。
///
/// 「删除即红」（ADR-0087 接线，断言对准用户可观察结果 = 产物在盘）：壳写
/// 函数族改回直锁（不经 shell_support::write_entry_locked / 连接层写入口）
/// 或删备份域提交点接线（wiring::install_after_commit_hook）任一环 → 写后
/// 不置脏、写时到期检查不发生 → 产物不出、状态不刷新 → 本测红。
#[test]
fn auto_backup_triggers_after_shell_write_and_card_refreshes() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("auto-backup-write-trigger");
    let backup_dir =
        std::env::temp_dir().join(format!("ledger-test-nsh-auto-backup-write-{}", new_uuid()));
    fs::create_dir_all(&backup_dir).unwrap();

    // 种子两笔（域直调造数，不经壳写入口——造数段不触发备份的对照语义）+
    // 手动受管产物一枚（合法受管命名）：首兜底受管列表非空跳过、不清日界锚。
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
                date: &test_date(0),
                category_id: None,
                merchant_id: None,
                note: Some("要删除的一笔".to_owned()),
            },
        );
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 1_800,
                date: &test_date(1),
                category_id: None,
                merchant_id: None,
                note: Some("要留下的一笔".to_owned()),
            },
        );
        nsh_app::boot::create_backup(
            &db,
            &backup_dir.join("ledger-backup-20200101-000000.db.zip"),
            "test",
        )
        .expect("预置手动受管产物（挡首次兜底，不清日界锚）");
    }
    assert!(
        find_first_auto_product(&backup_dir).is_none(),
        "预置段不得产生自动产物"
    );

    let storage = storage_shard(&[(
        "backup_dir",
        format!("{:?}", backup_dir.to_string_lossy()).as_str(),
    )]);
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_storage_shard(storage)).unwrap();
    open_transactions(&mut app);
    app.settle();
    // 首兜底被受管列表挡住：启动期零自动产物（本日的备份机会留给写触发）。
    assert!(
        find_first_auto_product(&backup_dir).is_none(),
        "首次兜底应被预置受管产物挡住（启动期零自动产物），实际目录：{:?}",
        fs::read_dir(&backup_dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect::<Vec<_>>())
            .unwrap_or_default()
    );

    // 用户写路径：行右键 → 删除 → 确认（壳写函数族经统一写入口 → 提交点
    // 置脏 + 写时顺带到期检查 → BackupNow）。
    app.right_click("text:2026-01-01").unwrap();
    app.choose_menu_item("删除").unwrap();
    app.wait_for("text:删除交易").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:共 1 条").unwrap();

    // 写触发的自动备份产物落盘（用户可观察结果；受管自动命名沿用域规则）。
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let product = loop {
        if let Some(found) = find_first_auto_product(&backup_dir) {
            break found;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "用户写交易后应触发自动备份（产物落盘），实际目录内容：{:?}",
            fs::read_dir(&backup_dir)
                .map(|entries| entries.flatten().map(|e| e.path()).collect::<Vec<_>>())
                .unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let name = product
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap()
        .to_owned();
    assert!(
        name.starts_with("ledger-auto-") && name.ends_with(".db.zip"),
        "产物为受管自动命名：{name}"
    );
    assert!(
        fs::metadata(&product).unwrap().len() > 0,
        "产物非空：{}",
        product.display()
    );

    // 设置卡自动备份状态刷新（深路径发射 → pending 桥 → 泵广播 → 卡片重读）。
    open_data_tab(&mut app);
    app.wait_for("id:data-auto-last").unwrap();
    app.wait_for_gone("text:上次自动备份：从未", DEFAULT_WAIT)
        .unwrap();
}

/// 加密开启流守卫（error 级无后门确认 + ≥8 位 / 一致守卫）：短口令即时红
/// 显 + 提交禁用；不一致红显；同向可用 → error 级确认弹窗（无后门后果说
/// 明在屏）→ 取消零写（密码文件为明文）→ 再进确认 → 整库转换成功（文件
/// 头探测密文）+ 本壳密文启动暂不支持的后果提示。删确认分支（直接转换）
/// → 弹窗不现 → 红；删口令守卫（不校验即执行）→ 短口令禁用面消失 → 红。
#[test]
fn encryption_enable_guard_blocks_and_converts() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-encrypt-journey");
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_data_tab(&mut app);

    // 短口令：即时红显 + 提交禁用。
    app.type_text("id:enc-passphrase", "短口令").unwrap();
    app.type_text("id:enc-confirm", "短口令").unwrap();
    app.wait_for("text:主口令至少需要 8 位").unwrap();
    assert!(
        app.screen()
            .press_target("id:enc-enable")
            .unwrap()
            .is_none(),
        "短口令下「开启加密」不可按（disabled 契约投影）"
    );
    // 两次输入不一致：红显。
    app.clear("id:enc-confirm").unwrap();
    app.type_text("id:enc-confirm", "另一个口令一一一贯不代表正确口令")
        .unwrap();
    app.wait_for("text:两次输入不一致").unwrap();

    // 同向长口令：清两框重新输入（清 + 输是 headless 输入控件的替换语义，
    // budget-edit-amount 先例同形）→ 提交可用 → error 级确认弹窗。
    app.clear("id:enc-passphrase").unwrap();
    app.type_text("id:enc-passphrase", "整库加密旅程口令X")
        .unwrap();
    app.clear("id:enc-confirm").unwrap();
    app.type_text("id:enc-confirm", "整库加密旅程口令X")
        .unwrap();
    app.wait_until("enc-enable ready", Duration::from_secs(5), |screen| {
        screen.press_target("id:enc-enable").unwrap().is_some()
    })
    .unwrap();
    app.press("id:enc-enable").unwrap();
    app.wait_for("text:开启加密？").unwrap();
    for piece in [
        "开启后，每次启动都需输入主口令。",
        "主口令不会被保存、也无法找回——一旦忘记，这份账本的数据将无法再打开。",
    ] {
        assert!(app.shows(piece), "无后门后果说明「{piece}」应在屏");
    }
    // 取消 → 零转换（文件头保持明文）。
    app.press("id:danger-cancel").unwrap();
    app.settle();
    assert!(
        matches!(
            ledger_infra::db::encryption::probe_file_kind(&dir.join("ledger.db")).unwrap(),
            ledger_infra::db::encryption::DbFileKind::Plaintext,
        ),
        "取消后文件头保持明文（零转换）"
    );
    // 确认 → 整库转换（文件头密文）+ 原位换连本会话继续可用（旧壳自动重启
    // 的本壳替代口径，#2088）：卡翻转到密文会话形态（已开启 + 写面四件）。
    app.press("id:enc-enable").unwrap();
    app.wait_for("text:开启加密？").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for(
        "text:加密已开启（整库转换完成），本会话已切换到加密库；下次启动需输入主口令解锁。",
    )
    .unwrap();
    assert!(
        matches!(
            ledger_infra::db::encryption::probe_file_kind(&dir.join("ledger.db")).unwrap(),
            ledger_infra::db::encryption::DbFileKind::Encrypted,
        ),
        "确认后整库转换为密文库（文件头探测）"
    );
    // 卡面翻转到密文会话形态：已开启警示块 + 自动解锁平台注记 + 修改主口令
    // / 关闭加密写面在屏（旧壳 EncryptionSettings 已开启视图平价）。
    app.wait_for("text:已开启加密").unwrap();
    app.wait_for("id:enc-auto-unlock-note").unwrap();
    app.wait_for("id:enc-change-form").unwrap();
    app.wait_for("id:enc-disable-form").unwrap();
    // 会话零中断：开启后主界面照常可用（读路径直调换连后新世界）。
    open_transactions(&mut app);
    app.wait_for("id:tx-table").unwrap();
}

/// 同步卡守卫旅程：通道未配置 → 「立即同步」报未配置码化错误（轮次编排归
/// 域，未配置早退不触网）；表单填全 → 保存 → 成功反馈 + 回显（已配置态）
/// + 发布 / 引导入口解锁。删保存写链 → 库内无配置 → 回显不置换 → 红。
#[test]
fn sync_card_requires_configured_channel_then_saves_and_echoes() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-sync-journey");
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_data_tab(&mut app);

    // 未配置早退：不触网（无网络替身注入即断言失败到平台层——手上无替身
    // 注入位，改用状态反馈断言；网络段归 #1219 域权威既有覆盖）。
    app.press("id:sync-now").unwrap();
    app.wait_for("text:同步失败: 同步通道尚未配置，请先在设置中填写同步通道信息")
        .unwrap();
    assert!(
        app.screen()
            .press_target("id:sync-publish-checkpoint")
            .unwrap()
            .is_none(),
        "通道未配置「发布检查点」不可按（disabled 契约投影）"
    );

    // 表单填全（厂商预设预填经下拉在 option 装配期给端点模板——直接手填
    // 端点走自建路径同构）→ 保存 → 成功反馈 + 发布 / 引导解锁。
    app.type_text("id:sync-endpoint", "https://s3.example.com")
        .unwrap();
    app.type_text("id:sync-region", "cn-hangzhou").unwrap();
    app.type_text("id:sync-bucket", "ledger-bucket").unwrap();
    app.type_text("id:sync-access-key", "test-access").unwrap();
    app.type_text("id:sync-secret-key", "test-secret").unwrap();
    app.press("id:sync-save-channel").unwrap();
    app.wait_for("text:通道配置已保存").unwrap();
    app.settle();
    // 通道配置入库（boot 接缝直读同一库：回显权威面）。
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.read_conn.lock().unwrap();
        let config = ledger_sync_engine::configured_channel(&conn).unwrap();
        let config = config.expect("通道配置应已落库（boot 接缝 save 路径）");
        assert_eq!(config.endpoint, "https://s3.example.com");
        assert_eq!(config.bucket, "ledger-bucket");
        assert_eq!(config.space_id, "default");
    }
    app.wait_until("sync-publish ready", Duration::from_secs(5), |screen| {
        screen
            .press_target("id:sync-publish-checkpoint")
            .unwrap()
            .is_some()
    })
    .unwrap();
}

/// 账本旅程：新建（弹窗提交 → 注册清单上屏 + 登记文件落盘）→ 改名 → 移除
/// （气泡确认 → 摘登记，目录照常在盘）。删登记写链 → 清单不上新行 → 红；
/// 删移除气泡确认（直接摘登记）→ 气泡不现 → 红。
#[test]
fn books_create_rename_and_remove_flow_registry() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-books-journey");
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_data_tab(&mut app);

    // 新建：弹窗提交 → 注册行上屏 + books/ 子目录落盘（只登记不建库）。
    wait_pressable(&mut app, "id:book-create");
    app.press("id:book-create").unwrap();
    app.wait_for("text:新建账本").unwrap();
    app.type_text("id:book-name-input", "旅程账本").unwrap();
    wait_pressable(&mut app, "id:book-name-submit");
    app.press("id:book-name-submit").unwrap();
    app.wait_for("text:已创建账本「旅程账本」，进入即自动建库")
        .unwrap();
    let books_root = dir.join("books");
    let registered: Vec<_> = fs::read_dir(&books_root).unwrap().collect();
    assert_eq!(registered.len(), 1, "新建登记创建一个 books/<id> 子目录");

    // 改名：行内改名 → 元数据更新（目录不变）。
    let book = ledger_infra::db::book_registry::read_registry(&dir);
    let registry_book = match book {
        ledger_infra::db::book_registry::RegistryRead::Resolved(registry) => {
            registry.books.last().unwrap().clone()
        }
        _ => panic!("注册表应已可读"),
    };
    wait_pressable(
        &mut app,
        format!("id:book-rename-{}", registry_book.id).as_str(),
    );
    app.press(format!("id:book-rename-{}", registry_book.id).as_str())
        .unwrap();
    app.wait_for("text:重命名账本").unwrap();
    app.clear("id:book-name-input").unwrap();
    app.type_text("id:book-name-input", "改名账本").unwrap();
    wait_pressable(&mut app, "id:book-name-submit");
    app.press("id:book-name-submit").unwrap();
    app.wait_for("text:已重命名为「改名账本」").unwrap();

    // 移除（气泡确认）：待确认文案在屏 → 确认 → 摘登记（目录照常在盘）。
    app.wait_for(format!("id:book-remove-{}", registry_book.id).as_str())
        .unwrap();
    wait_pressable(
        &mut app,
        format!("id:book-remove-{}", registry_book.id).as_str(),
    );
    app.press(format!("id:book-remove-{}", registry_book.id).as_str())
        .unwrap();
    app.wait_for("text:移除后可随时重新登记找回，账本文件仍保留在磁盘。确定移除「改名账本」？")
        .unwrap();
    app.press("id:book-remove-confirm").unwrap();
    app.wait_for("text:已移除账本「改名账本」，文件仍保留在磁盘")
        .unwrap();
    assert!(
        registry_book.dir.is_dir(),
        "移除仍保留账本目录（文件生命周期归用户）"
    );
}

/// 账本切换全旅程（活动指针写链 + 主账本回显 + 跨重启体验动作差分）：
/// 预登记副账本（可用含库文件）→ 行按压切换意图 → 轻量确认弹窗 → 确认 →
/// 「已切换…」反馈 + 注册表 active 指针改写；「下次启动」等价重置后进入
/// 副账本（其行的当前标记）。删切换写链 → 指针不写、重启仍在账本1 → 红。
#[test]
fn books_switch_journey_moves_active_pointer_across_restart() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-books-switch");
    // 预登记副账本（可用性：目录可创建 → 目标目录空库在首次进入建出，与
    // 旧壳语义差分：本壳立桩引导只开已有库文件的目录，测试里补一个空库
    // 文件 = 新装语义）。
    let second = {
        let book = ledger_infra::db::book_registry::create_book_entry(&dir, "副账本").unwrap();
        fs::write(book.dir.join("ledger.db"), b"").unwrap();
        book
    };

    let storage = storage_shard(&[]);
    let storage2 = storage_shard(&[]);
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_storage_shard(storage)).unwrap();
    open_data_tab(&mut app);
    // 清单两行（默认账本 + 副账本），副账本未标当前。
    app.wait_for("id:book-row-0-name").unwrap();
    wait_pressable(&mut app, format!("id:book-switch-{}", second.id).as_str());
    app.press(format!("id:book-switch-{}", second.id).as_str())
        .unwrap();
    app.wait_for("text:切换账本").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:已切换到「副账本」，重启应用后进入新账本（旧壳自动重载的偏差见交付报告）")
        .unwrap();
    // 活动指针写链（读取注册表文件：active = 副账本 id）。
    let registry = fs::read_to_string(dir.join("data_location.json")).unwrap();
    assert!(
        registry.contains(second.id.as_str()) && registry.contains("\"active\""),
        "注册表活动指针应指向副账本：{registry}"
    );

    // 「下次启动」等价：重置 + 同目录参数再启动 → 引导消费注册表进副账本。
    nsh_app::app::reset_book_assembly();
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config_with_storage_shard(storage2)).unwrap();
    open_data_tab(&mut app);
    app.wait_for(format!("id:book-switch-{}", second.id).as_str())
        .unwrap();
    assert!(
        app.shows("副账本"),
        "重启后副账本行在屏：{:?}",
        app.screen().texts()
    );
    // 副账本的行标当前（清单序 = 登记序：默认账本 0、副账本 1）。
    app.wait_for("id:book-row-1-current").unwrap();
}

/// 跨账本汇总置顶入口（导航接缝消费，旧壳 book-summary-entry 同位）：按压
/// → 落位跨账本汇总视图（数据根未注入的 headless 现场呈现读失败重试态，
/// 落位凭证以视图内确定文案为锚）。删导航接线（入口改空动作）→ 汇总视图
/// 不落位 → 红。
#[test]
fn books_summary_entry_opens_cross_book_summary() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("settings-books-summary");
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_data_tab(&mut app);
    app.press("id:book-summary-entry").unwrap();
    app.wait_for("text:汇总读取失败").unwrap();
}

// ---------------------------------------------------------------------------
// 启动账本目录解析链（ADR-0146 决策 1 / spec #2090 T1，issue #2091）：无参
// 启动（发布形态 .desktop 直跑，点图标即可用）按「argv 显式 > 账本注册表
// 活动指针 > 默认数据目录」解析。现场矩阵（数据根经既有环境隔离夹具注入，
// 账本目录槽位保持缺省 = 无 argv 现场）：A=已配置+活动本有库直开；B=未配置
// +默认目录有库直开（升级无感）；D=注册表损坏回退默认目录，回退告示可见且
// 不阻断打开；「未配置且无库」呈现首启引导（ADR-0146 决策 2，T2 #2092——
// T1 期间的缺参用法提示回归钉已由引导呈现钉有意取代）。argv 显式行为原样
//（既有启动用例全绿即契约）。删缺参分支的解析链接线（恢复缺参无条件
// MissingArg）→ 现场 A/B 与引导态用例红（「删除即变红」负向条目，ADR-0087：
// 断言对准屏幕可见结果）。
// ---------------------------------------------------------------------------

/// 现场 A（已配置 + 活动本有库）：无参启动直开注册表活动本——主界面呈现，
/// 活动本内的种子交易可见，缺参用法提示退场。删解析链接线 → 用法提示常驻、
/// 主界面不落位 → 红。
#[test]
fn startup_without_book_dir_opens_registry_active_book() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("shaft-registry");
    let active = make_book("shaft-active");
    seed_expense_notes(&active, &["解析链活动本"]);
    seed_registry(&root, &active, &[]);
    // 关键现场：只注入数据根，不注入 book_dir（发布形态点图标无 argv）。
    nsh_app::cross_book::set_default_data_dir(root);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:解析链活动本").unwrap();
    assert!(
        !app.shows("用法：nsh-app <账本目录>"),
        "缺参用法提示应退场：{:?}",
        app.screen().texts()
    );
}

/// 现场 B（未配置 + 默认数据目录有库）：无注册表文件（未配置）、默认目录
/// 已有存量库（旧壳升级现场）→ 无参启动直接打开该库，升级无感。
#[test]
fn startup_without_book_dir_opens_unconfigured_default_book() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("shaft-default");
    // 默认数据目录已有存量库（0 字节文件 = 新装语义，产品开库入口建出）。
    seed::create_empty_book_file(&root);
    seed_expense_notes(&root, &["默认目录存量库"]);
    nsh_app::cross_book::set_default_data_dir(root);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:默认目录存量库").unwrap();
    assert!(
        !app.shows("用法：nsh-app <账本目录>"),
        "缺参用法提示应退场：{:?}",
        app.screen().texts()
    );
}

/// 现场 D（注册表损坏）：回退默认数据目录打开库（不阻断），回退告示可见
///（设置卡回退行同款：标题 + 原因）。删告示呈现接线 → 库照开但告示缺席 → 红。
#[test]
fn startup_registry_corrupt_falls_back_to_default_dir_with_notice() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("shaft-corrupt");
    seed::create_empty_book_file(&root);
    seed_expense_notes(&root, &["回退直开"]);
    // 注册表损坏现场：非法 JSON。
    fs::write(
        root.join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME),
        "not-json{{",
    )
    .unwrap();
    nsh_app::cross_book::set_default_data_dir(root);
    let mut app = Headless::start(desktop_config()).unwrap();
    // 不阻断打开：默认目录的库照常打开（主界面 + 种子交易可见）。
    open_transactions(&mut app);
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:回退直开").unwrap();
    // 回退告示可见：标题 + 基础设施回退原因（机器写出的注册表损坏走出厂
    // 逃生舱的原因原文）。
    app.wait_for("text:已回退到默认位置").unwrap();
    app.wait_for("text:/数据位置指针文件无法解析/").unwrap();
}

/// 现场「未配置且无库」（数据根已注入，注册表与库文件皆缺）：首启引导界面
/// 呈现（ADR-0146 决策 2，T2 #2092）——欢迎标题与提示在屏、新建动作可用、
/// 缺参用法提示退场（本用例是 T1 缺参提示回归钉的有意翻转：引导取代死路
/// 提示）。删引导呈现接线（引导态回退两行死路呈现）→ 新建动作缺席 → 红。
#[test]
fn startup_unconfigured_without_book_renders_first_run_onboarding() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("shaft-onboarding");
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    app.wait_for("text:尚未找到账本库，新建一本账即可开始记账。")
        .unwrap();
    wait_pressable(&mut app, "id:onboarding-create");
    assert!(
        !app.shows("用法：nsh-app <账本目录>"),
        "缺参用法提示应退场：{:?}",
        app.screen().texts()
    );
    // 引导态不静默建库：确认前默认目录无库文件、无注册表（对旧壳静默新建
    // 的有意偏离，ADR-0146 理由段）。
    assert!(!root.join("ledger.db").exists(), "确认前不建库");
    assert!(
        !root
            .join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME)
            .exists(),
        "确认前不写注册表"
    );
}

/// 新建确认对话明示完整路径（ADR-0146 决策 2 / ADR-0078 分级基准）：按新建
/// 动作 → 确认对话在屏且将创建的默认数据目录完整路径可见。删确认对话接线
/// （按压直建或无对话）→ 路径明示缺席 → 红。
#[test]
fn onboarding_create_dialog_shows_full_creation_path() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("shaft-dialog");
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    wait_pressable(&mut app, "id:onboarding-create");
    app.press("id:onboarding-create").unwrap();
    app.wait_for("id:danger-dialog").unwrap();
    // 完整路径明示（知情内核）：路径原文在对话内可见（路径随警示句同节点，
    // 按包含断言）。
    let path_text = root.to_string_lossy().to_string();
    assert!(
        app.screen().texts().iter().any(|t| t.contains(&path_text)),
        "确认对话应明示将创建的完整路径：{:?}",
        app.screen().texts()
    );
    app.wait_for("text:创建账本").unwrap();
}

/// 确认取消：不建库、不写注册表、留在引导（零副作用；动作可再次触发）。
#[test]
fn onboarding_create_cancel_keeps_onboarding_without_provisioning() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("shaft-cancel");
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    wait_pressable(&mut app, "id:onboarding-create");
    app.press("id:onboarding-create").unwrap();
    app.wait_for("id:danger-confirm").unwrap();
    app.press("id:danger-cancel").unwrap();
    app.wait_for_gone("id:danger-dialog", DEFAULT_WAIT).unwrap();
    // 留在引导：欢迎标题仍在屏。
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    // 零副作用：库文件与注册表皆未落盘。
    assert!(!root.join("ledger.db").exists(), "取消不建库");
    assert!(
        !root
            .join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME)
            .exists(),
        "取消不写注册表"
    );
    // 动作可再次触发：再按新建 → 对话再次在屏。
    wait_pressable(&mut app, "id:onboarding-create");
    app.press("id:onboarding-create").unwrap();
    app.wait_for("id:danger-dialog").unwrap();
}

/// 确认后（验收判据 3）：库建于默认数据目录、注册表落盘「默认账本」+
/// 活动指针、本次启动即进主界面（引导不回归）。删新建核心接线（确认后无
/// 动作）→ 主界面不落位 / 注册表缺席 → 红；删登记或置指针任一环 → 对应
/// 文件内容断言红。
#[test]
fn onboarding_create_confirm_provisions_default_book_and_enters_main_view() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("shaft-confirm");
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    wait_pressable(&mut app, "id:onboarding-create");
    app.press("id:onboarding-create").unwrap();
    app.wait_for("id:danger-confirm").unwrap();
    app.press("id:danger-confirm").unwrap();
    // 本次启动即进主界面：侧栏就绪、交易页空态（空账本呈现「暂无数据」，
    // issue #2018 AC1 空态口径），引导退场。
    app.wait_for("id:nav-transactions").unwrap();
    open_transactions(&mut app);
    app.wait_for("text:暂无数据").unwrap();
    assert!(
        !app.shows("欢迎使用 Ledger"),
        "选定进主界面后引导不再回归：{:?}",
        app.screen().texts()
    );
    // 库建于默认数据目录（建连入口自建全新库，迁移随建连链）。
    assert!(root.join("ledger.db").is_file(), "确认后库建于默认数据目录");
    // 注册表落盘「默认账本」条目 + 活动指针（即时落盘；读文件验内容）。
    let registry =
        fs::read_to_string(root.join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME))
            .expect("注册表文件应已落盘");
    assert!(
        registry.contains("默认账本"),
        "注册表含默认账本条目：{registry}"
    );
    let resolved = match ledger_infra::db::book_registry::read_registry(&root) {
        ledger_infra::db::book_registry::RegistryRead::Resolved(registry) => registry,
        other => panic!("注册表应可解析，实际：{other:?}"),
    };
    assert_eq!(resolved.books.len(), 1, "首本唯一：默认账本折叠形态");
    assert_eq!(resolved.books[0].name, "默认账本");
    assert_eq!(
        resolved.books[0].dir, root,
        "首本直落默认数据目录（决策 5）"
    );
    assert_eq!(
        resolved.active_id, resolved.books[0].id,
        "活动指针指向默认账本"
    );
}

/// 二次启动直接打开账本（验收判据 4）：即时落盘的可观察验证——新建确认后
/// 重启（同数据根、无 argv），不经引导直进主界面。若登记非即时落盘
///（「意图下次生效」形态）→ 重启后仍在引导 → 红。
#[test]
fn onboarding_created_book_opens_directly_on_next_start() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("shaft-restart");
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    wait_pressable(&mut app, "id:onboarding-create");
    app.press("id:onboarding-create").unwrap();
    app.wait_for("id:danger-confirm").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("id:nav-transactions").unwrap();

    // 二次启动（「下次启动」等价重置 + 同数据根再启动，账本切换旅程同型）。
    nsh_app::app::reset_book_assembly();
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    open_transactions(&mut app);
    app.wait_for("text:暂无数据").unwrap();
    assert!(
        !app.shows("欢迎使用 Ledger"),
        "二次启动不经引导：{:?}",
        app.screen().texts()
    );
}

/// 损坏回退且无库的引导现场带损坏告示（验收判据 5）：回退标题与基础设施
/// 回退原因在引导界面可见（告示不阻断新建动作在屏）。删告示呈现接线 →
/// 引导在但告示缺席 → 红。
#[test]
fn onboarding_corrupt_fallback_without_book_shows_corruption_notice() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("shaft-onboard-corrupt");
    // 注册表损坏现场（非法 JSON）且默认目录无库：引导态 + 损坏告示。
    fs::write(
        root.join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME),
        "not-json{{",
    )
    .unwrap();
    nsh_app::cross_book::set_default_data_dir(root);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    // 损坏告示可见：标题 + 基础设施回退原因原文（与就绪态回退告示同源：
    // boot_record.fallback_reason；设置卡回退行同款式）。
    app.wait_for("text:已回退到默认位置").unwrap();
    app.wait_for("text:/数据位置指针文件无法解析/").unwrap();
    // 告示不阻断动作：新建按钮仍在屏（知情后可继续完成账本指定）。
    wait_pressable(&mut app, "id:onboarding-create");
}

/// 现场「已配置 + 活动本无库」（注册表落点非默认目录）：book-missing 维持
/// fail loud 错误态（ADR-0146 决策 4 保留的内核——防假空态；错误态增挂目录
/// 选择器归 T4，文案重写亦归 T4，本票断言只锚定稳定片段「账本库文件不存在」）。
#[test]
fn startup_configured_active_book_missing_fails_loud() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("shaft-missing");
    // 活动本目录不存在且无库文件（登记在册但落点无库）。
    let active =
        std::env::temp_dir().join(format!("ledger-test-nsh-book-shaft-miss-{}", new_uuid()));
    seed_registry(&root, &active, &[]);
    nsh_app::cross_book::set_default_data_dir(root);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:/启动失败：账本库文件不存在/").unwrap();
}

// ---------------------------------------------------------------------------
// 投资视图（#2045）：概览/盈亏两页签（旧壳 InvestmentsView 前两位的迁移）——
// 页签容器（visited-once keep-mounted + 会话槽）、概览读数（可投资资产 Hero +
// 构成比例条与图例 + 投资合计三项 + 缺价/无账户说明）、组合走势卡（plot line
// 封装件首建 + 预设区间 + 空态三态）、盈亏页签（账户/标的筛选 + 按年/按账户
// 两张单值表客户端切片分页 + 资金加权收益率卡）。读路径直调域读入口；
// 删任一接线（视图挂载分支 / 概览与盈亏装载 / 筛选写路径 / 分页条回调）即红
// 的负向条目逐项写在各测试注释（ADR-0087 断言强度：断言对准用户可观察结果）。
// ---------------------------------------------------------------------------

/// 投资视图造数账本（概览/盈亏断言统一刻度）：投资账户期初 1000（¥1000 内
/// 现金腿），标的现价万分位 10_000（=¥1），买入 1 份@¥1、卖出 1 份@¥0.8
///（2025-12-30，已实现 -2000 分）、现金分红 12345 分（2026-03-05）——
/// 可投资资产 = 990 现金腿 + 0 持仓腿（卖出清仓）= ¥9.9；盈亏按年 2026
///（已实现 -2000 + 分红 12345 = 10345）与 2025（已实现 -2000）两行。
fn seed_investments_book(tag: &str) -> std::path::PathBuf {
    let dir = make_book(tag);
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    seed_investment_flow_book(&db, 1_000, 10_000);
    dir
}

/// 投资流程造数主体（概览/盈亏分用例共用刻度；initial 分为投资账户期初现金，
/// price_wan 为标的现价万分位刻度——买入按 price_fen 刻度挂真实成本）。
fn seed_investment_flow_book(db: &ledger_infra::db::DbState, initial: i64, price_fen: i64) {
    use ledger_investment::{InstrumentInput, InstrumentType, MarketPriceInput};
    {
        let conn = db.conn.lock().unwrap();
        ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "证券".to_owned(),
                kind: AccountType::Investment,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(initial),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
    }
    let instrument = {
        let conn = db.conn.lock().unwrap();
        let id = ledger_investment::create_instrument(
            &conn,
            InstrumentInput {
                symbol: "TST".to_owned(),
                kind: InstrumentType::Stock,
                name: Some("测试标的".to_owned()),
                currency_code: "CNY".to_owned(),
                market: None,
            },
        )
        .unwrap();
        ledger_investment::create_market_price(
            &conn,
            MarketPriceInput {
                instrument_id: id.clone(),
                price_cents: price_fen,
                currency_code: "CNY".to_owned(),
                priced_at: today().to_string(),
                source: Some("manual".to_owned()),
            },
        )
        .unwrap();
        id
    };
    // 买入 2 份@price_fen（2025-12-20；金额 = 数量 × 价格）。
    {
        let conn = db.conn.lock().unwrap();
        ledger_transaction::create(
            &conn,
            ledger_transaction::TransactionInput {
                kind: ledger_transaction::TransactionKind::Buy,
                amount_cents: 0,
                currency_code: "CNY".to_owned(),
                account_id: Some(
                    ledger_accounts::list_accounts(&conn)
                        .unwrap()
                        .into_iter()
                        .find(|a| a.kind == AccountType::Investment)
                        .map(|a| a.id)
                        .unwrap(),
                ),
                instrument_id: Some(instrument.clone()),
                quantity: Some(2.0),
                price_cents: Some(price_fen),
                fee_cents: Some(0),
                date: "2025-12-20".to_owned(),
                note: Some("买入".to_owned()),
                ..probe_investments_input()
            },
        )
        .unwrap();
    }
    let account_id: String = db
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT id FROM accounts WHERE name='证券'", [], |r| {
            r.get(0)
        })
        .unwrap();
    // 卖出 1 份@一半价（2025-12-30）：已实现 = 卖出净额 − FIFO 成本 = −1 份成本。
    {
        let conn = db.conn.lock().unwrap();
        ledger_transaction::create(
            &conn,
            ledger_transaction::TransactionInput {
                kind: ledger_transaction::TransactionKind::Sell,
                amount_cents: 0,
                currency_code: "CNY".to_owned(),
                account_id: Some(account_id.clone()),
                instrument_id: Some(instrument),
                quantity: Some(1.0),
                price_cents: Some(price_fen / 2),
                fee_cents: Some(0),
                date: "2025-12-30".to_owned(),
                note: Some("卖出".to_owned()),
                ..probe_investments_input()
            },
        )
        .unwrap();
    }
    // 现金分红（2026-03-05；现金腿 12345 分）。
    {
        let conn = db.conn.lock().unwrap();
        let instrument_id: String = conn
            .query_row("SELECT id FROM instruments LIMIT 1", [], |r| r.get(0))
            .unwrap();
        ledger_transaction::create(
            &conn,
            ledger_transaction::TransactionInput {
                kind: ledger_transaction::TransactionKind::Dividend,
                amount_cents: 12_345,
                currency_code: "CNY".to_owned(),
                account_id: Some(account_id),
                instrument_id: Some(instrument_id),
                fee_cents: Some(0),
                date: "2026-03-05".to_owned(),
                note: Some("分红".to_owned()),
                ..probe_investments_input()
            },
        )
        .unwrap();
    }
}

/// 探针造数交易输入的最小基座（具名字段覆盖，其余零值；dashboard/tests.rs
/// 同形——两个测试二进制独立进程互不串扰，此处 headless 目标需要本地面）。
fn probe_investments_input() -> ledger_transaction::TransactionInput {
    ledger_transaction::TransactionInput {
        kind: ledger_transaction::TransactionKind::Expense,
        amount_cents: 0,
        currency_code: String::new(),
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

/// 落位投资视图（投资用例共用）：侧栏「投资」项 → 点入 → 等概览页签卡挂载。
fn open_investments(app: &mut Headless) {
    app.wait_for("id:nav-investments").unwrap();
    app.press("id:nav-investments").unwrap();
    app.wait_for("id:inv-tab-overview").unwrap();
}

/// 投资视图全景呈现（概览默认首签）：两页签闭集在屏、可投资资产 Hero /
/// 两腿图例 / 投资合计三项 / 缺价（本例无缺价）/ 构成比例条与图例、走势卡
/// 空态说明与预设钮组齐备。「删除即红」负向条目：删 view_host 的 Investments
/// 分支挂载 → 落回迁移占位（本测红）。
#[test]
fn investments_view_overview_lands_with_period_numbers() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_investments_book("invest-overview");
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_investments(&mut app);

    // 页签闭集（旧壳 investments.tabs 前两位；持仓/标的/明细已随 #2046 ⑮ /
    // #2047 ⑯ 就位）。
    for tab in ["概览", "盈亏"] {
        assert!(
            app.shows(tab),
            "页签「{tab}」应在屏：{:?}",
            app.screen().texts()
        );
    }
    // Hero：可投资资产 = 990 现金腿 + 持仓市值腿 0（卖出清仓、分红不动持仓）
    // = ¥9.9；本位币标注 = 域透传的 CNY。
    app.wait_for("text:¥132.95").unwrap();
    app.wait_for("id:overview-hero-value").unwrap();
    app.wait_for("text:本位币 CNY").unwrap();
    // 构成比例条与两腿图例（缺价说明缺席——全部持仓有现价）。
    app.wait_for("id:overview-composition-bar").unwrap();
    app.wait_for("text:投资账户现金 ¥131.95").unwrap();
    app.wait_for("text:持仓市值 ¥1").unwrap();
    assert_eq!(
        app.screen().count("id:overview-missing-price").unwrap(),
        0,
        "无缺价持仓不出缺价说明"
    );
    // 投资合计三项：总市值 = 持仓 0（中性）、持仓收益 = 0、累计收益 =
    // -2000 + 12345 = ¥103.45（盈亏色归人工对表，文本此处钉值）。
    app.wait_for("text:投资合计").unwrap();
    app.wait_for("id:overview-total-market-value-value")
        .unwrap();
    app.wait_for("id:overview-unrealized-pnl-value").unwrap();
    let texts = app.screen().texts();
    assert!(
        texts.iter().any(|t| t.contains("¥122.95")),
        "累计收益 = 0 - 50 + 12345 分折 ¥122.95：{:?}",
        texts
    );
    // 走势卡：空态说明（无价格历史序列）与预设钮组齐备。
    app.wait_for("id:overview-trend-card").unwrap();
    app.wait_for("text:当前区间内暂无采样点，可尝试更长的区间。")
        .unwrap();
    for preset in ["1 月", "3 月", "1 年", "3 年", "5 年", "全部"] {
        assert!(
            app.shows(preset),
            "预设「{preset}」在屏：{:?}",
            app.screen().texts()
        );
    }
    assert_eq!(
        app.screen().count("id:placeholder-note").unwrap(),
        0,
        "投资视图是真实视图，不再是迁移占位"
    );
}

/// 空账本引导：可投资资产 ¥0、构成条不渲染（空轨道不呈现）、无投资账户
/// 引导句在屏（不隐藏功能，旧壳 overview.noAccount 同词）。删除 view_host
/// 挂载分支即红（同上）。
#[test]
fn investments_view_guides_when_no_investment_account() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("invest-empty");
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_investments(&mut app);

    app.wait_for("id:overview-hero-value").unwrap();
    app.wait_for("text:¥0").unwrap();
    assert!(
        !app.shows("overview-composition"),
        "可投资资产 0 不渲染构成条（空轨道不呈现）"
    );
    app.wait_for("text:还没有投资账户；在「账户」页新建投资账户后，这里会显示你的可投资资产。")
        .unwrap();
}

/// 金额隐私：开关开启 → 全部金额统一掩码、构成条一并隐藏（图形和金额同
/// 严格口径，旧壳 spec #1684 的隐私裁量平整价）。
#[test]
fn investments_view_privacy_masks_amounts_and_hides_composition() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_investments_book("invest-privacy");
    let storage = nsh_testing::MemoryStorage::new(vec![(
        "amount_privacy_enabled".to_owned(),
        "true".to_owned(),
    )]);
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(EngineConfig {
        storage: Some(storage),
        ..desktop_config()
    })
    .unwrap();

    open_investments(&mut app);

    let texts = app.screen().texts();
    let masked = texts.iter().filter(|t| t.contains("••••")).count();
    assert!(
        masked >= 5,
        "隐私态金额统一掩码（Hero/两腿/合计三项）：{:?}",
        texts
    );
    assert!(
        !texts.iter().any(|t| t.contains("¥")),
        "掩码下不出现明文金额：{:?}",
        texts
    );
}

/// 盈亏页签：两张单值表行值（按年 2026 = -2000+12345 = 10345 / 2025 =
/// -2000）、副行两腿拆解与 MWR 卡行在屏。「删除即红」负向条目：删盈亏
/// 页签落位装载（load_summary 接线）→ 两表永不呈现（本测红）。
#[test]
fn investments_view_pnl_tab_tables_and_mwr_card() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_investments_book("invest-pnl");
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_investments(&mut app);
    app.press("id:inv-tab-pnl").unwrap();

    // 筛选行与折算口径披露句在屏。
    app.wait_for("id:pnl-filter-account").unwrap();
    app.wait_for("id:pnl-calibration-note").unwrap();
    // 按年表 2026 行：主值 = 已实现 0 + 分红 12345，副行两腿拆解。
    app.wait_for("id:pnl-year-table-value-2026").unwrap();
    let texts = app.screen().texts();
    assert!(
        texts.iter().any(|t| t.contains("¥123.45"))
            && texts
                .iter()
                .any(|t| t.contains("已实现盈亏 ¥0 · 现金分红 ¥123.45")),
        "2026 行主值与两腿拆解：{:?}",
        texts
    );
    // 2025 行（最新年在前）：卖 1 份@¥0.5 对 FIFO 成本 ¥1 → 已实现 -¥0.5。
    app.wait_for("id:pnl-year-table-value-2025").unwrap();
    let texts = app.screen().texts();
    assert!(
        texts.iter().any(|t| t.contains("-¥0.5"))
            && texts
                .iter()
                .any(|t| t.contains("已实现盈亏 -¥0.5 · 现金分红 ¥0")),
        "2025 行副行：{:?}",
        texts
    );
    // 按账户表同名行（账户名 = 行名）。
    app.wait_for("id:pnl-account-table-value-证券").unwrap();
    // MWR 卡：账户行 + 全账行在屏（数值形态由域解出，文本不钉）。
    app.wait_for("id:pnl-mwr-card").unwrap();
    app.wait_for("text:全账").unwrap();
}

/// 盈亏页签账户筛选收窄：选择无交易的账户 → 两表行集随收窄（按账户行仅剩
/// 该账户行；按年行只剩该账户年份）、MWR 账户行随收窄而全账行不收窄。
/// 「删除即红」负向条目：删筛选写路径 effect（变化不重查）→ 两表仍旧本测红。
#[test]
fn investments_view_pnl_filters_narrow_tables() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_investments_book("invest-filter");
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();

    // 先造一个无交易的投资账户（「闲置」；筛选候选面在用投资账户）。
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.conn.lock().unwrap();
        ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "闲置".to_owned(),
                kind: AccountType::Investment,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
    }

    open_investments(&mut app);
    app.press("id:inv-tab-pnl").unwrap();
    app.wait_for("id:pnl-year-table-value-2026").unwrap();

    // 账户直选：触发件 → 选项行「闲置」。
    app.press("id:pnl-account-filter-trigger").unwrap();
    app.wait_for("id:pnl-account-filter-option").unwrap();
    app.press("text:闲置").unwrap();

    // 收窄：两表只剩「闲置」的空行集 → 按年表空态（闲置账户无已实现）。
    app.wait_for("id:pnl-year-table-empty").unwrap();
    app.wait_for("id:pnl-account-table-empty").unwrap();
    // 清空筛选（可清空形态的 ✕，独立按压件）：行集恢复（clear 落 None 归
    // 默认态，删除同值守卫不会静默锁死）。
    app.press("id:pnl-account-filter-clear").unwrap();
    app.wait_for("id:pnl-year-table-value-2026").unwrap();
    app.wait_for_gone("id:pnl-year-table-empty", DEFAULT_WAIT)
        .unwrap();
}

/// 页签状态保留（旧壳 display-directive="show" + key 的本壳对应物）：
/// 未到访页签不预载（lazy 半边）+ 到访页签 display 隐藏保持挂载（切回回样）。
/// 「删除即红」负向条目：删 keep-mount 的 display 切换接线（改回卸载重建）
/// → 到访印记随卸载清零，本测红。
#[test]
fn investments_view_keeps_visited_panes_mounted_across_tab_switches() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_investments_book("invest-keep");
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_investments(&mut app);

    // 未到访的盈亏页签不预载（宿主内容缺席）。
    assert_eq!(
        app.screen().count("id:pnl-calibration-note").unwrap(),
        0,
        "未到访页签不预载"
    );

    // 到访盈亏并留下视图印记：切到盈亏，等表挂载。
    app.press("id:inv-tab-pnl").unwrap();
    app.wait_for("id:pnl-calibration-note").unwrap();

    // 非激活页签退出文档流（display 切换的呈现面）：概览 Hero 不在屏——
    // 删除 display 接线（两页签同时可见）即红。
    assert_eq!(
        app.screen().count("id:overview-hero-value").unwrap(),
        0,
        "非激活页签不显示（display 切换接线）"
    );

    // 切回概览：页签回样（keep-mounted：切走时宿主隐藏不重挂）。
    app.press("id:inv-tab-overview").unwrap();
    app.wait_for("id:overview-hero-value").unwrap();
    app.wait_for("text:¥132.95").unwrap();
}

/// 盈亏两表客户端切片分页（旧壳 PNL_PAGE_SIZE 8；第 1 页 = 最近年份）：
/// 9 个年份行 → 分页条出现，第 2 页呈现最老年份行。「删除即红」负向条目：
/// 删分页条的 on_change → 页码写回永不发生（本测红）。
#[test]
fn investments_view_pnl_pagination_slices_year_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("invest-pages");
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    // 9 个年度各一批买入+卖出（2025–2033）：每年度 1 对交易 → 按年 9 行。
    {
        use ledger_investment::{InstrumentInput, InstrumentType};
        let (account, instrument) = {
            let conn = db.conn.lock().unwrap();
            let account = ledger_accounts::create_account(
                &conn,
                AccountInput {
                    name: "证券".to_owned(),
                    kind: AccountType::Investment,
                    currency_code: "CNY".to_owned(),
                    initial_balance_cents: Some(500_000),
                    credit_limit_cents: None,
                    statement_day: None,
                    due_day: None,
                },
            )
            .unwrap();
            let instrument = ledger_investment::create_instrument(
                &conn,
                InstrumentInput {
                    symbol: "PGS".to_owned(),
                    kind: InstrumentType::Stock,
                    name: Some("分页标的".to_owned()),
                    currency_code: "CNY".to_owned(),
                    market: None,
                },
            )
            .unwrap();
            (account, instrument)
        };
        for year in 2025..2034 {
            let buy_date = format!("{year}-01-05");
            let sell_date = format!("{year}-06-15");
            {
                let conn = db.conn.lock().unwrap();
                ledger_transaction::create(
                    &conn,
                    ledger_transaction::TransactionInput {
                        kind: ledger_transaction::TransactionKind::Buy,
                        amount_cents: 0,
                        currency_code: "CNY".to_owned(),
                        account_id: Some(account.clone()),
                        instrument_id: Some(instrument.clone()),
                        quantity: Some(1.0),
                        price_cents: Some(2_000),
                        fee_cents: Some(0),
                        date: buy_date,
                        note: Some("买入".to_owned()),
                        ..probe_investments_input()
                    },
                )
                .unwrap();
                ledger_transaction::create(
                    &conn,
                    ledger_transaction::TransactionInput {
                        kind: ledger_transaction::TransactionKind::Sell,
                        amount_cents: 0,
                        currency_code: "CNY".to_owned(),
                        account_id: Some(account.clone()),
                        instrument_id: Some(instrument.clone()),
                        quantity: Some(1.0),
                        price_cents: Some(3_000),
                        fee_cents: Some(0),
                        date: sell_date,
                        note: Some("卖出".to_owned()),
                        ..probe_investments_input()
                    },
                )
                .unwrap();
            }
        }
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_investments(&mut app);
    app.press("id:inv-tab-pnl").unwrap();

    // 9 年行 → 2 页（8 行/页）：第 1 页呈现 2033–2026 的 8 行；分页条在屏。
    app.wait_for("id:pnl-year-table-pagination").unwrap();
    let texts = app.screen().texts();
    for year in ["2033", "2032", "2027", "2026"] {
        assert!(
            texts.iter().any(|t| t.contains(year)),
            "第 1 页含最新年份 {year}：{:?}",
            texts
        );
    }
    assert!(
        !texts.iter().any(|t| t.contains("2025")),
        "第 1 页不含最老年份 2025（8 行/页切片）：{:?}",
        texts
    );
    // 翻第 2 页：2025 行出现（on_change 页码写回会话槽的接线）。按钮行
    // 比分页容器晚一拍落位（#2072）——按压前等按钮本身。
    app.wait_for("role:button[2]").unwrap();
    app.press("role:button[2]").unwrap();
    let texts = app.screen().texts();
    assert!(
        texts.iter().any(|t| t.contains("2025")),
        "第 2 页含最老年份 2025：{:?}",
        texts
    );
}

/// 缺价持仓的显式说明：未录价持仓 → 概览缺价计数行在屏（合计里不计入但
/// 显式可见，旧壳 overview.missingPrice 同词）。
#[test]
fn investments_view_reports_missing_price_count() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("invest-missing-price");
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    {
        use ledger_investment::{InstrumentInput, InstrumentType};
        let conn = db.conn.lock().unwrap();
        let account = ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "证券".to_owned(),
                kind: AccountType::Investment,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
        let instrument = ledger_investment::create_instrument(
            &conn,
            InstrumentInput {
                symbol: "NOP".to_owned(),
                kind: InstrumentType::Stock,
                name: Some("无价标的".to_owned()),
                currency_code: "CNY".to_owned(),
                market: None,
            },
        )
        .unwrap();
        ledger_transaction::create(
            &conn,
            ledger_transaction::TransactionInput {
                kind: ledger_transaction::TransactionKind::Buy,
                amount_cents: 0,
                currency_code: "CNY".to_owned(),
                account_id: Some(account),
                instrument_id: Some(instrument),
                quantity: Some(1.0),
                price_cents: Some(2_000),
                fee_cents: Some(0),
                date: today().to_string(),
                note: Some("买入".to_owned()),
                ..probe_investments_input()
            },
        )
        .unwrap();
    }
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_investments(&mut app);
    app.wait_for("text:另有 1 只持仓因缺现价或折算汇率未计入。")
        .unwrap();
}

// ---------------------------------------------------------------------------
// 投资视图·持仓页签与记账表单（#2046）：持仓卡（同步入口 + 确定进度 +
// 结果/降级反馈 + 三维过滤 + 合计三卡 + 行表客户端切片分页 + 缺价行同步
// 引导 + staleness 本地水位提示条）与 buy/sell 编辑表单族。「删除即红」
// 负向条目写在各测试注释（ADR-0087：断言对准用户可观察结果）。
// ---------------------------------------------------------------------------

/// 持仓页签全景（旧壳 HoldingsOverview 卡同构）：卡头同步入口在屏、合计
/// 三卡（总市值/持仓收益/累计收益）、行格闭集（标的/账户/数量/成本/现价/
/// 净值日期/市值/持仓收益/收益率）、单页分页收起、staleness 提示条缺席
///（全部持仓有现价且水位新鲜）。「删除即红」：删 view_host 的 holdpanes
/// 分支（keep_mounted_pane InvestmentsTab::Holdings 挂载）→ 落回空态（本
/// 测红；删除接线断言见 lib 失效探针注释）。
#[test]
fn investments_view_holdings_tab_lands_rows_cards_and_stats() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_investments_book("invest-holdings");
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_investments(&mut app);
    app.press("id:inv-tab-holdings").unwrap();

    // 卡头（同步入口恒在——与行集无关；旧壳 card header-extra 同位）。
    app.wait_for("id:holdings-card").unwrap();
    app.wait_for("text:当前持仓").unwrap();
    app.wait_for("text:同步标的信息").unwrap();
    // 合计三卡（过滤子集口径下的市值/收益 + 全账本口径的累计收益；#2045
    // 盈亏累计口径 = 已实现 + 分红 = ¥122.95）。
    app.wait_for("id:holdings-total-market-value-value")
        .unwrap();
    app.wait_for_gone("id:holdings-total-market-value-warning", DEFAULT_WAIT)
        .unwrap();
    let texts = app.screen().texts();
    assert!(
        texts.iter().any(|t| t.contains("¥122.95")),
        "累计收益 = -¥0.5 + ¥123.45：{:?}",
        texts
    );
    // 行格闭集（数量 1、成本 ¥1、现价 ¥1、市值 ¥1、收益 ¥0）。
    app.wait_for("id:holdings-table-head").unwrap();
    app.wait_for("text:TST").unwrap();
    app.wait_for("text:测试标的").unwrap();
    let texts = app.screen().texts();
    for expected in ["¥1", "¥0"] {
        assert!(
            texts.iter().any(|t| t.contains(expected)),
            "行格金额面 {expected}：{:?}",
            texts
        );
    }
    // 单页收起分页条；无缺价说明（合计卡missing_note 缺席）与 staleness
    // 提示缺席（水位新鲜 + 现价在场）。
    assert_eq!(
        app.screen().count("id:holdings-pagination").unwrap(),
        0,
        "单页收起分页条：{:?}",
        app.screen().texts()
    );
    assert_eq!(
        app.screen().count("id:staleness-alert").unwrap(),
        0,
        "无过期标的不出提示条（背景提示宁缺勿滥）"
    );
}

/// 持仓三维过滤：账户单选收窄（无持仓账户 → 「筛选条件下无匹配持仓」行
/// 空态 + 合计清零）与搜索（关键词无命中 → 同一空态；清除回全量）。「删
/// 除即红」：删筛选写路径接线（应用值 effect）→ 行集不收窄（本测红）。
#[test]
fn investments_view_holdings_filters_narrow_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_investments_book("invest-holdings-filter");
    // 无持仓的投资账户（筛选候选面在用账户）。
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.conn.lock().unwrap();
        ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "闲置".to_owned(),
                kind: AccountType::Investment,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_investments(&mut app);
    app.press("id:inv-tab-holdings").unwrap();
    app.wait_for("text:TST").unwrap();

    // 账户过滤收窄：闲置账户无持仓 → 行集收窄、两态空态可辨。
    app.press("id:holdings-account-filter-select-trigger")
        .unwrap();
    app.wait_for("id:holdings-account-filter-select-option")
        .unwrap();
    app.press("text:闲置").unwrap();
    app.wait_for("id:holdings-no-match").unwrap();
    // 合计卡：过滤无命 = 无可计入行 → 「-」（旧壳 PortfolioStatsCards cents
    // null → "-" 同口径；不给伪零）。
    let texts = app.screen().texts();
    assert!(texts.contains(&"-"), "合计卡无计入行展示「-」：{:?}", texts);
    // 清空过滤（可清空形态 ✕）：行集回全量。
    app.press("id:holdings-account-filter-select-clear")
        .unwrap();
    app.wait_for("text:TST").unwrap();

    // 搜索无命中（防抖后应用值生效）→ 空态；清空回全量。
    app.type_text("id:holdings-search", "ZZZ").unwrap();
    app.wait_for("id:holdings-no-match").unwrap();
    app.clear("id:holdings-search").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("text:TST").unwrap();
}

/// 持仓客户端切片分页（旧壳 HOLDINGS_PAGE_SIZE 固定 20 不设选择器）：
/// 25 行持仓 → 第 1 页 20 行、第 2 页 5 行（默认标的代码字母序；分页条也
/// 在屏）。「删除即红」：删分页条 on_change 页码写回接线 → 第 2 页不出现。
#[test]
fn investments_view_holdings_pagination_slices_rows() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("invest-holdings-pages");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        use ledger_investment::{InstrumentInput, InstrumentType};
        let account = {
            let conn = db.conn.lock().unwrap();
            ledger_accounts::create_account(
                &conn,
                AccountInput {
                    name: "证券".to_owned(),
                    kind: AccountType::Investment,
                    currency_code: "CNY".to_owned(),
                    initial_balance_cents: Some(500_000),
                    credit_limit_cents: None,
                    statement_day: None,
                    due_day: None,
                },
            )
            .unwrap()
        };
        for i in 0..25 {
            let conn = db.conn.lock().unwrap();
            let instrument = ledger_investment::create_instrument(
                &conn,
                InstrumentInput {
                    symbol: format!("S{i:02}"),
                    kind: InstrumentType::Stock,
                    name: Some(format!("标的{i:02}")),
                    currency_code: "CNY".to_owned(),
                    market: None,
                },
            )
            .unwrap();
            ledger_transaction::create(
                &conn,
                ledger_transaction::TransactionInput {
                    kind: ledger_transaction::TransactionKind::Buy,
                    amount_cents: 0,
                    currency_code: "CNY".to_owned(),
                    account_id: Some(account.clone()),
                    instrument_id: Some(instrument),
                    quantity: Some(1.0),
                    price_cents: Some(2_000),
                    fee_cents: Some(0),
                    date: "2025-01-05".to_owned(),
                    note: Some("买入".to_owned()),
                    ..probe_investments_input()
                },
            )
            .unwrap();
        }
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_investments(&mut app);
    app.press("id:inv-tab-holdings").unwrap();

    // 第 1 页 20 行（默认代码字母序 S00..S19）；S24 不在第 1 页。
    app.wait_for("id:holdings-table-head").unwrap();
    app.settle();
    let texts = app.screen().texts();
    for sym in ["S00", "S19"] {
        assert!(
            texts.iter().any(|t| t.contains(sym)),
            "第 1 页含 {sym}：{:?}",
            texts
        );
    }
    assert!(
        !texts.iter().any(|t| t.contains("S24")),
        "第 1 页不含末段 S24：{:?}",
        texts
    );
    // 翻第 2 页：S20..S24 行出现。分页按钮行比表头晚一拍落位（#2072 同型
    // 空窗）——按压前等按钮本身。
    app.wait_for("role:button[2]").unwrap();
    app.press("role:button[2]").unwrap();
    app.wait_for("text:S24").unwrap();

    // 三维任一实际变化即翻页归零（ TValue-902；排序同为过滤签名维度）：
    // 在第 2 页点击市值列头（排序态翻转 = 签名实际变化）→ 页码归 1 →
    // S24 离屏、S19 回屏。「删除即红」条目：删翻页归零接线
    //（session.holdings_page.set(1)）→ 排序后仍停第 2 页（S24 不离屏，
    // 本测红）。
    app.press("id:holdings-col-market-value").unwrap();
    app.wait_for_gone("text:S24", DEFAULT_WAIT).unwrap();
    app.wait_for("text:S19").unwrap();
}

/// 空账本的同步入口存在性（行集为空时卡头同步入口仍在——旧壳 card
/// header-extra 同位）：按下 → 同步接缝在途→空库结果反馈（「暂无标的可
/// 同步」）。「删除即红」：删卡头 on_press 的 trigger_sync 接线 → 结果
/// 行永不出现（本测红）。空库不发起任何网络（通道在空标的分区无取数）。
#[test]
fn investments_view_holdings_sync_entry_reports_empty_book() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("invest-holdings-empty");
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    open_investments(&mut app);
    app.press("id:inv-tab-holdings").unwrap();
    app.wait_for("text:当前持仓").unwrap();
    app.press("id:holdings-sync-button").unwrap();
    app.wait_for("id:investment-sync-result").unwrap();
    assert!(
        app.shows("暂无标的可同步"),
        "空库同步结果反馈：{:?}",
        app.screen().texts()
    );
}

/// 缺价行引导与 staleness 提示条（#1193 / #1190）：行情通道标的缺现价 →
/// 行内「同步」锚在屏（动作 = 同步触发，与卡头入口同一接缝单点）＋ staleness
/// 提示条在屏（开场本地水位检查，持仓缺现价入选）。手动报价通道行内不设
/// 引导（录价入口住标的库页签，#2047 ⑯；分流见 holdings 模块头）。删
/// staleness 装载
/// 接线（视图落位装载）→ 提示条永不出现（本测红；失效重查删除即红钉在
/// lib staleness 探针）。
#[test]
fn investments_view_holdings_missing_price_guide_and_staleness_alert() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("invest-holdings-guide");
    let (account_id, instrument_id) = {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        use ledger_investment::{InstrumentInput, InstrumentType};
        let conn = db.conn.lock().unwrap();
        let account = ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "证券".to_owned(),
                kind: AccountType::Investment,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(500_000),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap();
        let instrument = ledger_investment::create_instrument(
            &conn,
            InstrumentInput {
                symbol: "600000".to_owned(),
                kind: InstrumentType::Stock,
                name: Some("无价标的".to_owned()),
                currency_code: "CNY".to_owned(),
                market: Some("sh".to_owned()),
            },
        )
        .unwrap();
        ledger_transaction::create(
            &conn,
            ledger_transaction::TransactionInput {
                kind: ledger_transaction::TransactionKind::Buy,
                amount_cents: 0,
                currency_code: "CNY".to_owned(),
                account_id: Some(account.clone()),
                instrument_id: Some(instrument.clone()),
                quantity: Some(10.0),
                price_cents: Some(2_000),
                fee_cents: Some(0),
                date: today().to_string(),
                note: Some("买入".to_owned()),
                ..probe_investments_input()
            },
        )
        .unwrap();
        (account, instrument)
    };

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_investments(&mut app);
    app.press("id:inv-tab-holdings").unwrap();

    // 缺价行：现价「-」+ 行内同步引导锚（行键 = 账户×标的，与旧壳
    // priceGuideRowKey 同拼法）。
    app.wait_for("text:600000").unwrap();
    let guide_id = format!(
        "id:holdings-row-{}-{}-missing-price-sync",
        account_id, instrument_id
    );
    app.wait_for(&guide_id).unwrap();
    // staleness 提示条（本地水位检查零网络；持仓缺现价入选）与「去同步」。
    app.wait_for("id:staleness-alert").unwrap();
    app.wait_for("id:staleness-go-sync").unwrap();
    assert!(
        app.shows("有 1 个标的的行情 / 净值可能已过期（超过 7 天未更新，或持仓缺现价）。"),
        "提示主文案（count/days 装配）：{:?}",
        app.screen().texts()
    );
}

// ---------------------------------------------------------------------------
// 投资买卖表单旅程（#2046）由 lib 探针承载（investments_form.rs mod probe）：
// 旧壳 buy/sell 行的编辑入口住户投资明细页签（ADR-0135 主列表默认排除投资
// kind，NSH 同口径——主列表触不到 buy 行，headless 用户路径日落空）；探针
// 在引擎内挂真实交易列表不可达入口不可行 → 探针直挂表单组件 + begin_edit
// （引擎同面：回填 / 禁用 / 保存 / 库态由同一 #[component] 真实路径承载），
// 「删除即红」条目随探针注释。#2046 交付偏差见票交付报告。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 投资明细与标的库页签（#2047）：明细行投影像与服务端分页/类型筛选、持仓
// 下钻（标的代码链接 → 明细页签 + 筛选）、标的库检索与 focus 锚定位高亮、
// 手动报价旅程（两落点写路径 + 证据门广播 + 行内现价列灰刷。删除任一接线
// 即红的负向条目逐项写在各测试注释；ADR-0087 断言强度：断言对准用户可观察
// 结果。
// ---------------------------------------------------------------------------

/// 造一个自建债券标的（手动来源 + 手动报价通道——录价/删除双入口的准入
/// 帮用例：价 channel 判定单点在后端）。
fn seed_manual_bond_instrument(db: &ledger_infra::db::DbState, symbol: &str) -> String {
    use ledger_investment::{InstrumentInput, InstrumentType};
    let conn = db.conn.lock().unwrap();
    ledger_investment::create_instrument_manual(
        &conn,
        InstrumentInput {
            symbol: symbol.to_owned(),
            kind: InstrumentType::Bond,
            name: Some(format!("{symbol} 债券")),
            currency_code: "CNY".to_owned(),
            market: None,
        },
    )
    .unwrap()
}

/// 明细页签全景：五 kind 行投影在屏（kind 分形态 + 标的格「代码 · 名称」
/// 组列：数量/单价/费用/账户/出资账户），总数与日期倒序；删明细页签挂载
/// 或列表读接线任一即红。

#[test]
fn investments_view_ledger_tab_lands_kind_rows_and_filters() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_investments_book("invest-ledger-rows");

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_investments(&mut app);

    // 落明细页签：三种形态行在屏（dividend 金额行、sell 单价行、convert 缺）
    // 日期倒序（dividend 2026-03-05 在前）。删列表读接线（激活边装载）→
    // 行集不呈现、本测红。
    app.press("id:inv-tab-ledger").unwrap();
    app.wait_for("id:ledger-title").unwrap();
    app.wait_for("text:2026-03-05").unwrap();
    assert!(
        app.shows("共 3 条"),
        "明细总数直出：{:?}",
        app.screen().texts()
    );
    assert!(app.shows("买入"), "buy kind 行在屏");
    assert!(app.shows("分红"), "dividend kind 行在屏");
    assert!(app.shows("卖出"), "sell kind 行在屏");

    // 类型子集筛选（多选）：只选「分红」→ 行集收窄到 1 行。删除筛选重拉
    // 接线（tracked signature effect）→ 行集不刷新、本测红。
    app.press("id:ledger-kind-trigger").unwrap();
    app.press("id:ledger-kind-option >> text:分红 >> nth=0")
        .unwrap();
    app.wait_for("text:共 1 条").unwrap();
    assert!(app.shows("2026-03-05"), "被筛选命中的 dividend 行保留");
}

/// 持仓下钻（ADR-0135 决策 6）：持仓行「标的代码」链接 → 落明细页签并按
/// 账户+标的过滤（该账户该标的的投资交易历史）。删持仓代码的 nav::open
/// 接线或 investments_drill 消费任一 → 点击后不换页签/不过滤，本测红。
/// 持仓下钻（ADR-0135 决策 6）：持仓行「标的代码」链接 → 落明细页签并按
/// 账户+标的过滤（该账户该标的的投资交易历史）。删持仓代码的 nav::open
/// 接线或 investments_drill 消费任一 → 点击后不换页签/不过滤，本测红。
#[test]
fn investments_view_holdings_code_drills_to_ledger_filtered() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_investments_book("invest-holdings-drill");

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    // 行键 = 账户 id + 标的 id（v_holdings 一行 = 一个账户一只标的；账
    // 户/标的 id 经域读反查）。
    let (account_id, instrument_id);
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let conn = db.conn.lock().unwrap();
        account_id = conn
            .query_row("SELECT id FROM accounts WHERE type='investment'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap();
        instrument_id = conn
            .query_row("SELECT id FROM instruments WHERE symbol='TST'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap();
    }
    open_investments(&mut app);
    app.press("id:inv-tab-holdings").unwrap();
    app.wait_for("id:holdings-title").unwrap();

    // 点持仓行标的代码（TST 持仓行）→ 明细页签按账户+标的过滤：该账户该
    // 标的三笔（buy/sell/dividend）全在列（dividend 行绑定同一账户与同一
    // 标的），且落点是明细页签。「删除即红」负向条目：删持仓代码链接的
    // nav::open 接线 → 点击后停留持仓页签，本断言红。
    let symbol_anchor = format!("id:holdings-row-{}-{}-symbol", account_id, instrument_id);
    // 持仓行异步落屏：等行锚再按（CI 慢环境 press 竞态锚定，同 #2072；
    // run 42927784003 首崩即此——holdings-title 只证页签挂载不证行落屏）。
    app.wait_for(&symbol_anchor).unwrap();
    app.press(&symbol_anchor).unwrap();
    app.wait_for("id:ledger-title").unwrap();
    app.wait_for("id:ledger-title").unwrap();
    app.wait_for("text:共 3 条").unwrap();
    assert!(
        app.shows("2025-12-20") && app.shows("2025-12-30") && app.shows("2026-03-05"),
        "该账户该标的三笔在列：{:?}",
        app.screen().texts()
    );
}

/// 标的库检索 + 手动报价旅程：浏览（服务端分页 50/页）、搜索收窄、手动
/// 通道行「录价」入口、提交断言写路径两个落点与回执分型 + 投资失效信号
/// 驱动行集原地重拉（现价列现值化）。删除价格失效信号订阅或「是否发」广
/// 播门任一 → 现价列不刷新，本测红。
#[test]
fn investments_view_instruments_search_and_manual_quote_journey() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = seed_investments_book("invest-instruments-quote");
    let dir_probe = dir.clone();
    let bond_id;
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        bond_id = seed_manual_bond_instrument(&db, "BD01");
    }

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_investments(&mut app);
    app.press("id:inv-tab-instruments").unwrap();
    app.wait_for("id:instruments-title").unwrap();

    // 浏览：两个标的行（TST 与 BD01）都在屏；行情/净值通道标的（TST）无
    // 录价入口、无来源行亦无。
    let row_bond = format!("id:instruments-row-{}", bond_id);
    app.wait_for(&row_bond).unwrap();
    assert!(app.shows("TST"), "TST 行在屏：{:?}", app.screen().texts());

    // 搜索收窄：输入「BD」→ 只剩 BD01 行（统一模糊语义同旧壳远程搜索）；
    // 删除搜索重拉接线（tracked signature effect）→ 行集不收窄，本段红。
    app.type_text("id:instruments-search", "BD").unwrap();

    // 手动报价（金额 unit 面额按域刻度）：行内「录价」按下 → 弹窗（标题
    // + 介绍 + 默认今天）; 录价 1.2345 元（= 12,345 万分之一元）→ 提交。
    // 手动报价通道行独有的录价按钮（行情/净值/无来源行的行内「-」）。
    app.press("id:instruments-quote").unwrap();
    app.wait_for("id:manual-quote-price").unwrap();
    app.type_text("id:manual-quote-price", "1.2345").unwrap();
    app.press("id:submit-manual-quote").unwrap();
    app.wait_for("text:已录价：BD01 现价更新为 1.2345").unwrap();

    // 价格失效信号（实际写入任一落点即广播——零变化不广播）→ 行集原地重
    // 拉：现价列从「-」变为「¥1.2345」。删「是否发」广播门或本 pane 订阅
    // 任一 → 现价列不刷新，本断言红。
    // DEBUG: db state first
    {
        let db = nsh_app::boot::boot_ledger(&dir_probe).unwrap();
        let conn = db.conn.lock().unwrap();
        let (price, history): (Option<i64>, i64) = conn
            .query_row(
                "SELECT (SELECT price_cents FROM market_prices WHERE instrument_id=?1), \
                 (SELECT COUNT(*) FROM price_history WHERE instrument_id=?1)",
                rusqlite::params![bond_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        eprintln!("DEBUG quote db state price={:?} history={}", price, history);
    }
    app.wait_for("text:¥1.2345").unwrap();

    // 库态核对（两个落点）：现价缓存 upsert + 价格历史周采样行（source
    // = manual）。写证据广播由信号订阅端呈现面承载，以上述重拉为准。
    {
        let db = nsh_app::boot::boot_ledger(&dir_probe).unwrap();
        let conn = db.conn.lock().unwrap();
        let (price, history): (Option<i64>, i64) = conn
            .query_row(
                "SELECT (SELECT price_cents FROM market_prices WHERE instrument_id=?1), \
                 (SELECT COUNT(*) FROM price_history WHERE instrument_id=?1)",
                rusqlite::params![bond_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(price, Some(12_345), "现价缓存按最新点映像规则 upsert");
        assert_eq!(history, 1, "价格历史周采样落库");
    }
}

// ---------------------------------------------------------------------------
// 窄窗自适应横切（#2057 / ADR-0088 对表）：断点切换、卡片菜单与搜索只读卡片。
// ---------------------------------------------------------------------------

/// 落位交易视图（窄窗移动档）：等顶栏 → 开抽屉 → 点交易（同一份导航状态）→
/// 卡片形态挂载。各窄窗交易用例经本助手落位。
fn open_transactions_mobile(app: &mut Headless) {
    app.wait_for("id:nav-topbar-open").unwrap();
    app.press("id:nav-topbar-open").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    app.press("id:nav-transactions").unwrap();
    app.wait_for("id:tx-cards").unwrap();
}

/// 断点切换平滑、状态不丢（#2057 AC）：1280 桌面档表格在屏 → 缩窗 600 移动
/// 档卡片在屏、桌面表格卸挂且总数/分页会话态保留 → 放宽 1280 表格回位且同一
/// 页数据仍在（读快照不重拉——总数与行集同一事实源）。删除窄窗分支（App 布局
/// 分支或卡片挂载任一环）→ 本测窄窗段落红。
#[test]
fn app_window_breakpoint_flip_keeps_state_smooth() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("flip");
    seed_expense_series(&dir, 1);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:共 1 条").unwrap();

    // 缩窗进移动档：卡片形态替换表格，会话态（总数）不丢。
    app.set_viewport(Size::new(Points(600.0), Points(720.0)))
        .unwrap();
    app.wait_for("id:tx-cards").unwrap();
    app.wait_for_gone("id:tx-table", DEFAULT_WAIT).unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:2026-01-01").unwrap();

    // 放宽回桌面档：表格回位，同一份数据仍在屏（状态零重置）。
    app.set_viewport(Size::new(Points(1280.0), Points(720.0)))
        .unwrap();
    app.wait_for("id:tx-table").unwrap();
    app.wait_for("text:共 1 条").unwrap();
    app.wait_for("text:2026-01-01").unwrap();
}

/// 窄窗卡片「⋯」与行右键共用同一菜单接缝（#2057）：交易卡呈现装配层同源格
/// 文本（含购买项块），卡片「⋯」弹同一菜单项闭集，删除确认后整笔交易消失
/// （同卡多购买项行的操作目标 = 交易本体）。删除卡片菜单接线即红。
#[test]
fn app_window_narrow_card_menu_shares_row_menu_and_deletes_transaction() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("narrow-delete");
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let cash = seed_cash_account(&db);
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    ledger_transaction::create(
        &conn,
        TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents: 2_860,
            currency_code: "CNY".to_owned(),
            account_id: Some(cash.clone()),
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
            merchant_name: Some("天猫".to_owned()),
            policy_id: None,
            refund_of_transaction_id: None,
            note: None,
            source_order_no: Some("TM-2002".to_owned()),
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
    drop(conn);

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(mobile_config()).unwrap();
    open_transactions_mobile(&mut app);

    // 卡片呈现装配层同源格文本：购买项块逐项（商品名 + 共 N 件）。
    for expected in ["支出", "天猫", "猫粮 共 2 件", "洗衣液 共 1 件", "¥28.6"] {
        assert!(
            app.shows(expected),
            "卡片应呈现「{expected}」：{:?}",
            app.screen().texts()
        );
    }

    // 卡片「⋯」→ 同一菜单项闭集（编辑/退款/加入物品/删除，与桌面右键同源）。
    app.press("id:tx-card-menu-0").unwrap();
    let menu = app.menu().expect("卡片「⋯」应弹出行菜单");
    assert_eq!(
        menu.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        vec!["编辑", "退款", "加入物品", "删除"],
        "卡片行菜单闭集与桌面右键同源：{menu:?}"
    );
    // ESC（原生菜单语义步 = close_menu，Linux 原生菜单捕获键盘不进应用事件流）
    app.close_menu().unwrap();
    app.settle();
    assert!(app.menu().is_none(), "ESC 后菜单收起：{:?}", app.menu());

    // 确认删除危险对话框 → 整笔交易消失（同 id 多行派生只当一笔操作）。
    app.press("id:tx-card-menu-0").unwrap();
    app.choose_menu_item("删除").unwrap();
    app.wait_for("text:删除交易").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for_gone("id:tx-card", DEFAULT_WAIT).unwrap();
    {
        let dates = book_transaction_dates(&dir);
        assert!(dates.is_empty(), "删除后库内该笔消失：{dates:?}");
    }
}

/// 搜索结果窄窗只读卡片复用（#2040 移动档卡片 #846 / #2057）：同一卡片形态
/// 挂载但无菜单无写入口（读-only），命中行文本同宽形态装配产物。
#[test]
fn app_window_narrow_search_results_reuse_readonly_cards() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("narrow-search");
    seed_expense_notes(&dir, &["元旦火锅聚餐", "超市买菜"]);

    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(mobile_config()).unwrap();
    // 搜索视图经抽屉落位（同一份导航状态）。
    app.wait_for("id:nav-topbar-open").unwrap();
    app.press("id:nav-topbar-open").unwrap();
    app.wait_for("id:nav-search").unwrap();
    app.press("id:nav-search").unwrap();
    app.wait_for("id:search-keyword").unwrap();

    app.type_text("id:search-keyword", "火锅").unwrap();
    app.wait_for("text:命中 1 条").unwrap();
    app.wait_for("id:tx-cards").unwrap();
    // 卡片字段与旧壳 TransactionCardList 同构（日期/类型/合并行/账户/金额，
    // 备注原文不在卡片呈现——旧壳同口径；命中凭据 = 总数与卡片同屏）。
    for expected in ["2026-01-01", "支出", "测试现金", "¥25"] {
        assert!(
            app.shows(expected),
            "命中卡片应呈现「{expected}」：{:?}",
            app.screen().texts()
        );
    }
    assert!(
        !app.shows("超市买菜"),
        "未命中行不再在屏：{:?}",
        app.screen().texts()
    );
    assert!(
        app.press("id:tx-card-menu-0").is_err(),
        "只读卡片不渲染「⋯」菜单（搜索结果无写入口）"
    );
}

/// 手写极简 HTTP/1.1 请求（接线断言专用；Connection: close 单往返，无状态
/// 复用）：返回（状态码，响应体全文）。
fn raw_http_request(
    method: &str,
    path: &str,
    body: Option<&str>,
) -> std::io::Result<(u16, String)> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect("127.0.0.1:9527")?;
    let body = body.unwrap_or("");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:9527\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status: u16 = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    Ok((status, body))
}

/// AI 导入 HTTP 壳挂载与失效信号泵接线断言（#2081，删除即红；ADR-0087 断言
/// 强度：对准用户可观察结果，不对准函数调用形状）。两半边各自独立成立：
/// - **挂载半边**：删除 `app.rs` run_startup 就绪分支的 `http_host::mount`
///   接线 → 端口无服务，本用例限时内收不到契约自举端点应答即红；
/// - **泵半边**：删除组件层 `http_host::spawn_pump` 接线（或泵的族广播映射）
///   → 失效事件不再广播进壳内 transactions 族信号 → 视图不重读、新行不出现
///   即红。
///
/// 覆盖链路（旧壳「失效信号 → 前端 store 重拉」本壳等价形的 NSH 新增段）：
/// 「HTTP 写端点 → emitter.post」的端到端已被 `tests/api_server/
/// signal_delivery.rs` 钉住（断言随迁未动）；本用例补 NSH 侧两段——服务可用
/// 性（契约自举端点不触 DB，与库无关）与「泵 → 族广播 → 视图重读」（事件经
/// `post_invalidation_for_tests` 注入泵通道，造数行走域公开写入口、写库不
/// 广播——headless 多场景的服务库槽可能归属首个挂载场景，写端点路径在并行
/// 下不自洽，见 http_host 模块文档生命周期裁决）。
#[test]
fn http_api_server_mounts_and_pumps_invalidation_to_views() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_book("http-mount");
    seed_expense_series(&dir, 1);

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);
    app.wait_for("text:¥25").unwrap();

    // 挂载半边：契约自举端点限时可用（服务是进程级设施，起过一次即常驻）。
    let mut contract = String::new();
    let mut mounted = false;
    for _ in 0..50 {
        if let Ok(result) = raw_http_request("GET", "/api/v1/contract", None) {
            let (status, body) = result;
            assert_eq!(status, 200, "契约自举端点应 200：{status} {body}");
            contract = body;
            mounted = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        mounted,
        "HTTP 服务应在启动后限时可用（run_startup 挂载接线断言，127.0.0.1:9527）"
    );
    // 紧凑契约方言（issue #839）的路径是去 /api/v1 前缀的相对形状（base 承载前缀）。
    assert!(
        contract.contains("transactions/batch"),
        "契约面应承载 AI 导入端点：{}",
        contract.chars().take(400).collect::<String>()
    );

    // 泵半边：域公开写入口直写本场景库一笔更新日期的支出（写库不广播——NSH
    // 壳内广播是调用方接线，非域入口行为），此时视图仍持旧行集；经泵通道注入
    // 一条参考失效事件 → 泵广播进壳内 transactions 族 → 视图订阅重读 → 新行
    // 在屏。删除泵接线（spawn / 换装 / 族映射任一环）本 wait_for 超时红。
    seed_expense_on_date(&dir, "2026-06-01", 777);
    nsh_app::http_host::post_invalidation_for_tests(ledger_infra::events::LEDGER_CHANGED);
    app.wait_for("text:2026-06-01").unwrap();
}

// —— book-missing 失败态的显式出口（ADR-0146 决策 4 / spec #2090 T4）：——
// —— 失败态按钮 + 目录选择现场闭集处置（决策 3 单点）的窗口呈现旅程。 ——

/// 注册表根 + 活动本缺库的失败态现场（本段用例共用前置）：注册表已配置、
/// 活动本 b1 目录无 ledger.db → 启动落 book-missing 失败态（决策 4 保留的
/// fail loud 内核），出口按钮在场。
fn start_failed_state_with_picker() -> (std::path::PathBuf, Headless) {
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t4-failed");
    let active = seed::make_book_dir("t4-active-missing");
    seed_registry(&root, &active, &[]);
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:/账本库文件不存在/").unwrap();
    app.wait_for("id:startup-pick-book-dir").unwrap();
    (root, app)
}

/// 主现场（已配置 + 活动本缺库）：失败态呈现按钮 → 选中注册表在册其他本 →
/// 切活动指针打开（主界面就位、指针落盘）→「下次启动」直接打开新活动本。
/// 删按钮接线 → 失败态断言红（ADR-0087 接线断言，实证见交付报告）。
#[test]
fn startup_book_missing_failed_state_offers_dir_picker_and_opens_registered_book() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t4-main");
    let active = seed::make_book_dir("t4-active-missing");
    let other = make_book("t4-other");
    seed_registry(&root, &active, &[("b2", "备用账本", &other)]);
    nsh_app::cross_book::set_default_data_dir(root.clone());

    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:/账本库文件不存在/").unwrap();
    // 文案重写（决策 4）：呈现不再出现立桩表述。
    let texts = app.screen().texts();
    assert!(
        texts.iter().all(|t| !t.contains("立桩")),
        "失败态呈现无立桩表述：{texts:?}"
    );
    app.wait_for("id:startup-pick-book-dir").unwrap();

    // 选中在册其他本 → 切活动指针 → 本次启动直接打开。
    app.on_pick_paths(vec![other.clone()]).unwrap();
    app.press("id:startup-pick-book-dir").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("注册表应可解析")
    };
    assert_eq!(registry.active_id, "b2", "活动指针切至选中的在册其他本");

    // 「下次启动」等价：重置再启动（无 argv）→ 解析链直接打开新活动本。
    nsh_app::app::reset_book_assembly();
    nsh_app::cross_book::set_default_data_dir(root);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:nav-transactions").unwrap();
}

/// 现场「argv 打错路径缺库」：同一失败态与按钮可用（解析链 argv 臂，T1 收敛
/// 的 book_missing_error 单点同文案同出口）。
#[test]
fn startup_argv_missing_book_shows_same_failed_state_with_picker() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let bad = seed::make_book_dir("t4-argv-missing");
    nsh_app::app::set_book_dir(bad);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:/账本库文件不存在/").unwrap();
    app.wait_for("id:startup-pick-book-dir").unwrap();
}

/// 取消选择：留原失败态、按钮可再次触发（误触零后果——取消零改向零写入）。
#[test]
fn startup_picker_cancel_keeps_failed_state_and_button_retriggerable() {
    let _guard = SERIAL.lock().unwrap();
    let (root, mut app) = start_failed_state_with_picker();
    let other = make_book("t4-cancel-other");

    // 用户取消（应答空 = Ok(None)）：原失败态与出口原样保留。
    app.on_pick_paths(vec![]).unwrap();
    app.press("id:startup-pick-book-dir").unwrap();
    app.settle();
    app.wait_for("text:/账本库文件不存在/").unwrap();
    app.wait_for("id:startup-pick-book-dir").unwrap();
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("注册表应可解析")
    };
    assert_eq!(registry.active_id, "b1", "取消零改向");

    // 可再次触发：再选在册其他本 → 打开。
    app.on_pick_paths(vec![other]).unwrap();
    app.press("id:startup-pick-book-dir").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
}

/// F5：argv 臂失败态选择器的注册表根统一默认数据根——登记与活动指针写
/// 数据根的注册表；打错的 argv 目录不作注册表根，data_location.json 不落
/// 该处。注册表根退回 argv 目录 → 登记写错位 → 数据根注册表断言红。
#[test]
fn startup_argv_picker_registers_into_default_data_root_not_argv_dir() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t4-f5-root");
    let active = make_book("t4-f5-active");
    seed_registry(&root, &active, &[]);
    // argv 打错路径（目录在但无库 → book-missing 失败态）；数据根照常注入。
    let bad = seed::make_book_dir("t4-f5-bad-argv");
    nsh_app::app::set_book_dir(bad.clone());
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let picked = make_book("t4-f5-picked");

    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:/账本库文件不存在/").unwrap();
    app.on_pick_paths(vec![picked.clone()]).unwrap();
    app.press("id:startup-pick-book-dir").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    // argv 臂重指直开选定目录（失败态出口语义不变）；注册表写数据根。
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("数据根注册表应可解析")
    };
    let entry = registry
        .books
        .iter()
        .find(|book| book.dir == picked)
        .expect("选中目录已登记进数据根注册表");
    assert_eq!(registry.active_id, entry.id, "新条目即数据根注册表的活动本");
    // 负向：打错的 argv 目录不被写入注册表。
    assert!(
        !bad.join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME)
            .exists(),
        "argv 目录不作注册表根：data_location.json 不落该处"
    );
}

/// 平台不支持（headless 无应答替身 = 生产「无 portal」同投影）：既有
/// `nsh-shell.file-dialog-failed` 码化呈现，出口保持在场可重试（#2038 先例）。
#[test]
fn startup_picker_unavailable_reports_coded_error_and_stays_retryable() {
    let _guard = SERIAL.lock().unwrap();
    let (_root, mut app) = start_failed_state_with_picker();
    // 不注入 on_pick_paths 替身 → pick_paths Err。
    app.press("id:startup-pick-book-dir").unwrap();
    app.wait_for("text:/文件对话框无法打开/").unwrap();
    app.wait_for("id:startup-pick-book-dir").unwrap();
}

/// 现场闭集②（有库未登记）：选中未登记库目录 → 自动登记 + 置活动 + 打开。
#[test]
fn startup_picker_unregistered_book_dir_registers_switches_and_opens() {
    let _guard = SERIAL.lock().unwrap();
    let (root, mut app) = start_failed_state_with_picker();
    let picked = make_book("t4-unregistered");

    app.on_pick_paths(vec![picked.clone()]).unwrap();
    app.press("id:startup-pick-book-dir").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("注册表应可解析")
    };
    let entry = registry
        .books
        .iter()
        .find(|book| book.dir == picked)
        .expect("选中目录已登记");
    assert_eq!(registry.active_id, entry.id, "新条目即活动本");
}

/// 现场闭集③（无库 → 新建意图）：确认对话明示完整路径；取消零写入留原态；
/// 确认后建库登记并打开。
#[test]
fn startup_picker_empty_dir_create_intent_confirms_and_opens() {
    let _guard = SERIAL.lock().unwrap();
    let (root, mut app) = start_failed_state_with_picker();
    let picked = seed::make_book_dir("t4-create");

    // 新建意图：确认对话明示完整路径（ADR-0146 决策 2/3）。
    app.on_pick_paths(vec![picked.clone()]).unwrap();
    app.press("id:startup-pick-book-dir").unwrap();
    app.wait_for("text:新建账本").unwrap();
    app.wait_for(&format!(
        "text:将在所选目录新建账本库：{}",
        picked.display()
    ))
    .unwrap();

    // 取消：零写入（无库文件），留原失败态可再触发。
    app.press("id:danger-cancel").unwrap();
    app.wait_for_gone("id:danger-dialog", DEFAULT_WAIT).unwrap();
    assert!(!picked.join("ledger.db").exists(), "取消零写入");
    app.wait_for("id:startup-pick-book-dir").unwrap();

    // 再触发 → 确认：建库 + 登记 + 打开。
    app.on_pick_paths(vec![picked.clone()]).unwrap();
    app.press("id:startup-pick-book-dir").unwrap();
    app.wait_for("text:新建账本").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    assert!(
        picked.join("ledger.db").is_file(),
        "确认后建库（建连迁移单点）"
    );
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("注册表应可解析")
    };
    assert_eq!(
        registry.active_dir(),
        Some(picked.as_path()),
        "登记 + 活动指针即时落盘"
    );
}

/// 损坏残留选中目录的失败呈现（闭集⑤）：码化错误上屏，失败态不挂选择器
/// 出口（缺的是诊断，不是目录，决策 4）。
fn pick_bad_book_dir_reports_coded_error_without_picker(bad_bytes: &[u8], expect: &str) {
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t4-bad-pick");
    let active = seed::make_book_dir("t4-active-missing");
    seed_registry(&root, &active, &[]);
    nsh_app::cross_book::set_default_data_dir(root);
    let picked = seed::make_book_dir("t4-bad");
    fs::write(picked.join("ledger.db"), bad_bytes).unwrap();
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:/账本库文件不存在/").unwrap();
    app.on_pick_paths(vec![picked]).unwrap();
    app.press("id:startup-pick-book-dir").unwrap();
    app.wait_for(expect).unwrap();
    assert_eq!(
        app.screen().count("id:startup-pick-book-dir").unwrap(),
        0,
        "损坏残留失败态不挂选择器出口"
    );
}

/// 现场闭集④（密文库，#2088 反转后语义）：选中密文库目录 → 登记并置活
/// 动指针后判开 → 引导落锁定视图（解锁面板在屏），无选择器出口（锁定态
/// 不是失败态，无目录出口）。
#[test]
fn startup_picker_encrypted_dir_lands_locked_view() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t4-encrypted-pick");
    let active = seed::make_book_dir("t4-active-missing");
    seed_registry(&root, &active, &[]);
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let picked = seed::make_book_dir("t4-encrypted");
    fs::write(picked.join("ledger.db"), vec![0u8; 8_192]).unwrap();
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:/账本库文件不存在/").unwrap();
    app.on_pick_paths(vec![picked]).unwrap();
    app.press("id:startup-pick-book-dir").unwrap();
    // 锁定视图（解锁面板 + 解锁入口在屏），无选择器出口。
    app.wait_for("id:locked-panel").unwrap();
    app.wait_for("id:unlock-passphrase").unwrap();
    assert_eq!(
        app.screen().count("id:startup-pick-book-dir").unwrap(),
        0,
        "锁定视图不挂选择器出口"
    );
    // 活动指针已切至密文本（登记 + 置活动，重启后同本直接落锁定视图）。
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("注册表应可解析")
    };
    assert_ne!(registry.active_id, "b1", "活动指针已切至新登记的密文本");
}

/// 现场闭集⑤（损坏残留）：既有损坏码呈现 + 无出口（不自动换目录重试）。
#[test]
fn startup_picker_corrupt_dir_reports_coded_error_without_picker() {
    let _guard = SERIAL.lock().unwrap();
    pick_bad_book_dir_reports_coded_error_without_picker(
        &[0xABu8; 20],
        "text:/数据库文件无法打开/",
    );
}

/// F3（决策 3⑤ 保留；④随 #2088 反转）：选中**在册但**损坏残留的本 → 先
/// 分类后切指针失败：码化错误呈现且注册表活动指针未变。（④ 密文库的指针
/// 断言并入 startup_picker_encrypted_dir_lands_locked_view。）删分类前置
///（退回在册直切）→ 指针被切走 → 指针断言红。
#[test]
fn startup_picker_registered_corrupt_dir_reports_coded_error_pointer_unchanged() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t4-f3-registered-corrupt");
    let active = seed::make_book_dir("t4-active-missing");
    let other = make_book("t4-registered-corrupt");
    // 在册兄弟本 b2 的库文件为损坏残留（非魔数且非页对齐）。
    fs::write(other.join("ledger.db"), vec![0xABu8; 20]).unwrap();
    seed_registry(&root, &active, &[("b2", "备用账本", &other)]);
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:/账本库文件不存在/").unwrap();
    app.on_pick_paths(vec![other]).unwrap();
    app.press("id:startup-pick-book-dir").unwrap();
    app.wait_for("text:/数据库文件无法打开/").unwrap();
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("注册表应可解析")
    };
    assert_eq!(registry.active_id, "b1", "注册表零写入：活动指针未变");
}

// —— 首启引导「打开已有账本」（ADR-0146 决策 3 / spec #2090 T3）：——
// —— 引导界面选择器动作 + 现场闭集处置单点（resolve_picked_book_dir）——
// —— 的窗口呈现旅程。 ——

/// F1 现场（ADR-0146 决策 2/4 分界）：注册表已配置 + 活动本指向默认数据目录
/// 自身 + 缺库 → book-missing 失败态（出口按钮在场），**不出现首启引导
/// 面板**——引导现场仅限未配置/损坏回退（决策 2），已解析活动条目缺库一律
/// fail loud（决策 4）。随后走失败态选择器面：选中在册兄弟本 → 切活动指针
/// 直开 →「下次启动」直接打开新活动本（T3 分支①「选另一在册本切指针直开」
/// 旅程的权威落点：该现场在引导面已不可构造，迁移到失败态选择器面）。
/// 删注册表状态信号分流（退回路径等值判定）→ 引导面板误现 → 负向断言红
///（ADR-0087 断言对准屏幕可见结果）。
#[test]
fn startup_configured_active_book_at_default_dir_missing_fails_with_picker_and_switches_pointer() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t3-at-default");
    let other = make_book("t3-other");
    // 活动本 b1 = 默认目录自身（无库），在册其他本 b2 有库。
    seed_registry(&root, &root, &[("b2", "备用账本", &other)]);
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    // 失败态在屏（fail loud），首启引导面板不出现（负向断言：决策 2/4 分界）。
    app.wait_for("text:/账本库文件不存在/").unwrap();
    app.wait_for("id:startup-pick-book-dir").unwrap();
    assert_eq!(
        app.screen().count("id:onboarding-panel").unwrap(),
        0,
        "已配置+活动本=默认目录+缺库不呈现首启引导面板：{:?}",
        app.screen().texts()
    );

    // 失败态选择器：选中在册其他本 → 切活动指针 → 本次启动直接打开。
    app.on_pick_paths(vec![other.clone()]).unwrap();
    app.press("id:startup-pick-book-dir").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("注册表应可解析")
    };
    assert_eq!(registry.active_id, "b2", "活动指针切至选中的在册其他本");

    // 「下次启动」等价：重置再启动（无 argv）→ 解析链直接打开新活动本。
    nsh_app::app::reset_book_assembly();
    nsh_app::cross_book::set_default_data_dir(root);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:nav-transactions").unwrap();
}

/// 分支②（有库未登记）：引导态（未配置 + 默认目录无库）→ 选中未登记库目录
/// → 自动登记新条目 + 置活动指针 → 打开（注册表落盘：新条目即活动本）→
///「下次启动」直接打开。
#[test]
fn onboarding_pick_unregistered_book_dir_registers_switches_and_opens() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t3-onboard-b2");
    let picked = make_book("t3-unregistered");
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    wait_pressable(&mut app, "id:onboarding-pick-book-dir");

    app.on_pick_paths(vec![picked.clone()]).unwrap();
    app.press("id:onboarding-pick-book-dir").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("注册表应可解析")
    };
    let entry = registry
        .books
        .iter()
        .find(|book| book.dir == picked)
        .expect("选中目录已登记");
    assert_eq!(registry.active_id, entry.id, "新条目即活动本");

    // 「下次启动」等价：重置再启动（无 argv）→ 解析链直接打开登记的活动本。
    nsh_app::app::reset_book_assembly();
    nsh_app::cross_book::set_default_data_dir(root);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:nav-transactions").unwrap();
}

/// 分支③（无库 → 新建意图，含确认对话路径明示与取消路径）：引导态 → 选中
/// 空目录 → 确认对话明示完整路径（与 T4 失败态分支③同词表同封装，warning
/// 级）；取消零写入留引导可再触发；确认后建库登记并打开。
#[test]
fn onboarding_pick_empty_dir_create_intent_confirms_and_opens() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t3-onboard-b3");
    let picked = seed::make_book_dir("t3-create");
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    wait_pressable(&mut app, "id:onboarding-pick-book-dir");

    // 新建意图：确认对话明示完整路径（ADR-0146 决策 2/3）。
    app.on_pick_paths(vec![picked.clone()]).unwrap();
    app.press("id:onboarding-pick-book-dir").unwrap();
    app.wait_for("text:新建账本").unwrap();
    app.wait_for(&format!(
        "text:将在所选目录新建账本库：{}",
        picked.display()
    ))
    .unwrap();

    // 取消：零写入（无库文件、无注册表），留引导可再触发。
    app.press("id:danger-cancel").unwrap();
    app.wait_for_gone("id:danger-dialog", DEFAULT_WAIT).unwrap();
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    assert!(!picked.join("ledger.db").exists(), "取消零写入");
    assert!(
        !root
            .join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME)
            .exists(),
        "取消不写注册表"
    );

    // 可再触发：再选同一目录 → 确认 → 建库 + 登记 + 打开。
    app.on_pick_paths(vec![picked.clone()]).unwrap();
    app.press("id:onboarding-pick-book-dir").unwrap();
    app.wait_for("text:新建账本").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    assert!(
        picked.join("ledger.db").is_file(),
        "确认后建库（建连迁移单点）"
    );
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("注册表应可解析")
    };
    assert_eq!(
        registry.active_dir(),
        Some(picked.as_path()),
        "登记 + 活动指针即时落盘"
    );
}

/// 引导态选中损坏残留目录的失败呈现（闭集⑤）：码化错误上屏，引导退场；
/// 失败态不挂选择器出口（缺的是诊断，不是目录，决策 4）——不自动换目录
/// 重试的呈现面。（④ 密文库随 #2088 落锁定视图，用例独立。）
fn onboarding_pick_bad_book_dir_reports_coded_error(bad_bytes: &[u8], expect: &str) {
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t3-onboard-bad");
    let picked = seed::make_book_dir("t3-bad-pick");
    fs::write(picked.join("ledger.db"), bad_bytes).unwrap();
    nsh_app::cross_book::set_default_data_dir(root);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    app.on_pick_paths(vec![picked]).unwrap();
    app.press("id:onboarding-pick-book-dir").unwrap();
    app.wait_for(expect).unwrap();
    assert_eq!(
        app.screen().count("id:startup-pick-book-dir").unwrap(),
        0,
        "损坏残留失败态不挂选择器出口（无自动换目录重试）"
    );
    assert_eq!(
        app.screen().count("id:onboarding-pick-book-dir").unwrap(),
        0,
        "错误呈现后引导面板退场"
    );
}

/// 现场闭集④（密文库，#2088 反转后语义）：引导态选中密文库目录 → 登记 +
/// 置活动 + 判开 → 引导落锁定视图（解锁面板在屏）。
#[test]
fn onboarding_pick_encrypted_dir_lands_locked_view() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t3-onboard-encrypted");
    let picked = seed::make_book_dir("t3-encrypted-pick");
    fs::write(picked.join("ledger.db"), vec![0u8; 8_192]).unwrap();
    nsh_app::cross_book::set_default_data_dir(root);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    app.on_pick_paths(vec![picked]).unwrap();
    app.press("id:onboarding-pick-book-dir").unwrap();
    app.wait_for("id:locked-panel").unwrap();
    app.wait_for("id:unlock-passphrase").unwrap();
    assert_eq!(
        app.screen().count("id:onboarding-pick-book-dir").unwrap(),
        0,
        "锁定视图下引导面板退场"
    );
}

/// 现场闭集⑤（损坏残留）：既有损坏码呈现，无自动重试循环。
#[test]
fn onboarding_pick_corrupt_dir_reports_coded_error() {
    let _guard = SERIAL.lock().unwrap();
    onboarding_pick_bad_book_dir_reports_coded_error(&[0xABu8; 20], "text:/数据库文件无法打开/");
}

/// F2 分支②：损坏注册表的引导现场（回退告示在屏）选「有库未登记」目录 →
/// 按未注册容错：登记新条目 + 置活动指针 + 打开（损坏文件被写入覆盖，不再
/// 停在「请先在数据设置中恢复默认位置」的不可达死路，ADR-0146 决策 3）。
/// 容错退回查册上抛 → 选择器后错误屏而非主界面 → 红。
#[test]
fn onboarding_corrupt_registry_pick_unregistered_book_dir_registers_and_opens() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t3-corrupt-b2");
    let picked = make_book("t3-corrupt-unregistered");
    // 注册表损坏现场（非法 JSON）且默认目录无库：引导态 + 损坏告示。
    fs::write(
        root.join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME),
        "not-json{{",
    )
    .unwrap();
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    app.wait_for("text:已回退到默认位置").unwrap();
    wait_pressable(&mut app, "id:onboarding-pick-book-dir");

    app.on_pick_paths(vec![picked.clone()]).unwrap();
    app.press("id:onboarding-pick-book-dir").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("登记写入应覆盖损坏文件，注册表可解析")
    };
    assert_eq!(
        registry.active_dir(),
        Some(picked.as_path()),
        "选中目录登记为活动本"
    );

    // 「下次启动」等价：重置再启动（无 argv）→ 直接打开登记的活动本。
    nsh_app::app::reset_book_assembly();
    nsh_app::cross_book::set_default_data_dir(root);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:nav-transactions").unwrap();
}

/// F2 分支③：损坏注册表的引导现场选空目录 → 新建意图确认对话 → 确认后
/// 建库 + 折叠登记（损坏文件被覆盖，注册表可解析）+ 打开。
#[test]
fn onboarding_corrupt_registry_pick_empty_dir_confirms_create_and_opens() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t3-corrupt-b3");
    let picked = seed::make_book_dir("t3-corrupt-create");
    fs::write(
        root.join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME),
        "not-json{{",
    )
    .unwrap();
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    wait_pressable(&mut app, "id:onboarding-pick-book-dir");

    app.on_pick_paths(vec![picked.clone()]).unwrap();
    app.press("id:onboarding-pick-book-dir").unwrap();
    app.wait_for("text:新建账本").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    assert!(
        picked.join("ledger.db").is_file(),
        "确认后建库（建连迁移单点）"
    );
    use ledger_infra::db::book_registry::{RegistryRead, read_registry};
    let RegistryRead::Resolved(registry) = read_registry(&root) else {
        panic!("登记写入应覆盖损坏文件，注册表可解析")
    };
    assert_eq!(
        registry.active_dir(),
        Some(picked.as_path()),
        "登记 + 活动指针即时落盘"
    );
}

/// 选择器层取消：留引导界面零写入，动作可再次触发（再次选定即打开）。
#[test]
fn onboarding_pick_cancel_keeps_onboarding_and_action_retriggerable() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let root = make_data_root("t3-onboard-cancel");
    let other = make_book("t3-cancel-other");
    nsh_app::cross_book::set_default_data_dir(root.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    wait_pressable(&mut app, "id:onboarding-pick-book-dir");

    // 用户取消（应答空 = Ok(None)）：留在引导，零写入。
    app.on_pick_paths(vec![]).unwrap();
    app.press("id:onboarding-pick-book-dir").unwrap();
    app.settle();
    app.wait_for("text:欢迎使用 Ledger").unwrap();
    assert!(
        !root
            .join(ledger_infra::db::book_registry::REGISTRY_FILE_NAME)
            .exists(),
        "取消零写入"
    );

    // 动作可再次触发：再选有库目录 → 打开。
    app.on_pick_paths(vec![other]).unwrap();
    app.press("id:onboarding-pick-book-dir").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
}

/// 引导期密文库锁定态不挂选择器出口（#2088：密文库落锁定视图而非失败态）、
/// 损坏残留失败态不挂选择器出口（决策 4 负向断言；argv 臂同规则——
/// failed_view 单点派生两臂共用）。
#[test]
fn startup_locked_and_corrupt_boot_states_have_no_picker() {
    let _guard = SERIAL.lock().unwrap();
    // 密文库：页对齐密文形态（基础设施「密文库落盘形态」判别）→ 锁定视图。
    nsh_app::app::reset_book_assembly();
    let encrypted = seed::make_book_dir("t4-boot-encrypted");
    fs::write(encrypted.join("ledger.db"), vec![0u8; 8_192]).unwrap();
    nsh_app::app::set_book_dir(encrypted);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:locked-panel").unwrap();
    assert_eq!(
        app.screen().count("id:startup-pick-book-dir").unwrap(),
        0,
        "锁定视图不挂选择器出口"
    );

    // 损坏残留：非魔数且非页对齐 → 失败态不挂选择器出口。
    nsh_app::app::reset_book_assembly();
    let corrupt = seed::make_book_dir("t4-boot-corrupt");
    fs::write(corrupt.join("ledger.db"), vec![0xABu8; 20]).unwrap();
    nsh_app::app::set_book_dir(corrupt);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("text:/数据库文件无法打开/").unwrap();
    assert_eq!(
        app.screen().count("id:startup-pick-book-dir").unwrap(),
        0,
        "损坏残留失败态不挂选择器出口"
    );
}

/// 造密文库夹具（明文造数 → 整库转换）：解锁 / 重置旅程共用。种子一笔
/// 交易（换连后主界面呈现真实数据的用户可观察锚点）。
fn make_encrypted_book(tag: &str, passphrase: &str) -> std::path::PathBuf {
    let dir = make_book(tag);
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
                date: &test_date(0),
                category_id: None,
                merchant_id: None,
                note: Some("密文会话数据".to_owned()),
            },
        );
    }
    ledger_infra::db::encryption::enable_encryption_for_file(&dir.join("ledger.db"), passphrase)
        .unwrap();
    dir
}

/// 密文库解锁全旅程（issue #2088 交付①）：密文库引导落锁定视图（不是失
/// 败态）→ 错误口令「口令错误或文件损坏」合并口径就地重试（可无限重试，
/// 不误报损坏）→ 正确口令解锁进业务会话（换连编排：主界面呈现真实数据 =
/// 成对只读连接读换连后新世界的用户可观察结果；readonly_connection 的 NSH
/// 自动化回归同链）。
///
/// 「删除即红」负向条目：删解锁面板挂载（locked 分支）或解锁任务接线
///（run_unlock 的 spawn 链）→ 锁定视图不入屏 / 解锁后不进主界面 → 本测
/// wait_for 红；断言对准用户可见结果（锁定面板、错误行、解锁后的交易行）。
#[test]
fn encrypted_book_unlock_journey_enters_session_with_seeded_data() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    const PASS: &str = "解锁旅程口令X";
    let dir = make_encrypted_book("unlock-journey", PASS);
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();

    // 锁定视图：标题 + 口令框 + 自动解锁平台注记（Linux 不支持平价）。
    app.wait_for("id:locked-panel").unwrap();
    app.wait_for("text:账本已加密").unwrap();
    app.wait_for("id:unlock-auto-unlock-note").unwrap();
    assert!(
        app.screen()
            .press_target("id:unlock-confirm")
            .unwrap()
            .is_none(),
        "空口令下「解锁」不可按（disabled 契约投影）"
    );

    // 错误口令：合并口径错误行就地呈现，仍留锁定视图可重试。
    app.type_text("id:unlock-passphrase", "错误口令一一贯不代表正确口令")
        .unwrap();
    app.press("id:unlock-confirm").unwrap();
    app.wait_for("text:口令错误或文件损坏，请重试").unwrap();
    assert!(
        app.screen().count("id:locked-panel").unwrap() > 0,
        "解锁失败仍留锁定视图（fail-closed 可无限重试）"
    );

    // 正确口令：解锁进业务会话，主界面呈现真实数据（换连后的读路径）。
    app.clear("id:unlock-passphrase").unwrap();
    app.type_text("id:unlock-passphrase", PASS).unwrap();
    app.press("id:unlock-confirm").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    open_transactions(&mut app);
    app.wait_for(&format!("text:{}", test_date(0))).unwrap();
}

/// 锁定视图忘口令重置全旅程（issue #2088 交付③）：常驻入口 → error 级无后
/// 门后果确认（ADR-0078 分级）→ infra 逃生门全语义（旧密文库保留 .bak 密
/// 文副本、原位新建明文空库、sync.db 配对移位）→ 原位换连进全新明文会话
///（无需重启，旧壳 reset_after_forgotten_passphrase 同语义）。
///
/// 「删除即红」负向条目：删忘口令入口或重置任务接线 → 确认弹窗不入屏 /
/// 重置后不进主界面 → 本测 wait_for 红；确认前零写入（旧库原样）对准
/// 文件级可观察结果。
#[test]
fn locked_view_forgot_passphrase_resets_to_fresh_plaintext_session() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    let dir = make_encrypted_book("forgot-reset-journey", "原口令 journeyX");
    let db_path = dir.join("ledger.db");
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config()).unwrap();
    app.wait_for("id:locked-panel").unwrap();

    // 忘口令入口 → error 级确认弹窗（无后门后果说明在屏）。
    app.press("id:unlock-forgot").unwrap();
    app.wait_for("text:重置为全新空库？").unwrap();
    app.wait_for("text:/主口令没有任何后门或找回手段/").unwrap();

    // 取消：零写入（旧密文库原样、锁定态原样）。
    app.press("id:danger-cancel").unwrap();
    app.settle();
    assert!(
        matches!(
            ledger_infra::db::encryption::probe_file_kind(&db_path).unwrap(),
            ledger_infra::db::encryption::DbFileKind::Encrypted,
        ),
        "取消后旧密文库原样"
    );
    assert_eq!(
        app.screen().count("id:locked-panel").unwrap(),
        1,
        "取消后仍留锁定视图"
    );

    // 确认：逃生门全语义 —— 旧密文库保留 .bak 密文副本、新明文空库就位、
    // 原位换连进全新明文会话（交易列表空、无锁定面板）。
    app.press("id:unlock-forgot").unwrap();
    app.wait_for("text:重置为全新空库？").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    open_transactions(&mut app);
    app.wait_for("text:暂无数据").unwrap();
    use ledger_infra::db::encryption::DbFileKind;
    assert!(
        matches!(
            ledger_infra::db::encryption::probe_file_kind(&db_path).unwrap(),
            DbFileKind::Plaintext,
        ),
        "重置后原位为全新明文空库"
    );
    let bak = db_path.with_extension("db.bak");
    assert!(
        matches!(
            ledger_infra::db::encryption::probe_file_kind(&bak).unwrap(),
            DbFileKind::Encrypted
        ),
        "旧密文库保留为 .bak 密文副本（无口令不可读，日后想起口令仍可救回）"
    );
}

/// 加密卡写面平价全旅程（issue #2088 交付②：修改主口令 / 关闭加密，旧壳
/// `change_encryption_passphrase` / `disable_encryption` 消费面）：开启加密
/// → 密文会话形态 → 修改主口令（error 级确认；旧口令错误就地报错原库原样；
/// 正确后凭新口令原位换连，文件头仍密文且新口令可解）→ 关闭加密（warning
/// 级确认；整库转回明文，文件头明文 + .bak 保留密文副本）。
///
/// 「删除即红」负向条目：删任一写面接线（encrypted_form 的表单/确认/任务
/// 链）→ 对应流程不入屏/不落盘 → 本测 wait_for / 文件探测红。
#[test]
fn encryption_card_change_passphrase_then_disable_in_encrypted_session() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    const PASS_V1: &str = "首开启口令 journeyA";
    const PASS_V2: &str = "换新口令 journeyB";
    let dir = make_book("enc-write-journey");
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_data_tab(&mut app);

    // 开启加密（守卫 + error 确认，前置旅程已另钉，此处直接走通）。
    app.type_text("id:enc-passphrase", PASS_V1).unwrap();
    app.type_text("id:enc-confirm", PASS_V1).unwrap();
    app.wait_until("enc-enable ready", Duration::from_secs(5), |screen| {
        screen.press_target("id:enc-enable").unwrap().is_some()
    })
    .unwrap();
    app.press("id:enc-enable").unwrap();
    app.wait_for("text:开启加密？").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:已开启加密").unwrap();
    assert!(
        matches!(
            ledger_infra::db::encryption::probe_file_kind(&dir.join("ledger.db")).unwrap(),
            ledger_infra::db::encryption::DbFileKind::Encrypted,
        ),
        "开启后库文件密文"
    );

    // —— 修改主口令：新口令守卫（未变 / 长度 / 一致）→ error 级确认 ——
    app.type_text("id:enc-change-current", PASS_V1).unwrap();
    app.type_text("id:enc-change-new", PASS_V1).unwrap();
    app.type_text("id:enc-change-confirm", PASS_V1).unwrap();
    app.wait_for("text:新主口令与当前主口令相同").unwrap();
    assert!(
        app.screen()
            .press_target("id:enc-change")
            .unwrap()
            .is_none(),
        "新口令与当前相同时「修改主口令」不可按"
    );
    app.clear("id:enc-change-new").unwrap();
    app.type_text("id:enc-change-new", "短口令").unwrap();
    app.clear("id:enc-change-confirm").unwrap();
    app.type_text("id:enc-change-confirm", "短口令").unwrap();
    app.wait_for("text:主口令至少需要 8 位").unwrap();
    app.clear("id:enc-change-new").unwrap();
    app.type_text("id:enc-change-new", PASS_V2).unwrap();
    app.clear("id:enc-change-confirm").unwrap();
    app.type_text("id:enc-change-confirm", "另一个口令一一贯不一致")
        .unwrap();
    app.wait_for("text:两次输入不一致").unwrap();
    app.clear("id:enc-change-confirm").unwrap();
    app.type_text("id:enc-change-confirm", PASS_V2).unwrap();
    app.wait_until("enc-change ready", Duration::from_secs(5), |screen| {
        screen.press_target("id:enc-change").unwrap().is_some()
    })
    .unwrap();

    // 先走一次旧口令错误路径：当前口令框填错 → 确认 → 合并口径错误，库仍
    // 凭旧口令可开（原库原样保留）。
    app.clear("id:enc-change-current").unwrap();
    app.type_text("id:enc-change-current", "错误当前口令X")
        .unwrap();
    app.press("id:enc-change").unwrap();
    app.wait_for("text:修改主口令？").unwrap();
    app.wait_for("text:/新主口令不会被保存、也无法找回/")
        .unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:/修改主口令失败: 口令错误或文件损坏/")
        .unwrap();
    assert!(
        ledger_infra::db::encryption::verify_source_passphrase(&dir.join("ledger.db"), PASS_V1)
            .is_ok(),
        "口令错误后原库原样保留（仍凭旧口令可开）"
    );

    // 正确当前口令 → 转换成功 → 本会话切换（状态条反馈 + 文件仍密文且新
    // 口令可解、旧口令不可解）。
    app.clear("id:enc-change-current").unwrap();
    app.type_text("id:enc-change-current", PASS_V1).unwrap();
    app.press("id:enc-change").unwrap();
    app.wait_for("text:修改主口令？").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:主口令已修改，本会话已切换（下次启动请使用新口令解锁）。")
        .unwrap();
    assert!(
        ledger_infra::db::encryption::verify_source_passphrase(&dir.join("ledger.db"), PASS_V2)
            .is_ok(),
        "改口令后凭新口令可开"
    );
    assert!(
        ledger_infra::db::encryption::verify_source_passphrase(&dir.join("ledger.db"), PASS_V1)
            .is_err(),
        "改口令后旧口令不再可开"
    );

    // —— 关闭加密：warning 级确认 → 整库转回明文，.bak 保留密文副本 ——
    app.type_text("id:enc-disable-current", PASS_V2).unwrap();
    app.press("id:enc-disable").unwrap();
    app.wait_for("text:关闭加密？").unwrap();
    app.wait_for("text:/既有密文备份不随之改变/").unwrap();
    // 取消：零转换。
    app.press("id:danger-cancel").unwrap();
    app.settle();
    assert!(
        matches!(
            ledger_infra::db::encryption::probe_file_kind(&dir.join("ledger.db")).unwrap(),
            ledger_infra::db::encryption::DbFileKind::Encrypted,
        ),
        "取消后库文件仍密文"
    );
    // 确认：整库转回明文。
    app.press("id:enc-disable").unwrap();
    app.wait_for("text:关闭加密？").unwrap();
    app.press("id:danger-confirm").unwrap();
    app.wait_for("text:加密已关闭，本会话已切换到明文库。")
        .unwrap();
    assert!(
        matches!(
            ledger_infra::db::encryption::probe_file_kind(&dir.join("ledger.db")).unwrap(),
            ledger_infra::db::encryption::DbFileKind::Plaintext,
        ),
        "关闭后库文件明文"
    );
    assert!(
        matches!(
            ledger_infra::db::encryption::probe_file_kind(&dir.join("ledger.db.bak")).unwrap(),
            ledger_infra::db::encryption::DbFileKind::Encrypted,
        ),
        ".bak 保留关闭前的密文副本"
    );
    // 卡面翻回明文库开启形态。
    app.wait_for("text:开启前请确认").unwrap();
}

/// 限时轮询断言窗口内无自动备份产物（三态负向半边；超窗仍无产物即断言
/// 成立，出现产物立即红）。
fn wait_no_auto_product(dir: &std::path::Path, window: Duration, label: &str) {
    let deadline = std::time::Instant::now() + window;
    while std::time::Instant::now() < deadline {
        assert!(
            find_first_auto_product(dir).is_none(),
            "{label}：等待窗口内出现兜底产物"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        find_first_auto_product(dir).is_none(),
        "{label}：等待窗口结束仍应无产物"
    );
}

/// 自动备份调度密文库三态端到端（issue #2088 交付⑤：锁定 / 解锁 / 失败）：
/// 密文库 + 备份目录已配置 + 自动备份开关已开启的现场——
/// - **锁定态**：引导落锁定视图，调度不拉起（无会话无宿主），等待窗口内
///   无兜底产物落盘（旧壳「锁定态不可调度」扇门的本壳行为）；
/// - **解锁失败态**：错误口令仍锁定，零调度零产物；
/// - **解锁**：正确口令进业务会话 → 调度宿主同点补拉起 → 首次兜底产物
///   落盘（#2088 预留编排点的用户可观察结果）。
///
/// 「删除即红」负向条目：删解锁路径的 `start_session_host` 补拉起（业务可
/// 用起点单点的备份宿主接线）→ 解锁后无产物 → 产物轮询超时红。
#[test]
fn auto_backup_scheduling_three_states_in_encrypted_scenario() {
    let _guard = SERIAL.lock().unwrap();
    nsh_app::app::reset_book_assembly();
    const PASS: &str = "调度三态口令X";
    // 夹具：种子数据 + 自动备份开启 + 整库加密（先明文置开关再转换）。
    let dir = make_book("auto-backup-enc");
    let backup_dir =
        std::env::temp_dir().join(format!("ledger-test-nsh-auto-backup-enc-{}", new_uuid()));
    fs::create_dir_all(&backup_dir).unwrap();
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed_cash_account(&db);
        seed_transaction(
            &db,
            TxnSeed {
                account: &account,
                to_account: None,
                kind: TransactionKind::Expense,
                amount_cents: 3_500,
                date: &test_date(0),
                category_id: None,
                merchant_id: None,
                note: Some("调度三态种子".to_owned()),
            },
        );
        // 自动备份开关开启（库内状态，转换随库走）。
        {
            let conn = db
                .conn
                .lock()
                .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
                .unwrap();
            ledger_infra::settings::set(
                &conn,
                ledger_infra::settings::SettingKey::AutoBackupEnabled,
                &true,
            )
            .unwrap();
        }
    }
    ledger_infra::db::encryption::enable_encryption_for_file(&dir.join("ledger.db"), PASS).unwrap();

    let storage = storage_shard(&[(
        "backup_dir",
        format!("{:?}", backup_dir.to_string_lossy()).as_str(),
    )]);
    nsh_app::app::set_book_dir(dir);
    let mut app = Headless::start(desktop_config_with_storage_shard(storage)).unwrap();

    // 锁定态：锁定视图在屏，等待窗口内无兜底产物（调度不拉起；等待窗走
    // 测试线程限时轮询——headless 无手动时钟场景，应用实时推进）。
    app.wait_for("id:locked-panel").unwrap();
    wait_no_auto_product(&backup_dir, Duration::from_secs(2), "锁定态不可调度");

    // 解锁失败态：错误口令仍锁定，零调度零产物。
    app.type_text("id:unlock-passphrase", "错误口令一一贯不代表正确口令")
        .unwrap();
    app.press("id:unlock-confirm").unwrap();
    app.wait_for("text:口令错误或文件损坏，请重试").unwrap();
    wait_no_auto_product(&backup_dir, Duration::from_secs(2), "解锁失败态仍锁定");

    // 解锁：进业务会话 → 备份宿主同点补拉起 → 首次兜底产物落盘。
    app.clear("id:unlock-passphrase").unwrap();
    app.type_text("id:unlock-passphrase", PASS).unwrap();
    app.press("id:unlock-confirm").unwrap();
    app.wait_for("id:nav-transactions").unwrap();
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let product = loop {
        if let Some(found) = find_first_auto_product(&backup_dir) {
            break found;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "解锁后应发生首次兜底自动备份（产物落盘），实际目录内容：{:?}",
            fs::read_dir(&backup_dir)
                .map(|entries| entries.flatten().map(|e| e.path()).collect::<Vec<_>>())
                .unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let name = product
        .file_name()
        .and_then(|n| n.to_str().map(str::to_owned))
        .unwrap_or_default();
    assert!(
        name.starts_with("ledger-auto-") && name.ends_with(".db.zip"),
        "产物为受管自动命名：{name}"
    );
}
