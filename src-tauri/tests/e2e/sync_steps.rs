//! 多端同步单端旅程 BDD 步骤（spec #863 / ADR-0091 决策 9）。
//!
//! 单端旅程：配置通道 → 触发同步 → 数据落库 → 挂起通知可见。真双端语义（LWW /
//! 防双扣 / 重放幂等 / 源端折算）归域单测（ADR-0087），本文件不为域规则凑数据。
//!
//! 步骤直调域层公开接缝（`sync_engine::trigger` 的通道配置/构库/轮次编排/会话
//! 信封形态、`sync_engine::channel` 的通道取件），不经壳层（测试无应用运行时）；
//! 通道用真实 WebDAV 桩（进程内 axum，域单测与命令集成测试同体消费，ADR-0084
//! 决策 1），故「配置通道 → 触发同步 → 数据落库」是真实 HTTP 语义而非内存替身。
//!
//! 桩按场景现起（`world.sync.stub`，Drop 清理）：每个场景一个独立通道世界，
//! 场景之间零串扰（域单测里桩是场景内局部量，同款纪律）。对端投递用「另起一个
//! 设备库发布」与「直接写一段坏 op 的段文件」两条真实通道路径，不绕过产品代码。
//!
//! 与测试工厂的分层边界（ADR-0086 决策 9）：本文件只共享 `test_support::webdav`
//! 的 **WebDAV 协议替身**（真实 HTTP 服务的进程内实现，域单测与命令面集成测试
//! 同体消费，ADR-0084 准入「跨 ≥2 处同体消费」），**不消费工厂的建库/种子/
//! 默认值集**——BDD 侧建库走产品开库入口（world 的 `DbState`）、种子走 `crate::common`
//! 的 e2e 共享助手、输入走 `step_inputs`、写入走 `step_verbs`，两层互不共享默认值。

use cucumber::{given, then, when};
use rusqlite::Connection;

use tauri_app_lib::settings::{self, SettingKey};
use tauri_app_lib::sync_engine::trigger::{
    SessionEnvelope, SyncChannel, build_channel, configured_channel, run_auto_round, run_round_once,
};
use tauri_app_lib::sync_engine::{
    ChannelLayout, ChannelManifest, DomainCommand, EnvelopeMode, SegmentEntry, StreamManifest,
    SyncChannelConfig, SyncOp, Transport,
};
use tauri_app_lib::transaction::{
    NormalizedTransaction, TransactionCommand, TransactionInput, TransactionKind,
};

use crate::common::seed_account_with_expenses;
use crate::step_inputs::expense_input;
use crate::step_verbs::create_transaction_verb;
use crate::world::LedgerWorld;
use tauri_app_lib::db::DbState;
use tauri_app_lib::test_support::spawn_webdav_stub;

/// 把阻塞的通道工作（reqwest 阻塞客户端 + 真 HTTP）移出异步上下文：cucumber
/// 场景跑在 tokio 运行时内，阻塞客户端在其中构造/析构会 panic（「Cannot drop a
/// runtime in a context where blocking is not allowed」）。`block_in_place` 声明
/// 「本段要阻塞」——与产品侧把同步轮次放进 `run_db` 阻塞线程池同一语义
///（ADR-0069 / 壳层 `sync_now` 的接线形态）。
fn blocking<T>(f: impl FnOnce() -> T) -> T {
    tokio::task::block_in_place(f)
}

/// 场景级通道句柄（world 不持它——通道是配置产物，随场景现构）。
/// 构库本身要建 reqwest 阻塞客户端，故整段在阻塞上下文中执行（见 [`blocking`]）。
fn channel_of(world: &LedgerWorld) -> SyncChannel {
    let config = {
        let conn = world_conn!(world);
        configured_channel(&conn)
            .expect("通道配置读取应成功")
            .expect("场景应先配置通道")
    };
    blocking(|| build_channel(&config).expect("通道构库应成功"))
}

/// 桩的同步根 URL（场景应先配置通道）。
fn stub_url(world: &LedgerWorld) -> String {
    world
        .sync
        .stub
        .as_ref()
        .expect("场景应先起通道桩")
        .base_url
        .clone()
}

