//! 多端同步触发旅程（issue #2080，headless）：触发闭集的用户可观察面——打开即
//! 同步、写后去抖即时同步与轮次失效广播。独立测试二进制（旧壳 sync_trigger.rs
//! 与 sync_trigger_poll.rs 分进程同因）：调度线程单次拉起守卫与写信号通道是
//! 进程级单例，本二进制唯一场景即首个 wire 调用方，触发接线断言不被同进程
//! 其他 run_startup 场景抢先拉起的调度（默认时机、旧会话连接）污染。
//!
//! 进程内起真实视图树（与 tests/app_headless.rs 同形态）：断言只写用户可见
//! 结果——对端设备的账目在打开后 / 本机记账后自动出现在交易列表（ADR-0087，
//! 不对准线程或函数调用形状）。真 S3 桩跑真实轮次（tests/support/s3_stub.rs
//! facade，旧壳 tests/commands/sync_channel.rs 同现场形态）。
//!
//! 桌面形态显式置定、装配入口注入测试库路径、造数经域层公开写入口（与
//! app_headless 同纪律）；表格/表单操作助手为 app_headless 同名助手在本
//! 二进制的副本（各测试二进制各得一份副本，seed.rs #[path] 同款形态）。

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

use std::time::Duration;

use ledger_accounts::{AccountInput, AccountType};
use nsh_engine::{EngineConfig, Environment, Idiom, Os, Points, Root, Size};
use nsh_testing::Headless;

// 造数助手共享 seed.rs（issue #2027 纪律：只引用 crate 外符号，两侧 #[path]
// 载入）；本二进制只消费其子集，按载入点放行 dead_code（各二进制各得一份副本
// 的既定代价）。
#[allow(dead_code)]
#[path = "support/seed.rs"]
mod seed;

// 同步触发测试的 S3 桩 facade（模块名必须是 test_support：s3.rs 内
// `use crate::test_support::ScratchDir` 以 crate 根定位，见 support/s3_stub.rs
// 头注）。
#[path = "support/s3_stub.rs"]
mod test_support;

/// 与 `wait_for` 缺省同一超时（nsh-testing 的 `wait_for_gone` 需显式超时）。
const DEFAULT_WAIT: Duration = Duration::from_secs(5);

/// 桌面形态 headless 配置（1280×720；与 app_headless::desktop_config 同形副本）。
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

/// 测试账本（产品开库入口建库；共享助手 [`seed`]）。
fn make_book(tag: &str) -> std::path::PathBuf {
    let dir = seed::make_book_dir(tag);
    seed::create_empty_book_file(&dir);
    dir
}

/// 落位交易列表视图（app_headless 同名助手副本：等侧栏就绪 → 点侧栏交易项 →
/// 等表格挂载）。
fn open_transactions(app: &mut Headless) {
    app.wait_for("id:nav-transactions").unwrap();
    app.press("id:nav-transactions").unwrap();
    app.wait_for("id:tx-table").unwrap();
}

/// 打开新增表单（app_headless 同名助手副本）。
fn open_create_form(app: &mut Headless, item_id: &str, title: &str) {
    app.press("id:tx-add").unwrap();
    app.wait_for(item_id).unwrap();
    app.press(item_id).unwrap();
    app.wait_for_within(&format!("text:{title}"), DEFAULT_WAIT)
        .unwrap();
}

/// 在账户下拉中选中唯一候选（app_headless 同名助手副本：造数面保证单账户；
/// 选项行锚点 + 行内文本命中可按压件）。
fn pick_only_account(app: &mut Headless) {
    app.press("id:tx-form-account").unwrap();
    app.wait_for("id:tx-form-account-option").unwrap();
    app.press("id:tx-form-account-option >> text:测试现金")
        .unwrap();
    app.settle();
}

/// 对端设备的轮次连接源（boot.rs ShellRoundConn 同形最小面）：写连接槽包成
/// 轮次分段接缝——每个数据库段短暂取一次连接、用完即还，网络段不消费连接。
struct PeerRoundConn {
    conn: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
    key: u64,
}

impl ledger_sync_engine::RoundConn for PeerRoundConn {
    fn with_connection<R, F>(
        &self,
        _segment: ledger_sync_engine::ConnSegment,
        use_connection: F,
    ) -> ledger_infra::error::Result<R>
    where
        F: FnOnce(&rusqlite::Connection) -> ledger_infra::error::Result<R>,
    {
        let hold_started = std::time::Instant::now();
        let guard = self
            .conn
            .lock()
            .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))?;
        let result = use_connection(&guard);
        ledger_infra::db::probe_lock_hold(hold_started.elapsed());
        result
    }

    fn round_key(&self) -> u64 {
        self.key
    }
}

/// 对端设备发布一轮（域手动轮次入口直跑——`sync_now` 的账本注册表门以引导
/// 快照注册表为前提，独立造数目录无注册表快照，域轮次入口是同一执行协议的
/// 可达面）；断言本机段已上通道（uploaded > 0）。
fn peer_publish_round(
    peer_db: &ledger_infra::db::DbState,
    config: &ledger_sync_engine::SyncChannelConfig,
) {
    let channel = ledger_sync_engine::build_channel(config).unwrap();
    let locks = PeerRoundConn {
        conn: std::sync::Arc::clone(&peer_db.conn),
        key: ledger_sync_engine::connection_round_key(&peer_db.conn),
    };
    let report = ledger_sync_engine::run_round_once(
        &locks,
        &channel,
        &ledger_sync_engine::EnvelopeMode::Plaintext,
    )
    .expect("对端轮次应成功");
    assert!(
        report.uploaded_ops > 0,
        "对端段应已上通道（uploaded = {}）",
        report.uploaded_ops
    );
}

