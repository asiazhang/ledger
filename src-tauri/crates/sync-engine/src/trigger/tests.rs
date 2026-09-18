//! 触发编排域单测（issue #863 / ADR-0091 决策 9）：通道配置单点、轮次编排与
//! 自动触发的零动作语义、会话信封形态——域行为的唯一断言权威层（ADR-0087）。
//!
//! 轮次协议与双端语义归 [`super::super::tests::channel`] 的既有套件；本套件只
//! 钉触发侧新增的事实：配置读取/构库单点、未配置即零动作（不触网、不落成功
//! 时刻）、一轮成功即落成功时刻、会话记忆决定信封形态。

use crate::EnvelopeMode;
use crate::SyncChannelConfig;
use crate::tests::common::direct;
use crate::tests::common::make_expense;
use crate::trigger::{SessionEnvelope, build_channel, configured_channel, run_auto_round};
use ledger_infra::settings::{self, SettingKey};
use ledger_transaction::write::protocol;
use tauri_app_lib::test_support::{self, seed_account};

/// 通道配置单点：未配置回 `None`；保存后读回同值。
#[test]
fn channel_config_roundtrip_through_settings_single_point() {
    let conn = test_support::open();
    assert!(
        configured_channel(&conn).unwrap().is_none(),
        "新库未配置通道"
    );

    let config = SyncChannelConfig {
        endpoint: "https://s3.example.com".into(),
        region: "cn-hangzhou".into(),
        bucket: "ledger".into(),
        prefix: "sync/family".into(),
        access_key: "ak".into(),
        secret_key: "sk".into(),
        path_style: true,
        space_id: "family".into(),
    };
    settings::set(&conn, SettingKey::SyncChannelConfig, &config).unwrap();
    assert_eq!(
        configured_channel(&conn).unwrap().as_ref(),
        Some(&config),
        "读回经同一单点，值与写入一致"
    );
}

/// 空间字段缺省回 `default`（旧配置升级路径）：反序列化缺字段不报错。
#[test]
fn missing_space_id_deserializes_to_default() {
    let config: SyncChannelConfig =
        serde_json::from_str(r#"{"endpoint":"https://s3.example.com"}"#).unwrap();
    assert_eq!(config.space_id, "default");
    assert_eq!(config.endpoint, "https://s3.example.com");
}

/// 退役后端留下的旧配置仍可解析（#1221 升级路径）：老 JSON 里的 `backend` 与
/// 地址 / 凭据键都已不是本形态的字段，serde 默认忽略未知键——配置照常反序列化，
/// 未识别的字段取空串 / 假值，已识别的空间字段照常取值。同步从未随任何版本
/// 发布（ADR-0091 决策 1 修订），本测试只保证「解析性不回归」，不承诺旧配置
/// 仍可用（旧配置缺 S3 字段，构库会报 `sync-channel.base-url-missing`）。
#[test]
fn legacy_config_with_retired_backend_keys_still_deserializes() {
    let config: SyncChannelConfig = serde_json::from_str(
        r#"{"backend":"webdav","base_url":"https://dav.example.com/dav/",
            "username":"n","password":"p","space_id":"family"}"#,
    )
    .unwrap();
    assert_eq!(config.space_id, "family", "已识别字段照常取值");
    assert_eq!(config.endpoint, "");
    assert_eq!(config.region, "");
    assert_eq!(config.bucket, "");
    assert_eq!(config.prefix, "");
    assert_eq!(config.access_key, "");
    assert_eq!(config.secret_key, "");
    assert!(!config.path_style);
}

/// 未知键一律忽略：退役键与未来键都不该让解析变红（与上一条同源机制，这条钉
/// 「忽略」的普遍性，不依赖任何具体键名）。
#[test]
fn unknown_config_keys_are_ignored() {
    let config: SyncChannelConfig = serde_json::from_str(
        r#"{"endpoint":"https://s3.example.com","bucket":"ledger","future_option":true}"#,
    )
    .unwrap();
    assert_eq!(config.endpoint, "https://s3.example.com");
    assert_eq!(config.bucket, "ledger");
}

