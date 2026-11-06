//! 壳行为单测（issue #2017 骨架 / issue #2018 首屏，ADR-0087 断言归属：引导与
//! 读接线归本壳）：引导序列（处置四态收窄 + 成对建连）、交易页读（读路径直调
//! 域服务）与装配层对 DB 造数行集的分页边界装配。
//!
//! 造数纪律：**不经根包 test_support 统一工厂**——工厂住根包（链接 tauri/
//! GTK3，与 NSH 的 GTK4 同进程符号互踩，src-tauri/Cargo.toml 头注；spec #2015
//! AC 原文的「统一测试数据库工厂造数」在本壳不可达），改用同纪律的分层形态：
//! 建库走**产品开库入口**（[`nsh_app::boot::boot_ledger`]，e2e world 同款分层
//! 形态，ADR-0056 决策 5 例外口径）；造数经域层公开写入口（账户 + 交易写入
//! 协议，CONTEXT-testing「公开写入口（测试侧）」纪律），余额刷新接缝已随域栈
//! 引导接线（[`crate::wiring`]）。账本目录落 `std::env::temp_dir` 下的唯一
//! 暂存目录（infra 身份工厂发 UUID，测试互不串库），进程退出由 OS 回收
//! （骨架测试无 ScratchDir 需求——不建库则无文件，与 BDD 世界持久目录同容忍）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ledger_accounts::{AccountInput, AccountType};
use ledger_currencies::Currency;
use ledger_infra::ids::new_uuid;
use ledger_transaction::{TransactionInput, TransactionKind};

use crate::app::{StartupView, headline_text, run_startup, set_book_dir};
use crate::boot::{
    CODE_BOOK_MISSING, CODE_ENCRYPTED_UNSUPPORTED, account_rows, boot_ledger, category_rows,
    currency_rows, merchant_rows, transaction_page,
};

/// 串行锁：接缝注册与 OnceLock 装配入口是进程级单例，用例间串行防串扰。
static SERIAL: Mutex<()> = Mutex::new(());

/// 唯一暂存账本目录。
fn scratch_book_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nsh-app-test-{tag}-{}", new_uuid()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// 建一个空账本库文件（0 字节：产品「新装」语义——已有文件上的空库，引导建连
/// 即完成迁移）；不经 init_db（建库归产品开库入口，check-test-support 规则 1）。
fn create_empty_book_file(dir: &Path) {
    fs::write(dir.join("ledger.db"), b"").unwrap();
}

/// 经域层公开写入口在同一账户上造一列支出（余额刷新接缝的行文证据；插入序
/// = 日期升序，供分页排序断言）。
fn seed_expense_series(dir: &Path, count: usize) {
    let db = boot_ledger(dir).unwrap();
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

/// 首页读断言（空库与造数库共用）：total 与当前页行集。
fn assert_first_page(dir: &Path, expected_total: i64, expected_rows: usize) {
    let db = boot_ledger(dir).unwrap();
    let page = transaction_page(&db, 1).unwrap();
    assert_eq!(
        page.total, expected_total,
        "total = 满足过滤条件的未删除总数"
    );
    assert_eq!(
        page.items.len(),
        expected_rows,
        "首页行数 = min(total, 每页行数)"
    );
}

#[test]
fn boot_opens_empty_book_and_reads_first_page_empty() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("empty");
    create_empty_book_file(&dir);
    assert_first_page(&dir, 0, 0);
}

#[test]
fn boot_reads_seeded_transactions_via_domain_write_entries() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("seeded");
    create_empty_book_file(&dir);
    seed_expense_series(&dir, 3);
    let db = boot_ledger(&dir).unwrap();
    let page = transaction_page(&db, 1).unwrap();
    assert_eq!(
        page.total, 3,
        "total = 公开写入口写入的未删除笔数（读路径直调域服务）"
    );
    assert_eq!(page.items.len(), 3);
    assert_eq!(
        page.items[0].note.as_deref(),
        Some("造数第 2 笔"),
        "排序 date DESC：最新在前"
    );
}