/// 本机设备标识（判定依据读取，非夹具）。
fn device_id_of(conn: &Connection) -> String {
    conn.query_row("SELECT id FROM sync_device LIMIT 1", [], |r| r.get(0))
        .expect("本机设备标识应已生成")
}

/// SHA-256 hex（段自校验摘要；步骤侧独立实现，避免依赖域的私有助手）。
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

#[given(expr = "以当前账本配置同步通道 空间 {string}")]
fn configure_channel(world: &mut LedgerWorld, space: String) {
    configure_channel_impl(world, space);
}

/// 同一段落在「用户动作」语境下也出现（旅程首步「配置通道」）：cucumber 按关键字
/// 匹配步骤定义，故 When 形态另行注册、委托同一实现（语义零分叉）。
#[when(expr = "以当前账本配置同步通道 空间 {string}")]
fn configure_channel_when(world: &mut LedgerWorld, space: String) {
    configure_channel_impl(world, space);
}

fn configure_channel_impl(world: &mut LedgerWorld, space: String) {
    // 场景前置：清会话密钥记忆（进程级单例跨场景共享，明文库场景不应残留密文形态）。
    SessionEnvelope::forget();
    // 通道桩按场景现起（同场景内重复配置复用同一桩——语义等价于「改配置」）。
    if world.sync.stub.is_none() {
        world.sync.stub = Some(spawn_webdav_stub(None));
    }
    let config = SyncChannelConfig {
        base_url: stub_url(world),
        username: String::new(),
        password: String::new(),
        space_id: space,
    };
    let conn = world_conn!(world);
    settings::set(&conn, SettingKey::SyncChannelConfig, &config).expect("通道配置应落库");
}

#[given(expr = "写入一笔支出 {int} 到账户 {string} 日期 {string} 备注 {string}")]
fn write_expense(
    world: &mut LedgerWorld,
    amount: i64,
    account: String,
    date: String,
    note: String,
) {
    let account_id = world.account_id(&account);
    let input = TransactionInput {
        note: Some(note),
        date,
        ..expense_input(amount, &account_id, "2026-02-01")
    };
    // 步骤动词（ADR-0086 决策 1/3）：写入经共享动词走公开写入口，步骤函数
    // 只做文本解析 + 动词调用（不直调行为层）。
    create_transaction_verb(world, input);
}

/// 对端把同一笔数据推上同一通道：另起一个「对端设备」库（同构种子）跑一轮发布。
/// 桩根目录按同步空间共享，故对端的段对本端可见——本端随后拉取即得真实数据。
#[given(expr = "对端账本已把同一笔数据推上同一通道")]
fn peer_publishes(world: &mut LedgerWorld) {
    // 对端库：e2e 共享种子形态（`crate::common`，非测试工厂——ADR-0086 决策 9
    // 分层互斥）。账户 + 一笔支出都经域公开写入口，产出账户 op 与交易 op 随行：
    // 本端重放时账户先落地，交易 op 才有可引用的外键——「对端数据落到本端」的
    // 真实形态（若只发交易 op，本端会因缺账户外键而挂起）。
    let peer = DbState::open_in_memory().expect("对端库初始化失败");
    {
        let conn = peer.conn.lock().expect("对端连接锁应可获取");
        seed_account_with_expenses(&conn, "对端现金", "对端记的账", 1, 2_500, "2026-02-01");
    }
    let base_url = stub_url(world);
    blocking(|| {
        let peer_channel = build_channel(&SyncChannelConfig {
            base_url,
            username: String::new(),
            password: String::new(),
            space_id: "default".into(),
        })
        .expect("对端通道构库应成功");
        let conn = peer.conn.lock().expect("对端连接锁应可获取");
        run_round_once(&conn, &peer_channel, &EnvelopeMode::Plaintext).expect("对端发布轮次应成功");
    });
}

