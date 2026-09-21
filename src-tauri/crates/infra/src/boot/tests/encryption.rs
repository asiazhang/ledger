//! SQLCipher 引擎基座测试（issue #569 / ADR-0075）：依赖切换不变量
//! （未设密钥的连接保持明文）、建连主口令缝（密文库落盘为密文、凭同一
//! 主口令可再次打开、错误主口令被拒）、文件头探测三态。
//!
//! 用户可见的加密流程（解锁、转换、备份语义）由后续票的 BDD（真临时
//! 目录文件库）覆盖；本处钉住引擎基座的连接与文件级行为。
//!
//! 建连经产品建缝 `open_connection` / `open_connection_with_passphrase`（文件库
//! 不入测试工厂，ADR-0084 决策 3）；建库后的迁移补齐/重入经产品迁移缝
//! `migrations().to_latest`（即 `init_db` 的迁移核心，spec #728 / issue #754）。

use std::path::Path;

use tauri_app_lib::test_support::ScratchDir;

use rusqlite::{Connection, params};

use crate::db::encryption::{DbFileKind, SQLITE_HEADER_MAGIC, probe_file_kind};
use crate::db::{migrations, open_connection, open_connection_with_passphrase};

/// 暂存目录（ScratchDir guard，issue #1645）：drop（含 panic unwind）整棵删除。
fn temp_dir(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("db-enc-{tag}"))
}

fn read_header(db: &Path) -> [u8; 16] {
    use std::io::Read;
    let mut file = std::fs::File::open(db).unwrap();
    let mut header = [0u8; 16];
    file.read_exact(&mut header).unwrap();
    header
}

/// 交付不变量（issue #569）：未设密钥的连接保持明文——文件库建库
/// 落盘为明文魔数，建连与迁移行为与依赖切换前一致。
#[test]
fn connection_without_key_stays_plaintext() {
    let dir = temp_dir("plain");
    let db = dir.join("ledger.db");
    let mut conn = open_connection(&db).unwrap();
    migrations().to_latest(&mut conn).unwrap();
    drop(conn);

    assert_eq!(
        read_header(&db),
        SQLITE_HEADER_MAGIC,
        "未设密钥的库文件头应为明文魔数"
    );
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Plaintext);

    // 切换前既有的明文打开路径可继续打开使用。
    let mut reopened = open_connection(&db).unwrap();
    migrations().to_latest(&mut reopened).unwrap();
    let currencies: i64 = reopened
        .query_row("SELECT COUNT(*) FROM currencies", [], |r| r.get(0))
        .unwrap();
    assert_eq!(currencies, 11, "种子币种应原样可读");
}

/// 带主口令打开的库文件落盘为密文（头部非明文魔数），凭同一主口令可再次
/// 打开并读回数据（issue #569 / ADR-0075 决策 4）。
#[test]
fn passphrase_connection_writes_ciphertext_and_reopens_with_same_passphrase() {
    let dir = temp_dir("keyed");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection_with_passphrase(&db, "主口令-正确").unwrap();
        migrations().to_latest(&mut conn).unwrap();
        conn.execute("CREATE TABLE reopen_probe(name TEXT)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO reopen_probe(name) VALUES (?1)",
            params!["数据①"],
        )
        .unwrap();
    }

    // 密文库头部为随机盐，绝非明文魔数；探测判定为密文库。
    assert_ne!(
        read_header(&db),
        SQLITE_HEADER_MAGIC,
        "带主口令打开的库文件落盘不应是明文魔数"
    );
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Encrypted);

    // 凭同一主口令可再次打开，迁移幂等、数据读回。
    let mut conn = open_connection_with_passphrase(&db, "主口令-正确").unwrap();
    migrations().to_latest(&mut conn).unwrap();
    let name: String = conn
        .query_row("SELECT name FROM reopen_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "数据①");
}

/// 错误主口令打不开密文库：`PRAGMA key` 本身不校验，失败发生在首条读
/// 语句（SQLITE_NOTADB「file is not a database」）；口令错误 ≠ 库损坏
/// 的用户可见区分由文件头探测承担（ADR-0075 决策 5 的基座行为）。
#[test]
fn passphrase_connection_rejects_wrong_passphrase() {
    let dir = temp_dir("wrong-key");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection_with_passphrase(&db, "正确口令").unwrap();
        migrations().to_latest(&mut conn).unwrap();
    }
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Encrypted);

    let mut conn = open_connection_with_passphrase(&db, "错误口令").unwrap();
    let err = migrations().to_latest(&mut conn).unwrap_err();
    assert!(
        err.to_string().contains("file is not a database"),
        "错误主口令应以 not-a-database 失败，实际: {err}"
    );
}

