//! NSH 两测试面共享的造数助手（issue #2027）：lib 单测（`src/tests.rs`）与
//! headless 组件测试（`tests/app_headless.rs`）各自维护的同形助手收拢于此——
//! 账本暂存目录、造数日期步进（[`test_date`]）、测试现金账户、最小支出输入
//! （24 字段 `TransactionInput`，含分类维度变体 [`expense_input_with_category`]）
//! 与支出系列三入口（按笔数 / 按备注 / 按日期）。
//!
//! 为什么住 `tests/` 豁免区、两侧 `#[path]` 载入（票内三选一裁决）：
//! - 根包 `test_support` 统一工厂立桩时链接旧壳 tauri（GTK3，与 NSH 的 GTK4
//!   同进程符号互踩），当时本 crate 不可消费（#2018
//!   提交信息已记录该偏离）；#2083 切轨后旧壳退役、互踩消失，本形态保留的
//!   现理由不再是链接约束——统一工厂的准入规则只收跨 ≥2 域重复的域夹具
//!   （ADR-0084 决策 1），壳测试特有的 NSH 夹具不入厂，「建库走产品开库入口」
//!   的分层纪律保留——「通用能力上收测试辅助层」仍不适用；
//! - lib 的 `#[cfg(test)]` 模块对外挂集成测试不可见，crate 不能 dev-depend
//!   自己；立在 crate 内的 `#[doc(hidden)]` 支撑模块会把测试代码带进产品 bin
//!   （LTO 前），与「测试代码不进产品构建」的既有形态（bin `test = false`、
//!   六件套只 `cfg(test)` 放行）相悖；
//! - 本形态零产品成本：文件只随两侧测试目标编译（lib 侧在 `#[cfg(test)] mod
//!   tests` 的 `#[path]` 子模块、集成侧在测试二进制），产品构建不触碰；结构
//!   守门对 `tests.rs` 文件与 `tests/` 目录（无论多深）整体豁免（ADR-0056
//!   决策 5），`#[path]` 载入在 e2e 测试面已是既定形态。
//!
//! 载入形态（两侧 `#[path]` include）：
//! - lib：`src/tests.rs` 内 `#[path = "../tests/support/seed.rs"] mod seed;`
//!   （`#[path]` 相对所在文件 `src/`）；
//! - 集成：`tests/` 下 6 个消费目标（app_headless、sync_trigger、
//!   sync_trigger_write 与市场三车道）内 `#[path = "support/seed.rs"] mod seed;`。
//!
//! 本文件因此**只引用 crate 外符号**（域 crate / 基础设施 / std），不引用
//! `nsh_app::`——两份编译上下文的 crate 身份不同（lib 测试构建内是 `crate::`，
//! 集成二进制内是 extern），需要 [`nsh_app::boot::boot_ledger`] 开库的目录级
//! 入口由两侧薄壳包装（各自调本面可达的 boot），本文件只收 db 级形态。
//!
//! 边界：只承载本 crate 两测试面的造数形态（nsh 特有夹具），通用能力仍归根包
//! 测试辅助层（不入厂理由见上文准入规则；#2018 立桩时的「本 crate 不可达」
//! 偏离已随 #2083 切轨消失）；每个测试二进制各得一份副本
//! （`#[path]` 双编译），进程级串行锁仍归各面（headless 面 `SERIAL` 静态、
//! lib 面 `app::test_assembly_guard`，#2086 携毒化守卫），「两测试二进制独立
//! 进程互不串扰」语义不变。暂存目录前缀统一归 `ledger-test-` 闭集
//! 第二段（残留一条 `rm -rf /tmp/ledger-test-*` 识别清理）。

use std::fs;
use std::path::{Path, PathBuf};

use ledger_accounts::{AccountInput, AccountType};
use ledger_infra::db::DbState;
use ledger_infra::error::AppError;
use ledger_infra::ids::new_uuid;
use ledger_transaction::{TransactionInput, TransactionKind};