/// 构库单点：非法空间（清洗后为空）报码化错误；保存路径与本单点同源。
#[test]
fn build_channel_rejects_invalid_space_with_coded_error() {
    let config = SyncChannelConfig {
        endpoint: "https://s3.example.com".into(),
        region: "cn-hangzhou".into(),
        bucket: "ledger".into(),
        access_key: "ak".into(),
        secret_key: "sk".into(),
        space_id: "///".into(),
        ..Default::default()
    };
    // `SyncChannel` 不实现 `Debug`（内含 reqwest 客户端），故不用 `unwrap_err`：
    // 经 match 取错误，语义等价且不引入 Debug 约束。
    let err = match build_channel(&config) {
        Ok(_) => panic!("非法空间应被拒"),
        Err(e) => e,
    };
    assert!(err.is_code("sync-channel.book-id-invalid"), "实际: {err:?}");
}

/// 会话信封形态（ADR-0098）：无会话口令 → 明文；有 → 密文并以之封包。
/// 「自动轮询不读钥匙串」由本单点保证（读会话记忆，不读钥匙串）。
#[test]
fn session_envelope_follows_session_passphrase() {
    SessionEnvelope::forget();
    assert_eq!(
        SessionEnvelope::current(),
        SessionEnvelope::Plaintext,
        "无会话记忆 = 明文形态（明文库的正常形态）"
    );

    SessionEnvelope::remember(SessionEnvelope::Encrypted("master-pass".into()));
    assert_eq!(
        SessionEnvelope::current(),
        SessionEnvelope::Encrypted("master-pass".into())
    );
    assert!(matches!(
        SessionEnvelope::current().mode(),
        EnvelopeMode::Encrypted { passphrase } if passphrase == "master-pass"
    ));

    // 换库清空（原位重引导，`restart_app` 单点）：新库形态未知，回明文
    // 形态等下一次解锁或手动同步重新记入——避免拿旧库口令去封新库的段
    //（忘记口令 / 启动失败重置已改随 resume 签名记入明文，issue #1395）。
    SessionEnvelope::forget();
    assert_eq!(SessionEnvelope::current(), SessionEnvelope::Plaintext);
    assert_eq!(
        SessionEnvelope::Plaintext.mode(),
        EnvelopeMode::Plaintext,
        "明文形态对应的信封模式是明文直通"
    );
}

/// 自动轮次：通道未配置时零动作（`None`，不触网、不落成功时刻）。
#[test]
fn auto_round_without_channel_is_a_noop() {
    let conn = test_support::open();
    let outcome = run_auto_round(&direct(&conn), &SessionEnvelope::Plaintext).unwrap();
    assert!(outcome.is_none(), "未配置通道：自动轮次零动作");
    assert_eq!(
        settings::get::<Option<String>>(&conn, SettingKey::SyncLastSyncAt, None).unwrap(),
        None,
        "零动作轮次不更新成功时刻"
    );
}