// ---------------------------------------------------------------------------
// 整库加密转换与解锁（issue #570 / ADR-0075 决策 5/6）
// ---------------------------------------------------------------------------

use crate::db::encryption::{enable_encryption_for_file, unlock_db_file};
use crate::db::{check_integrity, open_connection_with_passphrase as reopen_with_key};

/// 在库中建一笔账户 + N 条种子交易（与真实写路径一致的 Writer 接缝，
/// 账户插入含缓存行不变量，ADR-0067）。
fn seed_transactions(conn: &Connection, count: usize) {
    use ledger_transaction::TransactionInput;
    use ledger_transaction::amount::TransactionKind;
    // 写路径副作用接缝接线（issue #1090）：本测试经产品建缝拿文件库连接（不入
    // 测试工厂，ADR-0084 决策 3），建库单点的注册不覆盖本处——dev-dependency 环
    // 下 tauri_app_lib 是另一份实例（静态与类型身份分离），落库前显式注册余额
    // 刷新实现（幂等，与 BDD world 同款纪律）。
    ledger_accounts::balance::install_balance_refresh_hook();
    // 交易域接缝接线（issue #1092 / #1180）：与测试工厂同形——本处经 Writer/行为层
    // 写入，六向实现（计划装配/商户/本位币/来源列反查/转换两腿/出资账户视图）经
    // 组合入口一次装入（幂等，进程级）。
    tauri_app_lib::transaction_wiring::install_all();
    let account_id = crate::db::new_uuid();
    // 工厂账户种子（归一签名，spec #728 / ADR-0084 决策 4）；裸种子绕过 Writer
    // 接缝，按 V017 迁移回填语义补建缓存行（ADR-0067）。
    tauri_app_lib::test_support::seed_account(conn, &account_id, "现金", "cash", "CNY", 0);
    ledger_accounts::balance::refresh_account_balances(conn, &[account_id.as_str()]).unwrap();
    for i in 0..count {
        let input = TransactionInput {
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Expense,
            amount_cents: 1000 + i as i64,
            currency_code: "CNY".into(),
            account_id: account_id.clone(),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            funding_account_id: None,
            note: Some(format!("种子交易 {i}")),
            date: "2026-03-01".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            idempotency_key: None,
            origin: None,
        };
        ledger_transaction::create_transaction_internal(conn, input).unwrap();
    }
}

fn count_transactions(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

/// 错误码提取（码化错误契约，ADR-0050）。
fn code_of(err: &crate::error::AppError) -> Option<&str> {
    match err {
        crate::error::AppError::Coded { code, .. } => Some(code),
        _ => None,
    }
}

/// 开启加密——整库一次性转换往返：转换成功、旧库保留为 .bak 明文副本、
/// 新库为密文、凭新口令可打开且数据完整、schema 版本保持（迁移不重跑）。
#[test]
fn enable_encryption_converts_plaintext_db_and_preserves_data() {
    let dir = temp_dir("convert");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
        seed_transactions(&conn, 3);
    }
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Plaintext);

    enable_encryption_for_file(&db, "correct horse").unwrap();

    // 原库文件按重置命名语义保留为 .bak，且仍是明文库。
    let bak = db.with_extension("db.bak");
    assert!(bak.exists(), "原明文库应保留为 .bak 副本");
    assert_eq!(probe_file_kind(&bak).unwrap(), DbFileKind::Plaintext);
    // 原位库文件已变为密文库。
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Encrypted);

    // 凭新口令打开：数据完整、迁移幂等（user_version 已对齐，不再变化）。
    let mut conn = reopen_with_key(&db, "correct horse").unwrap();
    migrations().to_latest(&mut conn).unwrap();
    assert_eq!(count_transactions(&conn), 3, "转换后交易数据应完整无损");
    check_integrity(&conn).unwrap();
}

/// 「目录置只读（0o555）触发转换失败」手段是否可用（issue #791）：
/// 仅非 root Unix 成立——root 凭 CAP_DAC_OVERRIDE 无视权限位，非 Unix
/// 无权限位可依；不可用时测试显式跳过，不假红（CI 的 cargo test 仅在
/// Linux 非 root runner 运行，覆盖不受影响）。
fn readonly_trigger_available() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // /proc/self 的属主 uid 即本进程身份（测试进程不切换 uid，零
        // 新依赖）；无 /proc 的 Unix 平台按非 root 处理（票面针对
        // Linux root）。
        match std::fs::metadata("/proc/self") {
            Ok(meta) => meta.uid() != 0,
            Err(_) => true,
        }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// 转换中途失败时原库原样保留：导出无法落盘（目标目录不可写）时，
