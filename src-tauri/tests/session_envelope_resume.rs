//! 会话信封记忆随 resume 签名记入的集成测试（issue #1395 / ADR-0098 决策 3
//! 修订注记）。
//!
//! **独立测试二进制**（不经 tests/commands 目标，先例 readonly_connection.rs /
//! sync_trigger_poll.rs）：三个被测入口（解锁、忘记口令重置、启动失败重置）
//! 都经 `resume_business_surface` 尾部真拉后台服务（`start_background_services`，
//! issue #961 唯一编排点），调度线程的单次拉起守卫是进程级单例——与
//! tests/commands 内刻意注入时机的 sync_trigger 接线测试在同一进程内互斥
//! （先行的拉起会让其注入失效）。
//!
//! 断言对准 `SessionEnvelope::current()` 形态（进程级会话记忆的用户可观察
//! 结果——自动轮次的封包依据），非源码形状（CONTEXT-testing〈断言强度〉）：
//! 删除 resume 体内信封写入，解锁与重置场景的形态断言即红。域单测
//!（sync-engine trigger/tests.rs）不动，不升 e2e。

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

use tauri::Manager;

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::{self};
use ledger_sync_engine::SessionEnvelope;
use tauri_app_lib::commands::boot::{BootCell, reset_after_startup_failure};
use tauri_app_lib::commands::encryption::{reset_after_forgotten_passphrase, unlock_encryption};

type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

/// `$HOME` 现场隔离（本二进制进程内一次；tests/commands/isolation 同型）：
/// mock runtime 的 `app_data_dir()` 由 `$HOME` 派生，重定向后默认数据目录
/// 落在进程专属临时区，不触真实用户目录。
fn isolate_home() {
    static ISOLATE: std::sync::Once = std::sync::Once::new();
    ISOLATE.call_once(|| {
        let root = std::env::temp_dir().join(format!("ledger-sessionenv-it-{}", db::new_uuid()));
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY：set_var 自 Rust 2024 起 unsafe；本函数是本测试二进制唯一的
        // `$HOME` 写入点，Once 保证至多执行一次、写入后不再变化。
        unsafe { std::env::set_var("HOME", &root) };
    });
}

/// 密文库 + 锁定现场（解锁屏 AwaitUnlock 同形）：mock 应用 + 引导登记态 +
/// 密文库（明文库建好即弃连接 → 原位转密文 → 清 .bak）+ 锁定门 + 占位连接对
///（门立起、占位对维持形状；readonly_connection 解锁场景同款）。库目录走
/// ScratchDir（issue #1645）：guard 随元组交调用方持有，用例结束整棵删除。
fn locked_encrypted_device_app(
    tag: &str,
    passphrase: &str,
) -> (AppHandle, tauri_app_lib::test_support::ScratchDir) {
    let dir = tauri_app_lib::test_support::ScratchDir::new(&format!("sessionenv-it-{tag}"));
    let app = tauri::test::mock_app();
    app.manage(BootCell::new(db::data_location::boot(&dir)));
    app.manage(EncryptionGate::new(true));
    app.manage(BootFailureGate::new());
    let db_path = dir.join(db::data_location::DB_FILE_NAME);
    drop(db::open_db_in(&dir).unwrap());
    ledger_infra::db::encryption::enable_encryption_for_file(&db_path, passphrase).unwrap();
    std::fs::remove_file(db_path.with_extension("db.bak")).unwrap();
    // 占位连接对：空目录文件库成对打开（维持 DbState 形状，非业务库）。
    let placeholder_dir = dir.join("placeholder-shape");
    std::fs::create_dir_all(&placeholder_dir).unwrap();
    app.manage(db::open_db_in(&placeholder_dir).unwrap());
    (app.handle().clone(), dir)
}

/// 启动失败现场（失败恢复屏同形）：失败门登记 + 占位连接对 + 引导登记态，
/// 库目录指向独立临时目录（重置在其内新建明文空库）。
fn failed_device_app() -> (AppHandle, tauri_app_lib::test_support::ScratchDir) {
    let dir = tauri_app_lib::test_support::ScratchDir::new("sessionenv-it-failed");
    let app = tauri::test::mock_app();
    app.manage(BootCell::new(db::data_location::boot(&dir)));
    app.manage(EncryptionGate::new(false));
    let gate = BootFailureGate::new();
    gate.set_failed(None);
    app.manage(gate);
    let placeholder_dir = dir.join("placeholder-shape");
    std::fs::create_dir_all(&placeholder_dir).unwrap();
    app.manage(db::open_db_in(&placeholder_dir).unwrap());
    (app.handle().clone(), dir)
}

/// 三起点顺序旅程（会话信封是进程级单例，场景必须串行）：
/// ① 解锁记密文 → ② 忘记口令重置记明文 → ③ 启动失败重置记明文。
///
/// 删除 resume 体内信封写入：① 无任何记入路径、会话残留明文回退即红；
/// ②③ 残留前值（②继承①的密文、③为直接置入的密文，见场景内注释）同样红。
#[tokio::test(flavor = "multi_thread")]
async fn resume_surfaces_declare_session_envelope_in_signature() {
    isolate_home();

    // ---------- 场景①：解锁（do_unlock → Encrypted(口令)） ----------
    {
        let (app, _dir) = locked_encrypted_device_app("unlock", "解锁主口令");
        unlock_encryption(app.clone(), "解锁主口令".into())
            .await
            .expect("解锁应成功");
        assert_eq!(
            SessionEnvelope::current(),
            SessionEnvelope::Encrypted("解锁主口令".into()),
            "解锁成功后本会话信封应记入密文形态（resume 签名声明，自动轮次据此封包）"
        );
    }

    // ---------- 场景②：忘记口令重置（→ Plaintext，新库是明文空库） ----------
    {
        let (app, _dir) = locked_encrypted_device_app("forgot-reset", "待重置主口令");
        reset_after_forgotten_passphrase(app.clone())
            .await
            .expect("忘记口令重置应成功");
        assert_eq!(
            SessionEnvelope::current(),
            SessionEnvelope::Plaintext,
            "忘记口令重置后本会话信封应记入明文形态（新库是明文空库、形态已知，\
             不再清空等待重新记入）"
        );
    }

    // ---------- 场景③：启动失败重置（→ Plaintext，该起点声明的不变量） ----------
    {
        let (app, _dir) = failed_device_app();
        // 会话先直接置密文形态：生产可达现场里失败态信封必为 None（可证 no-op），
        // 置值只为让断言可区分「该起点的明文声明生效」与「残留前值」——删除
        // resume 体内信封写入时本场景即红（e2e 步骤层直置进程态的同款先例）。
        SessionEnvelope::remember(SessionEnvelope::Encrypted("旧会话口令".into()));
        reset_after_startup_failure(app.clone())
            .await
            .expect("启动失败重置应成功");
        assert_eq!(
            SessionEnvelope::current(),
            SessionEnvelope::Plaintext,
            "启动失败重置后本会话信封应记入明文形态（新库是明文空库，声明的不变量）"
        );
    }
}