/// 对端投递一条引用不存在账户的操作：直接写一个含坏 op 的段文件 + 归并 manifest。
/// 该 op 外键指向不存在的账户，重放必然被账户存活守卫拒绝——挂起队列因此非空
///（「挂起通知可见」的被测前提）。写入经通道的段/清单形态（产品代码消费的形状）。
#[given(expr = "对端投递一条引用不存在账户的操作")]
fn peer_delivers_unreplayable_op(world: &mut LedgerWorld) {
    let layout = ChannelLayout::new("default").expect("布局应可构造");
    let config = {
        let conn = world_conn!(world);
        configured_channel(&conn)
            .expect("通道配置读取应成功")
            .expect("场景应先配置通道")
    };
    let channel = blocking(|| build_channel(&config).expect("通道构库应成功"));
    let transport = channel.transport();
    // 对端身份与时钟：段必须归属该流（通道侧有错流防御），故用固定设备 id。
    let op = SyncOp {
        op_id: "e2e-parked-op".into(),
        device_id: "e2e-peer-device".into(),
        clock: 1,
        // 与当前 schema 同版（走重放路径而非 schema 偏斜挂起）：取本场景库的
        // `user_version`（迁移后即当前版本）。
        schema_version: world_conn!(world)
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .expect("读 schema 版本应成功"),
        command: DomainCommand::Transaction(TransactionCommand::Create {
            id: "e2e-parked-txn".into(),
            row: NormalizedTransaction {
                kind: TransactionKind::Expense,
                amount_cents: 1_000,
                currency_code: "CNY".into(),
                amount_native_cents: 1_000,
                account_id: "no-such-account".into(),
                to_account_id: None,
                funding_account_id: None,
                category_id: None,
                merchant_id: None,
                policy_id: None,
                refund_of_transaction_id: None,
                note: Some("引用不存在账户".into()),
                date: "2026-02-01".into(),
            },
            investment: None,
        }),
    };
    let payload = serde_json::to_vec(&vec![op]).expect("段载荷序列化应成功");
    let manifest = ChannelManifest {
        version: 1,
        streams: vec![StreamManifest {
            device_id: "e2e-peer-device".into(),
            segments: vec![SegmentEntry {
                file: "seg-0000000001-0000000001.enc".into(),
                first_clock: 1,
                last_clock: 1,
                size: payload.len() as u64,
                sha256: sha256_hex(&payload),
            }],
        }],
        checkpoint: None,
    };
    blocking(|| {
        transport
            .ensure_dir(&layout.stream_dir("e2e-peer-device"))
            .expect("建流目录应成功");
        transport
            .write_file(&layout.segment_path("e2e-peer-device", 1, 1), &payload)
            .expect("段写入应成功");
        transport
            .write_file(
                &layout.manifest_path(),
                &serde_json::to_vec_pretty(&manifest).expect("清单序列化应成功"),
            )
            .expect("清单写入应成功");
    });
}

#[given(expr = "通道指向不可达的同步地址")]
fn point_to_unreachable_channel(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    settings::set(
        &conn,
        SettingKey::SyncChannelConfig,
        &SyncChannelConfig {
            base_url: "http://127.0.0.1:9/dav/".into(),
            username: String::new(),
            password: String::new(),
            space_id: "default".into(),
        },
    )
    .expect("通道配置应落库");
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "打开应用即同步一轮")]
fn auto_sync_once(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    world.sync.last_auto_round = Some(blocking(|| {
        run_auto_round(&conn, &SessionEnvelope::Plaintext)
    }));
}

#[when(expr = "手动触发一轮同步")]
fn manual_sync_once(world: &mut LedgerWorld) {
    let channel = channel_of(world);
    let conn = world_conn!(world);
    match blocking(|| run_round_once(&conn, &channel, &EnvelopeMode::Plaintext)) {
        Ok(report) => {
            world.sync.last_report = Some(report);
            world.last_app_error = None;
        }
        Err(e) => world.last_app_error = Some(e),
    }
}

#[when(expr = "以密文库会话形态打开应用即同步一轮")]
fn auto_sync_encrypted_session(world: &mut LedgerWorld) {
    // 密文库会话形态：记入会话口令（自动轮次据此封包，不读钥匙串）。
    SessionEnvelope::remember(SessionEnvelope::Encrypted("master-pass".into()));
    let session = SessionEnvelope::current();
    world.sync.session_encrypted = matches!(session, SessionEnvelope::Encrypted(_));
    let conn = world_conn!(world);
    world.sync.last_auto_round = Some(blocking(|| run_auto_round(&conn, &session)));
    SessionEnvelope::forget();
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "通道上应有本机账本目录")]
fn channel_has_book_dir(world: &mut LedgerWorld) {
    let channel = channel_of(world);
    let conn = world_conn!(world);
    let layout = ChannelLayout::new("default").expect("布局应可构造");
    let manifest = blocking(|| {
        channel
            .transport()
            .read_file(&layout.manifest_path())
            .expect("读清单应成功")
            .expect("通道上应有 manifest（轮次确实跑过）")
    });
    assert!(!manifest.is_empty(), "manifest 不应为空");
    let device = device_id_of(&conn);
    assert!(
        layout.stream_dir(&device).starts_with(&layout.book_dir()),
        "本机流目录应位于账本目录下"
    );
}

