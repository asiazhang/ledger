//! NSH 两测试面共享的造数助手（issue #2027）：lib 单测（`src/tests.rs`）与
//! headless 组件测试（`tests/app_headless.rs`）各自维护的同形助手收拢于此——
//! 账本暂存目录、造数日期步进（[`test_date`]）、测试现金账户、最小支出输入
//! （24 字段 `TransactionInput`）与支出系列三入口（按笔数 / 按备注 / 按日期）。
//!
//! 为什么住 `tests/` 豁免区、两侧 `#[path]` 载入（票内三选一裁决）：
//! - 根包 `test_support` 统一工厂立桩时链接旧壳 tauri（GTK3，与 NSH 的 GTK4
//!   同进程符号互踩，`src-app/Cargo.toml` 头注），当时本 crate 不可消费（#2018
//!   提交信息已记录该偏离）；#2083 切轨后互踩消失，「建库走产品开库入口」的
//!   既有分层纪律保留——「通用能力上收测试辅助层」仍不适用；
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
//! - 集成：`tests/app_headless.rs` 内 `#[path = "support/seed.rs"] mod seed;`。
//!
//! 本文件因此**只引用 crate 外符号**（域 crate / 基础设施 / std），不引用
//! `nsh_app::`——两份编译上下文的 crate 身份不同（lib 测试构建内是 `crate::`，
//! 集成二进制内是 extern），需要 [`nsh_app::boot::boot_ledger`] 开库的目录级
//! 入口由两侧薄壳包装（各自调本面可达的 boot），本文件只收 db 级形态。
//!
//! 边界：只承载本 crate 两测试面的造数形态（nsh 特有夹具），通用能力仍归根包
//! 测试辅助层（本 crate 不可达，#2018 偏离记录）；每个测试二进制各得一份副本
//! （`#[path]` 双编译），进程级单例（SERIAL 串行锁）仍归各面文件，「两测试
//! 二进制独立进程互不串扰」语义不变。暂存目录前缀统一归 `ledger-test-` 闭集
//! 第二段（残留一条 `rm -rf /tmp/ledger-test-*` 识别清理）；账本目录发放自带
//! RAII guard 与 >24h 过期清扫（#2112），正常运行零残留。

use std::fs;
use std::path::{Path, PathBuf};

use ledger_accounts::{AccountInput, AccountType};
use ledger_infra::db::DbState;
use ledger_infra::error::AppError;
use ledger_infra::ids::new_uuid;
use ledger_transaction::{TransactionInput, TransactionKind};

/// 测试账本暂存目录 guard（UUID 后缀，测试互不串库）：构造即建
/// `ledger-test-nsh-book-{tag}-{uuid}/`，drop（含 panic unwind）整棵删除——
/// 「进程退出由 OS 回收」是错误假设（OS 从不回收 /tmp 目录），残留随用例数
/// 堆积把 tmpfs 写满即全量 disk-I/O 假红（#2112 事故）。两测试面经 `#[path]`
/// 双编译共享本文件（crate 身份不同），不可直接消费根包 `test_support::
/// ScratchDir`，故在此自持同形 guard（建目录 + 生命周期收尾一体）。
#[derive(Debug)]
pub struct BookDir {
    root: PathBuf,
}

impl BookDir {
    pub fn path(&self) -> &Path {
        &self.root
    }
}

impl std::ops::Deref for BookDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.root
    }
}

impl AsRef<Path> for BookDir {
    fn as_ref(&self) -> &Path {
        &self.root
    }
}

