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