#[test]
fn page_reads_offset_paging_boundaries_with_deterministic_order() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("paged");
    create_empty_book_file(&dir);
    seed_expense_series(&dir, 250);
    let db = boot_ledger(&dir).unwrap();

    let page1 = transaction_page(&db, 1).unwrap();
    assert_eq!(page1.total, 250, "total 与页无关：满足过滤条件的总数");
    assert_eq!(page1.items.len(), 100, "首页整 100 行");
    assert_eq!(
        page1.items[0].date,
        test_date(249),
        "排序 date DESC，首页首行最新"
    );
    assert_eq!(
        page1.items[99].date,
        test_date(150),
        "首页末行 = 第 151 笔写入序"
    );

    let page2 = transaction_page(&db, 2).unwrap();
    assert_eq!(page2.items.len(), 100);
    assert_eq!(
        page2.items[0].date,
        test_date(149),
        "offset = (页码-1) × 每页行数"
    );
    assert_eq!(page2.items[99].date, test_date(50));

    let last = transaction_page(&db, 3).unwrap();
    assert_eq!(last.items.len(), 50, "末页 50 行（250 = 100 + 100 + 50）");
    assert_eq!(last.items[0].date, test_date(49));
    assert_eq!(last.items[49].date, test_date(0), "末页末行最早");

    let beyond = transaction_page(&db, 4).unwrap();
    assert_eq!(beyond.items.len(), 0, "越界页空行集");
    assert_eq!(beyond.total, 250, "越界页 total 仍返回");
}

#[test]
fn currency_dict_reads_seeded_reference_data() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("currencies");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    let rows: Vec<Currency> = currency_rows(&db).unwrap();
    let cny = rows
        .iter()
        .find(|c| c.code == "CNY")
        .expect("种子币种字典含 CNY");
    assert_eq!(cny.symbol, "¥");
    assert_eq!(cny.decimal_places, 2);
}

/// 装配层 × DB 造数行集 × 分页边界（issue #2018 AC2 的合一面）：250 笔造数的
/// 首末页行集经 [`crate::assembly::assemble_rows`] 装配成格文本，首/末页首末
/// 行的类型标签、金额（种子币种 CNY 符号）与日期逐列可读（删除装配任一环或
/// 读路径分页任一环即红）。列定义自身（无 DB 依赖）归 assembly 单测，不在此
/// 重复。
#[test]
fn page_reads_assemble_into_cell_texts_across_page_boundaries() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("assembled");
    create_empty_book_file(&dir);
    seed_expense_series(&dir, 250);
    let db = boot_ledger(&dir).unwrap();
    let currencies = crate::assembly::CurrencyTable::from_currencies(&currency_rows(&db).unwrap());
    let categories = crate::assembly::CategoryTable::default();
    let ctx = crate::assembly::RowContext {
        currencies: &currencies,
        categories: &categories,
        accounts: &crate::assembly::AccountTable::default(),
        merchants: &crate::assembly::MerchantTable::default(),
        privacy_enabled: false,
    };

    let first = crate::assembly::assemble_rows(&transaction_page(&db, 1).unwrap().items, &ctx);
    assert_eq!(first.len(), crate::assembly::PAGE_SIZE);
    assert_eq!(first[0].date, test_date(249));
    assert_eq!(first[0].kind, "支出", "类型标签首版 = kind 直译");
    assert_eq!(
        first[0].amount, "¥25",
        "金额基础格式化（种子 CNY 符号 + 去尾零）"
    );
    assert_eq!(first[99].date, test_date(150), "首页末行边界");

    let last = crate::assembly::assemble_rows(&transaction_page(&db, 3).unwrap().items, &ctx);
    assert_eq!(last.len(), 50, "末页 50 行");
    assert_eq!(last[0].date, test_date(49));
    assert_eq!(last[49].date, test_date(0), "末页末行 = 最早一笔");
    assert_eq!(last[49].amount, "¥25");
}