/// 轮次编排单点：一轮成功即更新「上次成功同步时刻」（手动与自动入口共用）。
#[test]
fn round_once_stamps_last_sync_on_success() {
    let stub = test_support::spawn_s3_stub(test_support::S3StubConfig::new(
        test_support::S3Addressing::PathStyle,
    ));
    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn, make_expense("acc-1", 10000, "午饭")).unwrap();
    let config = stub.channel_config("default");
    settings::set(&conn, SettingKey::SyncChannelConfig, &config).unwrap();

    // 测试实例纪律：`stub.channel_config` 来自根包测试工厂（dev-dependency），
    // 返回根包图内同步域实例的 `SyncChannelConfig`；本用例的通道/轮次路径随
    // 该实例驱动（ledger-transaction/#1092 同款），断言不变。
    let channel = tauri_app_lib::ledger_sync_engine::build_channel(&config).unwrap();
    let report = tauri_app_lib::ledger_sync_engine::run_round_once(
        &tauri_app_lib::ledger_sync_engine::DirectConn::new(&conn),
        &channel,
        &tauri_app_lib::ledger_sync_engine::EnvelopeMode::Plaintext,
    )
    .unwrap();
    assert!(report.uploaded_ops >= 1, "本轮应上传本机 op");
    assert!(
        settings::get::<Option<String>>(&conn, SettingKey::SyncLastSyncAt, None)
            .unwrap()
            .is_some(),
        "成功轮次更新上次同步时刻"
    );
    assert!(
        channel
            .transport()
            .read_file(&channel.layout().manifest_path())
            .unwrap()
            .is_some(),
        "轮次应写出 manifest（编排确实跑了轮次协议，而非空转）"
    );
}

/// 写后触发的去抖合流（ADR-0091 决策 9「写 op 后即时入队上传」）：调用方形态
/// 是「外层 `recv_timeout` 已消费首个写信号，再进入吸干」——单次写必须跑一轮
/// （否则记完账永不上传）；窗口中途到达的写被吸干、合流进同一轮（不是每写一笔
/// 传一次）；通道断裂也不得挂死。
#[test]
fn write_after_sync_debounce_runs_one_round_per_burst() {
    let window = std::time::Duration::from_millis(80);

    // 单次写（最常见的「记一笔账」）：外层消费首个信号 → 吸干 → 本轮确实跑。
    // 返回值已取消（吸干后必然跑一轮，没有可假的分支），故断言「不挂死且无
    // 残留」：残留即第二轮空转。
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    tx.send(()).unwrap();
    assert!(
        rx.recv_timeout(window).is_ok(),
        "外层应先消费触发收尾的那个写信号"
    );
    crate::trigger::drain_write_signals(&rx, window);
    assert!(
        rx.try_recv().is_err(),
        "单次写不应残留信号（否则第二轮空转）"
    );

    // 窗口中途到达的写：被吸干、合流进同一轮（去抖窗口真实生效），不残留。
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    tx.send(()).unwrap();
    assert!(rx.recv_timeout(window).is_ok(), "外层先消费首个写信号");
    let late = tx.clone();
    let sender = std::thread::spawn(move || {
        std::thread::sleep(window / 2);
        late.send(()).expect("中途写应可投递");
    });
    crate::trigger::drain_write_signals(&rx, window);
    sender.join().expect("投递线程应结束");
    assert!(
        rx.try_recv().is_err(),
        "窗口中途的写应被吸干合流（不触发第二轮）"
    );

    // 通道断裂（发送端全部丢弃）：吸干不得挂死、不得 panic，照常返回跑一轮。
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    tx.send(()).unwrap();
    assert!(rx.recv_timeout(window).is_ok(), "外层先消费首个写信号");
    drop(tx);
    crate::trigger::drain_write_signals(&rx, window);
}

/// 写后触发在「调度未拉起」时是零动作（写路径对同步域无感）：投递不 panic、
/// 不触库、不触网——单测与未配置同步的进程都走这一形态。
#[test]
fn write_after_sync_without_scheduler_is_a_noop() {
    crate::sync_after_write();
}

/// 写信号投递侧（与吸干侧对称的参数接缝）：通道在位即投递一次；未装通道
/// 零动作。两侧共用显式通道参数，不依赖进程级单例、不需测试后门。
#[test]
fn write_signal_is_delivered_into_the_channel() {
    use std::sync::OnceLock;
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let slot: OnceLock<std::sync::mpsc::Sender<()>> = OnceLock::new();
    assert!(slot.set(tx).is_ok());

    crate::trigger::notify_write_signal(&slot);
    assert!(
        rx.try_recv().is_ok(),
        "通道在位：本地产出 op 应投递一次写信号"
    );
    assert!(rx.try_recv().is_err(), "一次产出一次信号（不重复投递）");

    // 未装通道（未拉起调度 / 单测环境）：零动作，不 panic。
    let empty: OnceLock<std::sync::mpsc::Sender<()>> = OnceLock::new();
    crate::trigger::notify_write_signal(&empty);
}

