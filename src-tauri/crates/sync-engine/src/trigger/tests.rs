use crate::EnvelopeMode;
use crate::SyncChannelConfig;
use crate::tests::common::direct;
use crate::tests::common::make_expense;
use crate::trigger::{SessionEnvelope, build_channel, configured_channel, run_auto_round};
use ledger_infra::settings::{self, SettingKey};
use ledger_transaction::write::protocol;
use tauri_app_lib::test_support::{self, seed_account};

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

#[test]
fn missing_space_id_deserializes_to_default() {
    let config: SyncChannelConfig =
        serde_json::from_str(r#"{"endpoint":"https://s3.example.com"}"#).unwrap();
    assert_eq!(config.space_id, "default");
    assert_eq!(config.endpoint, "https://s3.example.com");
}

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

#[test]
fn unknown_config_keys_are_ignored() {
    let config: SyncChannelConfig = serde_json::from_str(
        r#"{"endpoint":"https://s3.example.com","bucket":"ledger","future_option":true}"#,
    )
    .unwrap();
    assert_eq!(config.endpoint, "https://s3.example.com");
    assert_eq!(config.bucket, "ledger");
}

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
    let err = match build_channel(&config) {
        Ok(_) => panic!("非法空间应被拒"),
        Err(e) => e,
    };
    assert!(err.is_code("sync-channel.book-id-invalid"), "实际: {err:?}");
}

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

    SessionEnvelope::forget();
    assert_eq!(SessionEnvelope::current(), SessionEnvelope::Plaintext);
    assert_eq!(
        SessionEnvelope::Plaintext.mode(),
        EnvelopeMode::Plaintext,
        "明文形态对应的信封模式是明文直通"
    );
}

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

#[test]
fn write_after_sync_debounce_runs_one_round_per_burst() {
    let window = std::time::Duration::from_millis(80);

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

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    tx.send(()).unwrap();
    assert!(rx.recv_timeout(window).is_ok(), "外层先消费首个写信号");
    drop(tx);
    crate::trigger::drain_write_signals(&rx, window);
}

#[test]
fn write_after_sync_without_scheduler_is_a_noop() {
    crate::sync_after_write();
}

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

    let empty: OnceLock<std::sync::mpsc::Sender<()>> = OnceLock::new();
    crate::trigger::notify_write_signal(&empty);
}

use crate::RoundConn;
use crate::channel::SyncRoundReport;
use crate::trigger::run_round_once;
use std::sync::{Arc, Mutex};

fn begin_expect(key: u64) -> crate::trigger::round_gate::RoundHandle {
    match crate::trigger::round_gate::begin_round(key) {
        crate::trigger::round_gate::RoundStart::Began(handle) => handle,
        crate::trigger::round_gate::RoundStart::InFlight(_) => {
            panic!("空闲键上登记应成功")
        }
    }
}

fn memory_channel() -> crate::trigger::SyncChannel {
    use crate::tests::common::MemoryTransport;
    crate::trigger::SyncChannel::from_parts(
        Box::new(MemoryTransport::new()),
        crate::ChannelLayout::new("default").unwrap(),
    )
}

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

    let key = crate::DirectConn::new(&conn).round_key();

    let handle = begin_expect(key);

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

#[test]
fn manual_round_waits_and_reuses_in_flight_report() {
    let conn = test_support::open();
    let channel = memory_channel();

    let key = crate::DirectConn::new(&conn).round_key();
    let handle = begin_expect(key);
    let completer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        handle.complete(Ok(SyncRoundReport {
            uploaded_segments: 9,
            ..SyncRoundReport::default()
        }));
    });

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

#[test]
fn in_flight_mutex_keys_by_connection_identity() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    let key_a = crate::DirectConn::new(&conn_a).round_key();
    let key_b = crate::DirectConn::new(&conn_b).round_key();
    assert_ne!(key_a, key_b, "不同连接是不同库，身份键应不同");

    let handle_a = begin_expect(key_a);
    assert!(matches!(
        crate::trigger::round_gate::begin_round(key_a),
        crate::trigger::round_gate::RoundStart::InFlight(_)
    ));
    let handle_b = begin_expect(key_b);
    handle_a.complete(Ok(SyncRoundReport::default()));
    handle_b.complete(Ok(SyncRoundReport::default()));
}

#[test]
fn auto_round_gives_up_silently_when_connection_lock_is_busy() {
    let conn = Arc::new(Mutex::new(test_support::open()));
    let locks = super::scheduler::AutoRoundConn::new(&conn)
        .with_lock_timeout(std::time::Duration::from_millis(10));

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

    let outcome = super::scheduler::run_auto_round(&locks, &SessionEnvelope::Plaintext).unwrap();
    assert!(outcome.is_none(), "未配置通道：零动作");
}

#[test]
fn auto_round_source_probes_lock_hold_past_threshold() {
    use ledger_infra::test_utils::capture_events;
    use tracing::Level;

    let conn = Arc::new(Mutex::new(test_support::open()));
    let locks = super::scheduler::AutoRoundConn::new(&conn)
        .with_probe_threshold(std::time::Duration::from_millis(20));

    let events = capture_events(|| {
        locks
            .with_connection(crate::channel::ConnSegment::Read, |_| {
                std::thread::sleep(std::time::Duration::from_millis(30));
                Ok(())
            })
            .unwrap();
    });
    assert!(
        events.iter().any(|e| e.level == Level::WARN),
        "段内持锁超阈值应记 warn（探针覆盖轮次分段），实际捕获 {events:?}"
    );
}

#[test]
fn failed_round_does_not_stamp_last_sync() {
    use crate::tests::common::{MemoryTransport, seed_device};
    use crate::transport::Transport;

    let conn = test_support::open();
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
    let second_file = manifest.streams[0].segments[1].file.clone();
    let second_path = layout.stream_file_path("dev-a", &second_file);
    let second_bytes = transport.read_file(&second_path).unwrap().unwrap();
    transport.files_remove(&second_path);

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