/// 原库字节不变、仍是明文库、无残留临时产物——不存在半加密状态。
#[test]
fn enable_encryption_failure_keeps_original_db_intact() {
    if !readonly_trigger_available() {
        eprintln!(
            "跳过：目录只读触发手段在当前环境不可用（root 架空权限位或非 Unix），失败路径由 Linux 非 root CI 覆盖（issue #791）"
        );
        return;
    }

    let dir = temp_dir("convert-fail");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
        seed_transactions(&conn, 2);
    }
    let original_bytes = std::fs::read(&db).unwrap();

    // 目录置为不可写，转换临时文件（temp_sibling）无法落盘，ATTACH 导出失败。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    }

    let err = enable_encryption_for_file(&db, "pw").unwrap_err();
    let _ = code_of(&err); // 失败形态不拘（IO/Db），转换失败本身就是断言对象。

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // 原库字节不变、仍为明文库，可正常打开且数据完整。
    assert_eq!(
        std::fs::read(&db).unwrap(),
        original_bytes,
        "原库文件字节应保持不变"
    );
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Plaintext);
    let conn = open_connection(&db).unwrap();
    assert_eq!(count_transactions(&conn), 2, "明文库仍可用且数据完整");
    // 不存在半加密状态：目录中无 .bak 副本、无转换临时残留。
    assert!(!db.with_extension("db.bak").exists());
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        leftovers
            .iter()
            .all(|name| !name.starts_with(".ledger.db.")),
        "不应残留转换临时文件: {leftovers:?}"
    );
}

/// 解锁：正确口令成功且数据完整；错误口令报码化错误（可重试）且不改动
/// 文件任何字节；同一文件可无限次重试，最终凭正确口令解锁成功。
#[test]
fn unlock_accepts_correct_passphrase_and_retries_after_wrong() {
    let dir = temp_dir("unlock");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
        seed_transactions(&conn, 3);
    }
    enable_encryption_for_file(&db, "正确口令").unwrap();
    let encrypted_bytes = std::fs::read(&db).unwrap();

    // 错误口令：码化错误、可重试语义；文件字节不变。
    let err = unlock_db_file(&db, "错误口令").unwrap_err();
    assert_eq!(
        code_of(&err),
        Some("encryption.passphrase-incorrect"),
        "错误口令应报口令错误（而非文件损坏），实际: {err}"
    );
    assert_eq!(
        std::fs::read(&db).unwrap(),
        encrypted_bytes,
        "失败重试不得改动库文件"
    );

    // 无限重试：同一文件上再次尝试，正确口令解锁成功、数据完整。
    let conn = unlock_db_file(&db, "正确口令").unwrap();
    assert_eq!(count_transactions(&conn), 3);
    check_integrity(&conn).unwrap();
}

/// 解锁的状态区分：明文库上解锁报「不是加密状态」而非口令错误，
/// 避免把状态漂移误报成可无限重试的口令问题。
#[test]
fn unlock_on_plaintext_file_reports_not_encrypted() {
    let dir = temp_dir("unlock-plain");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
    }
    let err = unlock_db_file(&db, "pw").unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.not-encrypted"));
}

/// 开启加密的口令守卫：空口令拒绝。
#[test]
fn enable_encryption_rejects_empty_passphrase() {
    let dir = temp_dir("convert-empty");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
    }
    let err = enable_encryption_for_file(&db, "").unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.passphrase-empty"));
    // 已是密文库时拒绝再次开启。
    enable_encryption_for_file(&db, "pw").unwrap();
    let err = enable_encryption_for_file(&db, "pw2").unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.not-plaintext"));
}