// ---------------------------------------------------------------------------
// 轮次在途互斥（issue #1339 / ADR-0120 决策 3）与调度侧分段取锁口径
// ---------------------------------------------------------------------------

use crate::RoundConn;
use crate::channel::SyncRoundReport;
use crate::trigger::run_round_once;
use std::sync::{Arc, Mutex};

/// 登记一轮在途并要求成功（域单测速记：InFlight 即测试前提被破坏）。
fn begin_expect(key: u64) -> crate::trigger::round_gate::RoundHandle {
    match crate::trigger::round_gate::begin_round(key) {
        crate::trigger::round_gate::RoundStart::Began(handle) => handle,
        crate::trigger::round_gate::RoundStart::InFlight(_) => {
            panic!("空闲键上登记应成功")
        }
    }
}

/// 内存假通道的轮次句柄（`SyncChannel::from_parts` 测试接缝，不经真实 S3）。
/// 本组用例全链随被测本实例驱动（域单测纪律：`crate::…`）。
fn memory_channel() -> crate::trigger::SyncChannel {
    use crate::tests::common::MemoryTransport;
    crate::trigger::SyncChannel::from_parts(
        Box::new(MemoryTransport::new()),
        crate::ChannelLayout::new("default").unwrap(),
    )
}

/// 在途互斥接线 · 自动入口（负向判据，ADR-0087）：在途时重复触发**不启动第二
/// 轮**——自动入口放弃本轮（`None`，通道零写入、成功时刻不落），在途轮次交出
/// 结果后下一轮照常跑。删除 [`run_round_gated`] 的 `begin_round` 接线，第一段
/// 的 `run_auto_round` 会真的跑轮次（`Some` + manifest 出现），本测试即红。
#[test]
fn auto_round_skips_while_round_in_flight() {
    let stub = test_support::spawn_s3_stub(test_support::S3StubConfig::new(
        test_support::S3Addressing::PathStyle,
    ));
    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn, make_expense("acc-1", 10000, "午饭")).unwrap();
    let config = stub.channel_config("default");
    settings::set(&conn, SettingKey::SyncChannelConfig, &config).unwrap();
    let channel = tauri_app_lib::ledger_sync_engine::build_channel(&config).unwrap();

    // 测试实例纪律：轮次/通道路径随根包图内实例驱动（同上 run_round_once 用例）；
    // 轮次身份键是裸地址值，实例无关。trait 方法经本实例导入的 RoundConn 调用。
    let key = crate::DirectConn::new(&conn).round_key();

    // 登记一轮在途（本测试扮演在途执行体）。
    let handle = begin_expect(key);

    // 在途：自动入口放弃本轮——零动作（不触网、不写通道、不落成功时刻）。
    let outcome =
        run_auto_round(&crate::DirectConn::new(&conn), &SessionEnvelope::Plaintext).unwrap();
    assert!(
        outcome.is_none(),
        "在途时自动轮次应放弃本轮，实际 {outcome:?}"
    );
    assert_eq!(
        settings::get::<Option<String>>(&conn, SettingKey::SyncLastSyncAt, None).unwrap(),
        None,
        "放弃本轮不更新成功时刻"
    );
    assert!(
        channel
            .transport()
            .read_file(&channel.layout().manifest_path())
            .unwrap()
            .is_none(),
        "放弃本轮不触通道（manifest 不应出现）"
    );

    // 在途轮次交出结果（句柄释放即在途登记清空）后，下一轮照常跑。
    handle.complete(Ok(SyncRoundReport::default()));
    let outcome =
        run_auto_round(&crate::DirectConn::new(&conn), &SessionEnvelope::Plaintext).unwrap();
    let report = outcome.expect("在途清空后自动轮次应执行");
    assert!(
        report.uploaded_ops >= 1,
        "在途清空后的轮次应真实发布本机 op，实际 {report:?}"
    );
    assert!(
        settings::get::<Option<String>>(&conn, SettingKey::SyncLastSyncAt, None)
            .unwrap()
            .is_some(),
        "执行成功的轮次落成功时刻"
    );
}

