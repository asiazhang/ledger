use cucumber::{given, then, when};
use rusqlite::Connection;

use ledger_infra::settings::{self, SettingKey};
use ledger_sync_engine::trigger::{
    SessionEnvelope, SyncChannel, build_channel, configured_channel, run_auto_round, run_round_once,
};
use ledger_sync_engine::{
    ChannelLayout, DirectConn, DomainCommand, EnvelopeMode, SyncChannelConfig, SyncOp,
};
use ledger_transaction::{
    NormalizedTransaction, TransactionCommand, TransactionInput, TransactionKind,
};

use crate::common::seed_account_with_expenses;
use crate::step_inputs::expense_input;
use crate::step_verbs::create_transaction_verb;
use crate::world::LedgerWorld;
use ledger_infra::db::DbState;
use tauri_app_lib::test_support::{S3Addressing, S3StubConfig, publish_raw_segment, spawn_s3_stub};

fn blocking<T>(f: impl FnOnce() -> T) -> T {
    tokio::task::block_in_place(f)
}

fn channel_of(world: &LedgerWorld) -> SyncChannel {
    let config = {
        let conn = world_conn!(world);
        configured_channel(&conn)
            .expect("通道配置读取应成功")
            .expect("场景应先配置通道")
    };
    blocking(|| build_channel(&config).expect("通道构库应成功"))
}

fn stub_channel_config(world: &LedgerWorld, space: &str) -> SyncChannelConfig {
    let stub = world.boot.sync_stub.as_ref().expect("场景应先起通道桩");
    SyncChannelConfig {
        endpoint: stub.endpoint.clone(),
        region: stub.region.clone(),
        bucket: stub.bucket.clone(),
        access_key: stub.access_key.clone(),
        secret_key: "test-secret-key".into(),
        prefix: String::new(),
        path_style: true,
        space_id: space.to_string(),
    }
}

fn device_id_of(conn: &Connection) -> String {
    conn.query_row("SELECT id FROM sync_device LIMIT 1", [], |r| r.get(0))
        .expect("本机设备标识应已生成")
}

#[given(expr = "以当前账本配置同步通道 空间 {string}")]
fn configure_channel(world: &mut LedgerWorld, space: String) {
    configure_channel_impl(world, space);
}

#[when(expr = "以当前账本配置同步通道 空间 {string}")]
fn configure_channel_when(world: &mut LedgerWorld, space: String) {
    configure_channel_impl(world, space);
}

fn configure_channel_impl(world: &mut LedgerWorld, space: String) {
    SessionEnvelope::forget();
    if world.boot.sync_stub.is_none() {
        world.boot.sync_stub = Some(spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle)));
    }
    let config = stub_channel_config(world, &space);
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
    create_transaction_verb(world, input);
}

#[given(expr = "对端账本已把同一笔数据推上同一通道")]
fn peer_publishes(world: &mut LedgerWorld) {
    let peer = DbState::open_in_memory().expect("对端库初始化失败");
    {
        let conn = peer.conn.lock().expect("对端连接锁应可获取");
        seed_account_with_expenses(&conn, "对端现金", "对端记的账", 1, 2_500, "2026-02-01");
    }
    let peer_config = stub_channel_config(world, "default");
    blocking(|| {
        let peer_channel = build_channel(&peer_config).expect("对端通道构库应成功");
        let conn = peer.conn.lock().expect("对端连接锁应可获取");
        run_round_once(
            &DirectConn::new(&conn),
            &peer_channel,
            &EnvelopeMode::Plaintext,
        )
        .expect("对端发布轮次应成功");
    });
}

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
    let op = SyncOp {
        op_id: "e2e-parked-op".into(),
        device_id: "e2e-peer-device".into(),
        clock: 1,
        schema_version: world_conn!(world)
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .expect("读 schema 版本应成功"),
        command: DomainCommand::Transaction(TransactionCommand::Create {
            id: "e2e-parked-txn".into(),
            row: NormalizedTransaction {
                funding: Vec::new(),
                kind: TransactionKind::Expense,
                amount_cents: 1_000,
                currency_code: "CNY".into(),
                amount_native_cents: 1_000,
                fx_rate_used: None,
                fx_rate_source: None,
                account_id: Some("no-such-account".into()),
                to_account_id: None,
                funding_account_id: None,
                category_id: None,
                merchant_id: None,
                policy_id: None,
                refund_of_transaction_id: None,
                source_order_no: None,
                note: Some("引用不存在账户".into()),
                date: "2026-02-01".into(),
                purchases: Vec::new(),
            },
            investment: None,
            split: None,
            convert: None,
        }),
    };
    blocking(|| {
        publish_raw_segment(
            transport,
            &layout,
            "e2e-peer-device",
            &EnvelopeMode::Plaintext,
            &[op],
        )
        .expect("段与清单写入应成功");
    });
}