impl Drop for BookDir {
    // 清理失败静默吸收：清理失败不得变成测试失败的噪声源；panic unwind 时
    // 照常 drop，红测不留残留。
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// 过期暂存残留清扫（#2112）：删除 `/tmp` 下 `ledger-test-` 前缀且修改时间
/// 早于 24h 前的目录。只清「明显过期 + 本仓前缀」双条件的残留（异常终止的
/// 历史累积），绝不触碰并发会话的活跃目录——活跃目录的 mtime 必然新鲜。
/// 读目录/删目录失败一律静默吸收，清扫永不成为测试失败的噪声源。
fn sweep_stale_scratch_dirs() {
    const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with("ledger-test-") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        let Ok(age) = modified.elapsed() else {
            continue;
        };
        if age >= STALE_AFTER {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// 测试账本暂存目录（发放时执行 [`sweep_stale_scratch_dirs`] 过期清扫；guard
/// 语义见 [`BookDir`]）。
pub fn make_book_dir(tag: &str) -> BookDir {
    sweep_stale_scratch_dirs();
    let root = std::env::temp_dir().join(format!("ledger-test-nsh-book-{tag}-{}", new_uuid()));
    fs::create_dir_all(&root).unwrap();
    BookDir { root }
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

#[cfg(test)]
mod selftest {
    use super::*;

    /// 回拨目录 mtime（清扫用例的「明显过期」现场制造面）。
    fn backdate_mtime_hours(path: &Path, hours: u64) {
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(hours * 60 * 60);
        let file = fs::OpenOptions::new()
            .read(true)
            .open(path)
            .expect("打开目录以回拨 mtime");
        file.set_times(std::fs::FileTimes::new().set_modified(old))
            .expect("回拨目录 mtime");
    }

    /// 同一 tag 两次发放必不同目录（uuid 后缀），且两目录并存（并发用例互不
    /// 串库的前提）； uuid 唯一性丢失本断言红。
    #[test]
    fn book_dirs_are_unique_and_coexist() {
        let a = make_book_dir("selftest-coexist");
        let b = make_book_dir("selftest-coexist");
        assert_ne!(a.path(), b.path(), "同 tag 两次发放应得不同目录");
        assert!(a.path().is_dir());
        assert!(b.path().is_dir(), "两个 guard 并存期各自目录都应在位");
    }

    /// 清理契约：drop（含 panic unwind）整棵删除——删掉 [`BookDir`] 的 `Drop`
    /// 实现（或改成 no-op）本断言红。
    #[test]
    fn book_dir_is_removed_on_drop() {
        let dir = make_book_dir("selftest-drop");
        let root = dir.path().to_path_buf();
        fs::write(root.join("ledger.db"), b"").unwrap();
        assert!(root.exists());
        drop(dir);
        assert!(!root.exists(), "drop 后整棵暂存目录应消失");
    }

    /// panic unwind 照常清理：红测不留残留的承诺；去掉 `Drop` 实现本断言红。
    #[test]
    fn panic_unwind_still_cleans_up() {
        let dir = make_book_dir("selftest-unwind");
        let root = dir.path().to_path_buf();
        assert!(root.exists());
        let joined = std::thread::spawn(move || {
            let _guard = dir;
            panic!("unwind 经过的现场");
        })
        .join();
        assert!(joined.is_err(), "现场线程应 panic");
        assert!(!root.exists(), "panic unwind 后暂存目录不应残留");
    }

    /// 发放即清扫（#2112 接线）：在位目录回拨到 >24h 后再发放一本，过期残留
    /// 应被清扫删除——删除 [`make_book_dir`] 内的 [`sweep_stale_scratch_dirs`]
    /// 接线本断言红。同时验证双边界：未过期的目录与非本仓前缀的目录不得被
    /// 触碰（并发会话的活跃目录 mtime 必然新鲜，靠双条件保证绝不误删）。
    #[test]
    fn issuing_a_book_dir_sweeps_stale_residue_only() {
        let stale = make_book_dir("selftest-sweep-stale");
        backdate_mtime_hours(stale.path(), 25);
        let fresh = make_book_dir("selftest-sweep-fresh");
        let foreign =
            std::env::temp_dir().join(format!("not-ledger-test-sweep-{}", std::process::id()));
        fs::create_dir_all(&foreign).unwrap();

        make_book_dir("selftest-sweep-trigger");

        assert!(fresh.path().exists(), "未过期的暂存目录不得被清扫");
        assert!(foreign.exists(), "非本仓前缀的目录不得被清扫");
        fs::remove_dir_all(&foreign).unwrap();
        assert!(!stale.path().exists(), ">24h 的本仓前缀残留应被清扫");
    }
}