/// 在途互斥接线 · 手动入口（负向判据，ADR-0087）：在途时重复触发**不启动第二
/// 轮**——等待并交出同一轮次报告（回显形态在 #1339 定夺留痕）。删除接线，手动
/// 入口会自己跑一轮（空通道 → 全零报告），拿不到在途轮次的标记报告，本测试即红。
#[test]
fn manual_round_waits_and_reuses_in_flight_report() {
    let conn = test_support::open();
    let channel = memory_channel();

    // 在途执行体：另一线程稍后交出一份带标记的报告（上传段数 9——空通道真跑
    // 一轮不可能产出）。
    let key = crate::DirectConn::new(&conn).round_key();
    let handle = begin_expect(key);
    let completer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        handle.complete(Ok(SyncRoundReport {
            uploaded_segments: 9,
            ..SyncRoundReport::default()
        }));
    });

    // 手动入口：等待并复用唯一在途轮次的报告（自己不跑轮次：manifest 保持缺席）。
    let report = run_round_once(
        &crate::DirectConn::new(&conn),
        &channel,
        &EnvelopeMode::Plaintext,
    )
    .unwrap();
    completer.join().unwrap();
    assert_eq!(
        report.uploaded_segments, 9,
        "手动入口应交出在途轮次的报告（复用，而非自己再跑一轮）"
    );
    assert!(
        channel
            .transport()
            .read_file(&channel.layout().manifest_path())
            .unwrap()
            .is_none(),
        "复用在途轮次时本入口不应再写通道"
    );
}

/// 在途互斥按轮次身份键分键（同端同库判据）：同键至多一轮在途，异键（不同库）
/// 并行不互斥。
#[test]
fn in_flight_mutex_keys_by_connection_identity() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    let key_a = crate::DirectConn::new(&conn_a).round_key();
    let key_b = crate::DirectConn::new(&conn_b).round_key();
    assert_ne!(key_a, key_b, "不同连接是不同库，身份键应不同");

    let handle_a = begin_expect(key_a);
    // 同键：撞上在途。异键：并行登记成功。
    assert!(matches!(
        crate::trigger::round_gate::begin_round(key_a),
        crate::trigger::round_gate::RoundStart::InFlight(_)
    ));
    let handle_b = begin_expect(key_b);
    handle_a.complete(Ok(SyncRoundReport::default()));
    handle_b.complete(Ok(SyncRoundReport::default()));
}