#[given(expr = "通道指向不可达的同步地址")]
fn point_to_unreachable_channel(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    settings::set(
        &conn,
        SettingKey::SyncChannelConfig,
        &SyncChannelConfig {
            endpoint: "https://127.0.0.1:9".into(),
            region: "us-east-1".into(),
            bucket: "ledger-test".into(),
            access_key: "test-access-key".into(),
            secret_key: "test-secret-key".into(),
            prefix: String::new(),
            path_style: true,
            space_id: "default".into(),
        },
    )
    .expect("通道配置应落库");
}

#[when(expr = "打开应用即同步一轮")]
fn auto_sync_once(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    world.boot.sync_last_auto_round = Some(blocking(|| {
        run_auto_round(&DirectConn::new(&conn), &SessionEnvelope::Plaintext)
    }));
}

#[when(expr = "手动触发一轮同步")]
fn manual_sync_once(world: &mut LedgerWorld) {
    let channel = channel_of(world);
    let conn = world_conn!(world);
    match blocking(|| run_round_once(&DirectConn::new(&conn), &channel, &EnvelopeMode::Plaintext)) {
        Ok(report) => {
            world.boot.sync_last_report = Some(report);
            world.last_app_error = None;
        }
        Err(e) => world.last_app_error = Some(e),
    }
}

#[when(expr = "以密文库会话形态打开应用即同步一轮")]
fn auto_sync_encrypted_session(world: &mut LedgerWorld) {
    SessionEnvelope::remember(SessionEnvelope::Encrypted("master-pass".into()));
    let session = SessionEnvelope::current();
    world.boot.sync_session_encrypted = matches!(session, SessionEnvelope::Encrypted(_));
    let conn = world_conn!(world);
    world.boot.sync_last_auto_round = Some(blocking(|| {
        run_auto_round(&DirectConn::new(&conn), &session)
    }));
    SessionEnvelope::forget();
}

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
        .boot
        .sync_last_auto_round
        .as_ref()
        .expect("应先执行自动轮次")
        .as_ref()
        .expect("自动轮次应成功");
    let report = round.as_ref().expect("通道已配置：自动轮次应有报告");
    assert_eq!(report.uploaded_ops, expected, "上传 op 数不匹配");
}

#[then(expr = "本端应已应用对端操作")]
fn applied_foreign_ops(world: &mut LedgerWorld) {
    let report = world
        .boot
        .sync_last_report
        .as_ref()
        .expect("应先执行手动轮次");
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
        .boot
        .sync_last_auto_round
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
    let parked = ledger_sync_engine::parked_ops(&conn).expect("读挂起队列应成功");
    assert_eq!(parked.len(), expected, "挂起条数不匹配: {parked:?}");
}

#[then(expr = "挂起通知应携带码化原因")]
fn parked_notice_has_code(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    let parked = ledger_sync_engine::parked_ops(&conn).expect("读挂起队列应成功");
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
        world.boot.sync_session_encrypted,
        "会话口令在场即密文形态（自动轮次不读钥匙串）"
    );
}

#[then(expr = "同步轮次应封包上传")]
fn auto_round_sealed(world: &mut LedgerWorld) {
    let round = world
        .boot
        .sync_last_auto_round
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