/// 文件头探测三态判定正确（明文库 / 密文库 / 空文件），且只依赖文件
/// 本身、不依赖任何库外引导状态（issue #569 / ADR-0075 决策 4）。
#[test]
fn probe_detects_plaintext_encrypted_and_empty() {
    let dir = temp_dir("probe");

    // 明文库。
    let plain = dir.join("plain.db");
    let mut conn = open_connection(&plain).unwrap();
    migrations().to_latest(&mut conn).unwrap();
    drop(conn);
    assert_eq!(probe_file_kind(&plain).unwrap(), DbFileKind::Plaintext);

    // 密文库。
    let encrypted = dir.join("encrypted.db");
    let mut conn = open_connection_with_passphrase(&encrypted, "口令").unwrap();
    migrations().to_latest(&mut conn).unwrap();
    drop(conn);
    assert_eq!(probe_file_kind(&encrypted).unwrap(), DbFileKind::Encrypted);

    // 空文件（0 字节）→ 按明文新装对待。
    let empty = dir.join("empty.db");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(probe_file_kind(&empty).unwrap(), DbFileKind::Empty);

    // 文件不存在 → 同空文件（新装语义）。
    assert_eq!(
        probe_file_kind(&dir.join("missing.db")).unwrap(),
        DbFileKind::Empty
    );
}

// ---------------------------------------------------------------------------
// 忘记口令重置守卫（issue #573）
// ---------------------------------------------------------------------------

use crate::db::encryption::reset_encrypted_db_file;

/// 重置只对密文库生效：明文库拒绝且现场不变（原库在位、无副本产生）。
/// 密文库上的重置产物（副本存在、头部为密文、新库为明文）由 BDD 钉住。
#[test]
fn reset_rejects_non_encrypted_file() {
    let dir = temp_dir("reset-guard");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
    }
    let err = reset_encrypted_db_file(&db).unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.not-encrypted"));
    assert!(db.exists(), "明文库应原样保留");
    assert!(!db.with_extension("db.bak").exists(), "不应产生副本");

    // 库文件不存在（新装语义之外的逃生门误用）同样拒绝。
    let missing = dir.join("missing.db");
    let err = reset_encrypted_db_file(&missing).unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.db-missing"));
}

// ---------------------------------------------------------------------------
// 外来形态明文库：保留字节探测与启动归一化（issue #1453）
// ---------------------------------------------------------------------------

use crate::db::encryption::{normalize_plaintext_db_file, probe_file};
use crate::test_utils::{
    FOREIGN_RESERVED_BYTES, read_reserved_byte, write_foreign_form_plaintext_db,
};

/// 夹具自证（issue #1453）：按字节构造的一页空库 + SQLite 自身写表写行，得到的
/// 是**合法**（可打开、完整性检查通过、读写正常）且保留字节 = 12 的明文库——
/// 与现场库（外部工具写坏保留字节的合法库）同形。夹具若不成立，本组测试就
/// 证明不了任何事，故这条先钉住夹具本身。
#[test]
fn foreign_form_plaintext_fixture_is_a_valid_db_with_reserved_bytes() {
    let dir = temp_dir("foreign-fixture");
    let db = dir.join("ledger.db");
    write_foreign_form_plaintext_db(&db, 3);

    let probe = probe_file(&db).unwrap();
    assert_eq!(probe.kind, DbFileKind::Plaintext);
    assert_eq!(probe.reserved_bytes, Some(FOREIGN_RESERVED_BYTES));
    assert_eq!(read_reserved_byte(&db), FOREIGN_RESERVED_BYTES);

    // 合法：明文建连、完整性检查通过、数据可读。
    let conn = open_connection(&db).unwrap();
    check_integrity(&conn).unwrap();
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM fixture_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 3);
}

/// 根因级证明（issue #1453）：外来形态明文库在归一化前**无法** `VACUUM INTO`
///（SQLCipher 的无 KEY ATTACH 推断路径报 `unable to open database`），归一化后
/// 可执行且产物保留字节 = 0。前一半同时是夹具复现性的证明：该语句若在此不再
/// 失败（夹具失效或引擎行为变化），本用例即变红，附带的失败形态即现场根因。
#[test]
fn foreign_form_db_breaks_vacuum_into_until_normalized() {
    let dir = temp_dir("foreign-vacuum");
    let db = dir.join("ledger.db");
    write_foreign_form_plaintext_db(&db, 3);
    let snapshot = dir.join("snapshot.db");
    let vacuum_into = |path: &Path| format!("VACUUM INTO '{}'", path.display());

    // 归一化前：备份/检查点产出的同款语句必然失败（现场根因）。
    {
        let conn = open_connection(&db).unwrap();
        // 只断言「失败」这一可观察行为，不钉驱动报错文本（SQLCipher/SQLite 版本
        // 升级会改措辞，断言措辞属实现形状守护）。
        let attempt = conn.execute_batch(&vacuum_into(&snapshot));
        assert!(
            attempt.is_err(),
            "外来形态库应复现「不带 KEY 的 ATTACH 打不开」根因（夹具失效即在此变红）"
        );
    }
    std::fs::remove_file(&snapshot).ok();

    normalize_plaintext_db_file(&db).unwrap();

    // 归一化后：同一条语句可执行，且产物本身是干净形态（不会把污染传给备份）。
    {
        let conn = open_connection(&db).unwrap();
        conn.execute_batch(&vacuum_into(&snapshot)).unwrap();
    }
    assert_eq!(read_reserved_byte(&snapshot), 0);
}