/// 调度侧的放弃口径（ADR-0120 决策 2/4）：任一段拿不到连接锁即整体放弃本轮
/// ——连接源报码化放弃错误，自动入口把它静默归一为 `None`（真实失败照常上抛）。
#[test]
fn auto_round_gives_up_silently_when_connection_lock_is_busy() {
    let conn = Arc::new(Mutex::new(test_support::open()));
    // 锁等待上限注入为 10ms（spec #1086 / issue #1514）：本用例断言的是「拿不到锁
    // 即放弃本轮」这一**瞬时可判定**的语义，等待时长不含信息量。按产品默认
    // LOCK_TIMEOUT（5s）实等，`run_auto_round_inner` 的读段 + 簿记段两次取锁合计
    // 10s 墙钟——改用注入短超时后同样的断言面仍在，耗时降到毫秒级。
    let locks = super::scheduler::AutoRoundConn::new(&conn)
        .with_lock_timeout(std::time::Duration::from_millis(10));

    // 锁被别人持有：连接源报放弃错误（稳定码），自动入口静默归一为 `None`。
    let held = conn.lock().unwrap();
    let error = locks
        .with_connection(crate::channel::ConnSegment::Read, |_| Ok(()))
        .unwrap_err();
    assert!(
        error.is_code("sync-engine.round-lock-give-up"),
        "拿不到锁应报放弃码，实际 {error:?}"
    );
    let outcome = super::scheduler::run_auto_round(&locks, &SessionEnvelope::Plaintext).unwrap();
    assert!(
        outcome.is_none(),
        "拿不到锁的自动轮次应静默放弃，实际 {outcome:?}"
    );
    drop(held);

    // 锁可得：连接源直通（配置未设置 → 零动作 `None`，但那是配置分支，不是放弃）。
    let outcome = super::scheduler::run_auto_round(&locks, &SessionEnvelope::Plaintext).unwrap();
    assert!(outcome.is_none(), "未配置通道：零动作");
}

/// 持锁时长探针覆盖轮次分段（#1276 守门③ / #1339）：调度侧轮次连接源的每段
/// 持锁超阈值记 warn、不静默——把段内的慢闭包（如误入的网络等待）照出来。
#[test]
fn auto_round_source_probes_lock_hold_past_threshold() {
    use ledger_infra::test_utils::capture_events;
    use tracing::Level;

    let conn = Arc::new(Mutex::new(test_support::open()));
    let locks = super::scheduler::AutoRoundConn::new(&conn);

    let events = capture_events(|| {
        locks
            .with_connection(crate::channel::ConnSegment::Read, |_| {
                std::thread::sleep(std::time::Duration::from_millis(1100));
                Ok(())
            })
            .unwrap();
    });
    assert!(
        events.iter().any(|e| e.level == Level::WARN),
        "段内持锁超阈值应记 warn（探针覆盖轮次分段），实际捕获 {events:?}"
    );
}

/// 轮次编排单点的落库段（ADR-0120 决策 6）：中途失败不更新「上次成功同步时刻」
/// ——失败经 [`run_round_once`] 上抛，成功时刻保持缺席。
#[test]
fn failed_round_does_not_stamp_last_sync() {
    use crate::tests::common::{MemoryTransport, seed_device};
    use crate::transport::Transport;

    let conn = test_support::open();
    // 通道上有一个他人流，但段文件缺失：轮次在拉取段必然失败。
    let transport = MemoryTransport::new();
    let manifest = r#"{"version":1,"streams":[{"device_id":"peer-1","segments":[
        {"file":"seg-0000000001-0000000002.enc","first_clock":1,"last_clock":2,
         "size":3,"sha256":"aa"}]}],"checkpoint":null}"#;
    transport
        .write_file("book-default/manifest.json", manifest.as_bytes())
        .unwrap();
    seed_device(&conn, "local-dev");
    let channel = crate::trigger::SyncChannel::from_parts(
        Box::new(transport),
        crate::ChannelLayout::new("default").unwrap(),
    );

    let error = run_round_once(
        &crate::DirectConn::new(&conn),
        &channel,
        &EnvelopeMode::Plaintext,
    )
    .unwrap_err();
    assert!(
        error.is_code("sync-channel.segment-missing"),
        "段缺失应上抛，实际 {error:?}"
    );
    assert_eq!(
        settings::get::<Option<String>>(&conn, SettingKey::SyncLastSyncAt, None).unwrap(),
        None,
        "中途失败不更新成功时刻（整体裁决点才落库）"
    );
}

