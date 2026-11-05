//! 壳行为单测（issue #2017 / ADR-0087 断言归属：引导与读接线归本壳）：引导
//! 序列（处置四态收窄 + 成对建连）与交易总数读取（读路径直调域服务）。
//!
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
use ledger_infra::ids::new_uuid;
use ledger_transaction::{TransactionInput, TransactionKind};

use crate::app::{StartupView, headline_text, run_startup};
use crate::boot::{CODE_BOOK_MISSING, CODE_ENCRYPTED_UNSUPPORTED, boot_ledger, transaction_total};

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

/// 经域层公开写入口造一笔现金账户支出（余额刷新接缝的行文证据）。
fn seed_one_expense(dir: &Path, note: &str) {
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
    ledger_transaction::create(
        &conn,
        TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents: 2_500,
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
            note: Some(note.to_owned()),
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

#[test]
fn boot_opens_empty_book_and_reports_zero() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("empty");
    create_empty_book_file(&dir);
    let db = boot_ledger(&dir).unwrap();
    assert_eq!(transaction_total(&db).unwrap(), 0, "空账本交易总数为 0");
}

#[test]
fn boot_counts_seeded_transactions_via_domain_write_entries() {
    let _guard = SERIAL.lock().unwrap();
    let dir = scratch_book_dir("seeded");
    create_empty_book_file(&dir);
    seed_one_expense(&dir, "午餐");
    seed_one_expense(&dir, "晚餐");
    let db = boot_ledger(&dir).unwrap();
    assert_eq!(
        transaction_total(&db).unwrap(),
        2,
        "交易总数 = 公开写入口写入的未删除笔数（读路径直调域服务）"
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

/// 立桩竖井的端到端视图态（装配入口注入 → 引导 → 计数）：headless 组件测试的
/// 前置语义在 lib 侧先钉一半（startup 装配与文案），窗口呈现接线归
/// `tests/app_headless.rs`。
#[test]
fn run_startup_without_book_dir_renders_usage() {
    // 不设装配入口（本用例独占进程级 OnceLock 缺席路径：其余用例不注入，
    // set_book_dir 仅在 headless 集成测试消费——两测试二进制互不串扰）。
    assert_eq!(run_startup(), StartupView::MissingArg);
    assert_eq!(headline_text(&StartupView::MissingArg), "未指定账本库目录");
    assert_eq!(
        headline_text(&StartupView::Ready { total: 42 }),
        "交易总数：42 条"
    );
}