/// 归一化正向（issue #1453 验收判据）：畸形夹具经归一化后保留字节归零、
/// `integrity_check` 通过、`sqlite_master` 对象数与 `user_version` 与源一致、
/// 数据完整；旧形态留 `.bak` 副本。
#[test]
fn normalize_rewrites_foreign_form_db_and_preserves_data() {
    let dir = temp_dir("normalize");
    let db = dir.join("ledger.db");
    write_foreign_form_plaintext_db(&db, 3);
    // 非零 user_version：钉住「显式对齐」而不是依赖导出函数顺带复制。
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("PRAGMA user_version = 42").unwrap();
    }
    let objects_before: i64 = {
        let conn = Connection::open(&db).unwrap();
        conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
            .unwrap()
    };

    normalize_plaintext_db_file(&db).unwrap();

    assert_eq!(read_reserved_byte(&db), 0, "归一化后保留字节应归零");
    assert_eq!(probe_file(&db).unwrap().reserved_bytes, Some(0));
    let conn = open_connection(&db).unwrap();
    check_integrity(&conn).unwrap();
    let objects_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
        .unwrap();
    assert_eq!(objects_after, objects_before, "对象数应与源一致");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(user_version, 42, "schema 版本应与源一致");
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM fixture_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 3, "数据应完整保留");

    // 转换前形态保留为 `.bak` 副本（仍是外来形态，可留作排查）。
    let bak = db.with_extension("db.bak");
    assert!(bak.exists(), "旧形态应按既有转换语义保留 .bak 副本");
    assert_eq!(read_reserved_byte(&bak), FOREIGN_RESERVED_BYTES);
}

/// 回归保护（issue #1453）：应用自有形态的明文库零改动——文件一个字节都不
/// 重写、不产生 `.bak` 副本（归一化只在命中外来形态时发生）。
#[test]
fn normalize_leaves_app_owned_plaintext_db_untouched() {
    let dir = temp_dir("normalize-owned");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
        seed_transactions(&conn, 2);
    }
    let before = std::fs::read(&db).unwrap();

    normalize_plaintext_db_file(&db).unwrap();

    assert_eq!(
        std::fs::read(&db).unwrap(),
        before,
        "应用自有形态的明文库不应被重写"
    );
    assert!(!db.with_extension("db.bak").exists(), "不应产生副本");
}

/// 归一化失败（导出落不了盘）时原库原样保留、无任何残留：与整库转换共用同一
/// 失败路径（ADR-0075 决策 6 的既有原子性语义）。启动编排据此把失败登记为
/// 启动失败（码化错误 `boot.db-normalize-failed` 由壳层 `boot_sequence` 映射，
/// 交既有失败恢复屏），用户不会「带病运行」。
#[test]
fn normalize_failure_keeps_original_db_intact() {
    if !readonly_trigger_available() {
        eprintln!(
            "跳过：目录只读触发手段在当前环境不可用（root 架空权限位或非 Unix），失败路径由 Linux 非 root CI 覆盖（issue #791）"
        );
        return;
    }

    let dir = temp_dir("normalize-fail");
    let db = dir.join("ledger.db");
    write_foreign_form_plaintext_db(&db, 2);
    let original_bytes = std::fs::read(&db).unwrap();

    // 目录置为不可写，归一化的临时产物（temp_sibling）无法落盘。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    }

    normalize_plaintext_db_file(&db).unwrap_err();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // 原库字节不变、仍是外来形态、可正常打开且数据完整；无 `.bak`、无临时残留。
    assert_eq!(
        std::fs::read(&db).unwrap(),
        original_bytes,
        "归一化失败不得改动原库任何字节"
    );
    assert_eq!(read_reserved_byte(&db), FOREIGN_RESERVED_BYTES);
    assert!(!db.with_extension("db.bak").exists(), "失败不得产生副本");
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        leftovers
            .iter()
            .all(|name| !name.starts_with(".ledger.db.")),
        "不应残留归一化临时文件: {leftovers:?}"
    );
    let conn = open_connection(&db).unwrap();
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM fixture_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 2, "原库数据完整可读");
}