/// 已提交重放留存（ADR-0120 决策 6 / issue #1339 验收「失败语义」）：他人流两段
/// 之间失败——失败点之前已提交的重放留存（逐条原子，不是回滚前缀），成功时刻
/// 不落库；段文件重传（内容确定等同）后重试，已重放 op 经幂等去重不重复应用，
/// 整轮成功后成功时刻才落库。
#[test]
fn replay_committed_before_midround_failure_survives_and_resumes() {
    use crate::tests::common::{MemoryTransport, seed_device};
    use crate::transport::Transport;
    use ledger_transaction::write::protocol;

    let (conn_a, conn_b) = (test_support::open(), test_support::open());
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_account(&conn_a, "acc-a", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-a", "现金", "cash", "CNY", 0);
    let first = protocol::create(&conn_a, make_expense("acc-a", 1000, "第一笔")).unwrap();
    let second = protocol::create(&conn_a, make_expense("acc-a", 2000, "第二笔")).unwrap();

    // A 端按单 op 容量发布两段。
    let transport = MemoryTransport::new();
    let layout = crate::ChannelLayout::new("default").unwrap();
    let options = crate::ChannelOptions {
        segment_max_ops: 1,
        ..crate::ChannelOptions::default()
    };
    crate::run_round_with(
        &direct(&conn_a),
        &transport,
        &layout,
        &EnvelopeMode::Plaintext,
        &options,
    )
    .unwrap();
    let manifest: crate::ChannelManifest = serde_json::from_slice(
        &transport
            .read_file(&layout.manifest_path())
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.streams[0].segments.len(), 2, "A 端应发布两段");
    // 故障注入：摘走第二段的段文件（manifest 仍列出它——「清单先行、文件未到齐」
    // 的既有通道形态，段缺失错误由此产生）；原字节留作重传样本。
    let second_file = manifest.streams[0].segments[1].file.clone();
    let second_path = layout.stream_file_path("dev-a", &second_file);
    let second_bytes = transport.read_file(&second_path).unwrap().unwrap();
    transport.files_remove(&second_path);

    // B 端第一轮：第一段重放提交，第二段缺失 → 轮次失败。
    let channel = crate::trigger::SyncChannel::from_parts(Box::new(transport), layout.clone());
    let error = run_round_once(&direct(&conn_b), &channel, &EnvelopeMode::Plaintext).unwrap_err();
    assert!(
        error.is_code("sync-channel.segment-missing"),
        "第二段缺失应让轮次失败，实际 {error:?}"
    );
    assert!(
        crate::tests::common::read_transaction(&conn_b, &first.id).is_some(),
        "失败点之前已提交的重放应留存（不是回滚前缀）"
    );
    assert!(
        crate::tests::common::read_transaction(&conn_b, &second.id).is_none(),
        "失败点之后的 op 不应已应用"
    );
    assert_eq!(
        settings::get::<Option<String>>(&conn_b, SettingKey::SyncLastSyncAt, None).unwrap(),
        None,
        "失败轮次不更新成功时刻"
    );

    // 段重传（内容确定等同，通道纪律）后 B 端重试：第一段的 op 经已知 op / 位点
    // 门幂等跳过（不重复应用），第二段照常应用，整轮成功后成功时刻才落库。
    channel
        .transport()
        .write_file(&second_path, &second_bytes)
        .unwrap();

    let report = run_round_once(&direct(&conn_b), &channel, &EnvelopeMode::Plaintext).unwrap();
    assert!(report.applied >= 1, "补齐段应被应用，实际 {report:?}");
    assert_eq!(
        crate::tests::common::read_transaction(&conn_b, &first.id)
            .unwrap()
            .amount_cents,
        1000,
        "已提交重放幂等续作：第一笔不重复、不变形"
    );
    assert!(
        crate::tests::common::read_transaction(&conn_b, &second.id).is_some(),
        "补齐段落地第二笔"
    );
    assert!(
        settings::get::<Option<String>>(&conn_b, SettingKey::SyncLastSyncAt, None)
            .unwrap()
            .is_some(),
        "整轮成功后成功时刻才落库"
    );
}