/// 分类路径 × DB 造数行集（issue #2019 分类路径 AC 的接线面）：经域公开写入口
/// 建父子分类，支出行引用子分类，`category_rows` 读接缝（在用口径）+ 装配层
/// 路径表产出「父 > 子」格文本；软删分类不在投影行集，引用落 `-`。删除读
/// 接缝（`category_rows`）或装配消费任一环即红。
#[test]
fn category_rows_assemble_into_path_cells_active_only() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("categories");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let parent = ledger_categories::create_category(
        &conn,
        ledger_categories::CategoryInput {
            name: "餐饮".to_owned(),
            kind: "expense".to_owned(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();
    let child = ledger_categories::create_category(
        &conn,
        ledger_categories::CategoryInput {
            name: "午餐".to_owned(),
            kind: "expense".to_owned(),
            parent_id: Some(parent.clone()),
            icon: None,
        },
    )
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
    let make_expense =
        |date: &str, category_id: Option<String>, note: Option<String>| TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents: 2_500,
            currency_code: "CNY".to_owned(),
            account_id: Some(account.clone()),
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
        };
    // 三笔各占一天（排序 date DESC：行序 = 写入序倒序，断言与 created_at
    // 同秒无关）。
    ledger_transaction::create(
        &conn,
        make_expense("2026-01-15", Some(child.clone()), Some("买咖啡".to_owned())),
    )
    .unwrap();
    ledger_transaction::create(&conn, make_expense("2026-01-16", None, None)).unwrap();
    // 软删分类：不在投影行集，历史引用落 `-`。
    let gone = ledger_categories::create_category(
        &conn,
        ledger_categories::CategoryInput {
            name: "已删分类".to_owned(),
            kind: "expense".to_owned(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();
    ledger_categories::delete_category(&conn, &gone).unwrap();
    ledger_transaction::create(&conn, make_expense("2026-01-17", Some(gone.clone()), None))
        .unwrap();
    drop(conn);

    let currencies = crate::assembly::CurrencyTable::from_currencies(&currency_rows(&db).unwrap());
    let categories = crate::assembly::CategoryTable::from_categories(&category_rows(&db).unwrap());
    let ctx = crate::assembly::RowContext {
        currencies: &currencies,
        categories: &categories,
        accounts: &crate::assembly::AccountTable::from_accounts(&account_rows(&db).unwrap()),
        merchants: &crate::assembly::MerchantTable::from_merchants(&merchant_rows(&db).unwrap()),
        privacy_enabled: false,
    };
    let rows = crate::assembly::assemble_rows(&transaction_page(&db, 1).unwrap().items, &ctx);
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].category, "-",
        "软删分类的引用不在在用投影行集（最新一笔）"
    );
    assert_eq!(rows[1].category, "-", "无分类引用");
    assert_eq!(rows[1].note, "-", "无备注");
    assert_eq!(rows[2].category, "餐饮 > 午餐", "子分类为「父 > 子」路径");
    assert_eq!(rows[2].note, "买咖啡");
}

/// 账户与商户读接缝（#2020 账户格 / 借贷派生 / 商户格的参考数据面）：账户读
/// **在用可见**口径（排除软删与隐藏——旧壳 accountMap 同口径），商户读**含软删
/// 全量**（历史交易显示，issue #191）。删除读接缝（account_rows / merchant_rows）
/// 或装配消费任一环即红（呈现侧断言归 headless 集成测试）。
#[test]
fn account_and_merchant_rows_read_with_reference_visibility_rules() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("refs");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let visible = ledger_accounts::create_account(
        &conn,
        AccountInput {
            name: "现金".to_owned(),
            kind: AccountType::Cash,
            currency_code: "CNY".to_owned(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap();
    // 黑洞账户（隐藏，不在 list_accounts 投影）：域公开入口即建路径。
    ledger_accounts::ensure_black_hole_account(&conn, "CNY").unwrap();
    let merchant = ledger_merchants::create_merchant(
        &conn,
        ledger_merchants::MerchantInput {
            name: "京东".to_owned(),
        },
    )
    .unwrap();
    ledger_merchants::delete_merchant(&conn, &merchant).unwrap();
    drop(conn);

    let accounts = account_rows(&db).unwrap();
    assert_eq!(
        accounts.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        [visible.as_str()],
        "账户投影只收在用且未隐藏（黑洞不在表）"
    );
    let merchants = merchant_rows(&db).unwrap();
    assert_eq!(
        merchants
            .iter()
            .map(|m| (m.id.as_str(), m.is_deleted))
            .collect::<Vec<_>>(),
        [(merchant.as_str(), true)],
        "商户投影含软删全量（历史交易显示口径）"
    );
}

#[test]
fn boot_missing_book_file_fails_coded() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("missing");
    let err = match boot_ledger(&dir) {
        Ok(_) => panic!("库文件缺失应报错，实际引导成功"),
        Err(e) => e,
    };
    assert!(
        err.is_code(CODE_BOOK_MISSING),
        "库文件缺失报码化错误，实际：{err}"
    );
}

#[test]
fn boot_page_aligned_foreign_header_fails_as_encrypted_unsupported() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("locked");
    // 两整页非明文魔数字节（页对齐 = 基础设施「密文库落盘形态」判别）：
    // 引导层分类为 AwaitUnlock，立桩收窄为码化错误（无解锁向导）。
    fs::write(dir.join("ledger.db"), vec![0u8; 8_192]).unwrap();
    let err = match boot_ledger(&dir) {
        Ok(_) => panic!("密文库应报立桩范围外错误，实际引导成功"),
        Err(e) => e,
    };
    assert!(
        err.is_code(CODE_ENCRYPTED_UNSUPPORTED),
        "密文库报立桩范围外码化错误，实际：{err}"
    );
}

/// 立桩竖井的端到端视图态（装配入口注入 → 引导 → 会话就位 → 首页读）：headless
/// 组件测试的前置语义在 lib 侧先钉一半（startup 装配与三态文案），窗口呈现
/// 接线归 `tests/app_headless.rs`。
#[test]
fn run_startup_without_book_dir_renders_usage() {
    let _guard = SERIAL.lock().unwrap();
    // 缺席路径显式复位（headless 集成测试与本文件内的注入用例都可能先行运行，
    // 进程级槽位必须回到空态，缺席断言才与用例顺序无关）。
    crate::app::reset_book_assembly();
    assert_eq!(run_startup(), StartupView::MissingArg);
    assert_eq!(headline_text(&StartupView::MissingArg), "未指定账本库目录");
    assert_eq!(headline_text(&StartupView::Loading), "正在打开账本库…");
    assert_eq!(
        headline_text(&StartupView::Ready),
        "",
        "就绪态不走启动文案路径（表格视图承载）"
    );
    assert_eq!(
        headline_text(&StartupView::Failed {
            message: "账本库文件不存在".to_owned()
        }),
        "启动失败：账本库文件不存在"
    );
}

/// 会话句柄读取（app 槽位的 crate 内测试出口，随头注可见性）。
fn nsh_app_session() -> std::sync::Arc<ledger_infra::db::DbState> {
    crate::app::session_handle().expect("就绪时会话句柄已就位")
}

/// 注入装配入口后的启动竖井：引导成功 → 会话句柄就位 → 首页读得真实行集
/// （删除任一环——注入 / 引导 / 会话句柄 / 首页读——本用例即红，ADR-0087 接线
/// 断言；窗口呈现侧的对应断言归 `tests/app_headless.rs`）。
#[test]
fn run_startup_with_seeded_book_places_session_and_reads_first_page() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("startup");
    create_empty_book_file(&dir);
    seed_expense_series(&dir, 2);
    set_book_dir(dir.clone());
    assert_eq!(run_startup(), StartupView::Ready, "引导成功呈现就绪态");
    let db = nsh_app_session();
    let page = crate::boot::transaction_page(&db, 1).unwrap();
    assert_eq!(page.total, 2, "会话句柄可直读首页（读路径直调域服务）");
    assert_eq!(page.items.len(), 2);
}