/// 配置通道到指定账本（经设置域单点直落：桩是明文 http 端点，保存门只收
/// https——保存门的拒绝路径归 headless 同步卡旅程既有覆盖）。
fn configure_channel(
    db: &ledger_infra::db::DbState,
    config: &ledger_sync_engine::SyncChannelConfig,
) {
    let conn = db.conn.lock().unwrap();
    ledger_infra::settings::set(
        &conn,
        ledger_infra::settings::SettingKey::SyncChannelConfig,
        config,
    )
    .unwrap();
}

/// 多端同步触发旅程：对端设备先经真 S3 桩发布本机段 → NSH 壳引导（run_startup
/// 成功分支接线触发闭集）→ **打开即同步**轮次拉取重放对端 op，失效广播驱动
/// 列表重读，对端账目上屏 → 对端再发布一笔 → 经表单记一笔本机账 →
/// `record_local` 写信号 → 调度线程去抖合流跑一轮（**写后触发**）拉取重放
/// 对端第二笔 → 失效广播 → 第二笔对端账目上屏。
///
/// 删除 run_startup 的同步触发接线 → 调度不在位、on-start 不跑 → 对端账目
/// 永不上屏 → 红；删除轮次回调的失效广播排空（sync_triggers）→ 轮次照跑而
/// 列表不重读（后半程的列表已稳定，第二笔只能经广播上屏）→ 红；删除域侧
/// 写后钩子（wiring install_after_write_hook）→ 去抖轮次不跑 → 第二笔不上屏
/// → 红（ADR-0087：断言对准「另一设备的账目自动出现在列表」，不对准线程或
/// 函数调用形状）。轮询节奏是生产常量：去抖 5s，断言窗口给足（12s）。
#[test]
fn sync_triggers_pull_peer_segments_and_reload_list() {
    nsh_app::app::reset_book_assembly();
    let stub = test_support::s3::spawn_s3_stub(test_support::s3::S3StubConfig::new(
        test_support::s3::S3Addressing::PathStyle,
    ));
    let config = stub.channel_config("family");

    // 对端设备：独立账本 + 同一通道同一空间（跨端共识的世界身份），垫一笔账
    // 后发布本机段。对端账户名取「对端现金」（不与本地造数账户同名：重放会把
    // 对端账户带入本机，表单账户候选保持唯一，不与既有表单旅程的选法互相牵制）。
    let peer_dir = make_book("sync-trigger-peer");
    let peer_db = nsh_app::boot::boot_ledger(&peer_dir).unwrap();
    configure_channel(&peer_db, &config);
    let peer_account = {
        let conn = peer_db.conn.lock().unwrap();
        ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "对端现金".to_owned(),
                kind: AccountType::Cash,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap()
    };
    {
        let conn = peer_db.conn.lock().unwrap();
        ledger_transaction::create(
            &conn,
            seed::expense_input(
                &peer_account,
                &seed::test_date(0),
                2_500,
                "对端同步来的一笔",
            ),
        )
        .unwrap();
    }
    peer_publish_round(&peer_db, &config);

    // 本机设备：独立账本 + 同一通道 + 一个现金账户（表单唯一候选）。
    let dir = make_book("sync-trigger-local");
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        configure_channel(&db, &config);
        seed::seed_cash_account(&db);
    }

    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(desktop_config()).unwrap();
    open_transactions(&mut app);

    // 打开即同步：引导完成即拉起的一次性轮次把对端第一笔拉取重放，失效广播
    // 驱动重读上屏（首读早于轮次完成的近确定时序下，本行只能经广播上屏）。
    app.wait_for_within("text:对端同步来的一笔", Duration::from_secs(12))
        .unwrap();

    // 对端再发布一笔（app 已就绪：本轮不会被任何触发拉取，列表保持三态不变）。
    {
        let conn = peer_db.conn.lock().unwrap();
        ledger_transaction::create(
            &conn,
            seed::expense_input(
                &peer_account,
                &seed::test_date(1),
                2_600,
                "对端同步来的第二笔",
            ),
        )
        .unwrap();
    }
    peer_publish_round(&peer_db, &config);

    // 经表单记一笔本机账：写入口 → record_local 写信号 → 去抖合流 → 轮次
    // （发布本机段 + 拉取重放对端第二笔）→ applied 失效广播 → 重读上屏。
    open_create_form(&mut app, "id:tx-add-expense", "记一笔 · 支出");
    app.type_text("id:tx-form-amount", "12.50").unwrap();
    pick_only_account(&mut app);
    app.press("id:tx-form-submit").unwrap();
    app.wait_for_gone("text:记一笔 · 支出", DEFAULT_WAIT)
        .unwrap();

    app.wait_for_within("text:对端同步来的第二笔", Duration::from_secs(12))
        .unwrap();
    app.wait_for("text:共 3 条").unwrap();

    // 轮次证据：上次成功同步时刻落库（域整体裁决点；不经 UI 直读同一库）。
    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let last = ledger_infra::settings::get::<Option<String>>(
        &conn,
        ledger_infra::settings::SettingKey::SyncLastSyncAt,
        None,
    )
    .unwrap();
    assert!(last.is_some(), "自动轮次应在整体裁决点落库上次成功同步时刻");
}