#[then(expr = "本轮同步应上传 {int} 条操作")]
fn uploaded_ops_is(world: &mut LedgerWorld, expected: usize) {
    let round = world
        .sync
        .last_auto_round
        .as_ref()
        .expect("应先执行自动轮次")
        .as_ref()
        .expect("自动轮次应成功");
    let report = round.as_ref().expect("通道已配置：自动轮次应有报告");
    assert_eq!(report.uploaded_ops, expected, "上传 op 数不匹配");
}

#[then(expr = "本端应已应用对端操作")]
fn applied_foreign_ops(world: &mut LedgerWorld) {
    let report = world.sync.last_report.as_ref().expect("应先执行手动轮次");
    assert!(
        report.applied >= 1,
        "本端应至少应用一条对端 op，实际报告 {report:?}"
    );
}

#[then(expr = "同步状态应显示已配置通道")]
fn status_channel_configured(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    assert!(
        configured_channel(&conn).expect("读配置应成功").is_some(),
        "通道应显示已配置"
    );
}

#[then(expr = "同步状态应显示未配置通道")]
fn status_channel_absent(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    assert!(
        configured_channel(&conn).expect("读配置应成功").is_none(),
        "通道应显示未配置"
    );
}

#[then(expr = "同步状态应带上次同步时刻")]
fn status_has_last_sync_at(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    let stamp: Option<String> =
        settings::get(&conn, SettingKey::SyncLastSyncAt, None).expect("读成功时刻应成功");
    assert!(stamp.is_some(), "成功轮次后应有上次同步时刻");
}

#[then(expr = "同步轮次应零动作")]
fn auto_sync_was_noop(world: &mut LedgerWorld) {
    let round = world
        .sync
        .last_auto_round
        .as_ref()
        .expect("应先执行同步轮次");
    assert!(
        matches!(round, Ok(None)),
        "同步轮次应零动作（未配置通道），实际 {round:?}"
    );
}

#[then(expr = "挂起队列应有 {int} 条不可重放操作")]
fn parked_count_is(world: &mut LedgerWorld, expected: usize) {
    let conn = world_conn!(world);
    let parked = tauri_app_lib::sync_engine::parked_ops(&conn).expect("读挂起队列应成功");
    assert_eq!(parked.len(), expected, "挂起条数不匹配: {parked:?}");
}

#[then(expr = "挂起通知应携带码化原因")]
fn parked_notice_has_code(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    let parked = tauri_app_lib::sync_engine::parked_ops(&conn).expect("读挂起队列应成功");
    let first = parked.first().expect("挂起队列应非空");
    assert!(
        first.code.contains('.'),
        "挂起原因应是稳定错误码，实际 {:?}",
        first.code
    );
    assert!(!first.message.is_empty(), "挂起通知应携带可读原因");
}

#[then(expr = "会话信封形态应为密文")]
fn session_is_encrypted(world: &mut LedgerWorld) {
    assert!(
        world.sync.session_encrypted,
        "会话口令在场即密文形态（自动轮次不读钥匙串）"
    );
}

#[then(expr = "同步轮次应封包上传")]
fn auto_round_sealed(world: &mut LedgerWorld) {
    let round = world
        .sync
        .last_auto_round
        .as_ref()
        .expect("应先执行同步轮次")
        .as_ref()
        .expect("同步轮次应成功");
    let report = round.as_ref().expect("通道已配置：同步轮次应有报告");
    assert!(
        !report.plaintext_mode,
        "密文会话形态下同步轮次应封包（plaintext_mode 为假），实际 {report:?}"
    );
    assert!(report.uploaded_ops >= 1, "本轮应上传本机 op");
}