/// 测试账本暂存目录（UUID 后缀，测试互不串库；进程退出由 OS 回收——共享原文
/// 只引用 crate 外符号，根包 `ScratchDir` 两侧指名不同形（lib 面
/// `crate::test_support`、集成面 `nsh_app::test_support`）无法统一引用；旧
/// 「GTK 符号互踩不可达」理由已随 #2083 切轨消失）。
pub fn make_book_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ledger-test-nsh-book-{tag}-{}", new_uuid()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// 建一个空账本库文件（0 字节：产品「新装」语义——已有文件上的空库，引导建连
/// 即完成迁移）；不经 init_db（建库归产品开库入口，check-test-support 规则 1）。
pub fn create_empty_book_file(dir: &Path) {
    fs::write(dir.join("ledger.db"), b"").unwrap();
}

/// 造数日期：2026-01-01 起（平年月份表逐日推进，插入序 = 日期升序）。
pub fn test_date(index: usize) -> String {
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

/// 造一个现金账户「测试现金」（种子系列的共用账户，域公开写入口；账户名不与
/// 搜索断言关键词相交，供搜索读接线的原文命中造数面）。
pub fn seed_cash_account(db: &DbState) -> String {
    let conn = db
        .conn
        .lock()
        .map_err(|e| AppError::Db(e.to_string()))
        .unwrap();
    ledger_accounts::create_account(
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
    .unwrap()
}

/// 一笔现金支出输入（公开写入口的最小载荷，日期与金额可指定）。
pub fn expense_input(account: &str, date: &str, amount_cents: i64, note: &str) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Expense,
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
        refund_of_transaction_id: None,
        note: Some(note.to_owned()),
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

/// 一笔现金支出输入的分类维度变体（issue #2059）：支出可挂分类、备注可
/// 缺省——分类路径装配读的造数入口；其余字段随 [`expense_input`] 单源，
/// 调用面不再内联 24 字段字面量。
///
/// 现仅 lib 面（`src/tests.rs`）消费；headless 面支出变体走本地
/// `bare_expense_input` 底座，尚未消费本入口。`#[path]` 双编译下单面子集
/// 消费是共享原文的既定代价，按 `s3_stub.rs` facade 同形放行 dead_code。
#[allow(dead_code)]
pub fn expense_input_with_category(
    account: &str,
    date: &str,
    amount_cents: i64,
    category_id: Option<&str>,
    note: Option<&str>,
) -> TransactionInput {
    TransactionInput {
        category_id: category_id.map(str::to_owned),
        note: note.map(str::to_owned),
        // 底座 note 载荷由外层 struct update 唯一决定，传固定空串占位：
        // 不随外层 note 派生（旧写法 None 时先造 Some("") 再被覆盖，属死载荷）。
        ..expense_input(account, date, amount_cents, "")
    }
}

/// 经域层公开写入口在同一账户上造一列支出（余额刷新接缝的行文证据；备注
/// 「造数第 N 笔」，插入序 = 日期升序，供分页排序断言）。
pub fn seed_expense_series(db: &DbState, count: usize) {
    let notes: Vec<String> = (0..count)
        .map(|index| format!("造数第 {index} 笔"))
        .collect();
    let refs: Vec<&str> = notes.iter().map(String::as_str).collect();
    seed_expense_notes(db, &refs);
}

/// 经域层公开写入口在同一账户上造一列带指定备注的支出（每笔 ¥25，日期按
/// [`test_date`] 递增；搜索读接线断言的原文命中造数面）。
pub fn seed_expense_notes(db: &DbState, notes: &[&str]) {
    let account = seed_cash_account(db);
    let conn = db
        .conn
        .lock()
        .map_err(|e| AppError::Db(e.to_string()))
        .unwrap();
    for (index, note) in notes.iter().enumerate() {
        ledger_transaction::create(
            &conn,
            expense_input(&account, &test_date(index), 2_500, note),
        )
        .unwrap();
    }
}

/// 在指定日期造一笔指定金额的现金支出（期间维度读路径的造数入口）。
pub fn seed_expense_on_date(db: &DbState, date: &str, amount_cents: i64, note: &str) {
    let account = seed_cash_account(db);
    let conn = db
        .conn
        .lock()
        .map_err(|e| AppError::Db(e.to_string()))
        .unwrap();
    ledger_transaction::create(&conn, expense_input(&account, date, amount_cents, note)).unwrap();
}
