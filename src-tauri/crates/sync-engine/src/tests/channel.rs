use rusqlite::Connection;
use std::sync::{Arc, Mutex as StdMutex};

use crate::channel::{
    ChannelLayout, ChannelManifest, ChannelOptions, ConnSegment, RoundConn, connection_round_key,
    fetch_checkpoint, run_round, run_round_with, upload_checkpoint, upload_checkpoint_with,
};
use crate::envelope::{EnvelopeMode, EnvelopeParams, is_sealed};
use crate::tests::common::{MemoryTransport, direct, make_expense, read_transaction};
use crate::transport::Transport;
use crate::transport::s3::{S3Config, S3Transport};
use crate::{
    OpOutcome, apply_ops, bootstrap_from_checkpoint, create_checkpoint, parked_ops, read_ops,
    stream_positions,
};
use ledger_transaction::write::protocol;
use tauri_app_lib::test_support::{self, seed_account};
use tauri_app_lib::test_support::{S3Addressing, S3Deny, S3Stub, S3StubConfig, spawn_s3_stub};

fn fast_options() -> ChannelOptions {
    ChannelOptions {
        segment_max_ops: 2000,
        envelope: EnvelopeParams {
            kdf_iterations: 2_000,
        },
    }
}

fn layout() -> ChannelLayout {
    ChannelLayout::new("0197abcd-0000-7000-8000-000000000001").unwrap()
}

fn s3_transport(stub: &S3Stub, prefix: &str) -> S3Transport {
    S3Transport::new(S3Config {
        endpoint: stub.endpoint.clone(),
        region: stub.region.clone(),
        bucket: stub.bucket.clone(),
        access_key: stub.access_key.clone(),
        secret_key: "test-secret".to_string(),
        prefix: prefix.to_string(),
        path_style: stub.addressing == S3Addressing::PathStyle,
    })
    .unwrap()
}

fn device_id_of(conn: &Connection) -> String {
    conn.query_row("SELECT id FROM sync_device LIMIT 1", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn round_publishes_own_ops_and_manifest_records_segments() {
    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn, make_expense("acc-1", 10000, "午饭")).unwrap();
    protocol::create(&conn, make_expense("acc-1", 500, "咖啡")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    let report = run_round(&direct(&conn), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    assert_eq!(report.uploaded_segments, 1);
    assert_eq!(report.uploaded_ops, 2);
    assert!(
        report.plaintext_mode,
        "明文模式须在报告中标记（界面显著提示依据）"
    );

    let raw = mem.read_file(&layout.manifest_path()).unwrap().unwrap();
    let manifest: ChannelManifest = serde_json::from_slice(&raw).unwrap();
    assert_eq!(manifest.version, 1);
    assert!(manifest.checkpoint.is_none(), "未发布检查点时指针为空");
    assert_eq!(manifest.streams.len(), 1);
    let segments = &manifest.streams[0].segments;
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].first_clock, 1);
    assert_eq!(segments[0].last_clock, 2);
    assert_eq!(segments[0].sha256.len(), 64);
    assert_eq!(
        mem.read_file(&layout.segment_path(&device_id_of(&conn), 1, 2))
            .unwrap()
            .unwrap()
            .len() as u64,
        segments[0].size
    );
}

#[test]
fn round_pull_applies_foreign_ops_and_is_incremental() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    let created = protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();
    protocol::create(&conn_a, make_expense("acc-1", 500, "咖啡")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    run_round(&direct(&conn_a), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();

    let report = run_round(&direct(&conn_b), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    assert_eq!(report.downloaded_segments, 1);
    assert_eq!(report.applied, 2);
    assert_eq!(
        read_transaction(&conn_b, &created.id),
        read_transaction(&conn_a, &created.id),
        "B 端业务字段与 A 端一致"
    );

    let report = run_round(&direct(&conn_b), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    assert_eq!(report.downloaded_segments, 0);
    assert_eq!(report.applied, 0);

    protocol::create(&conn_a, make_expense("acc-1", 700, "打车")).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    let report = run_round(&direct(&conn_b), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    assert_eq!(report.downloaded_segments, 1);
    assert_eq!(report.applied, 1);
    let ops_a = read_ops(&conn_a).unwrap();
    assert_eq!(ops_a.len(), 3);
    assert_eq!(read_ops(&conn_b).unwrap().len(), 3, "B 端日志与 A 端等量");
}

#[test]
fn two_way_exchange_converges_over_channel() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    let a_txn = protocol::create(&conn_a, make_expense("acc-1", 10000, "A 记的账")).unwrap();
    let b_txn = protocol::create(&conn_b, make_expense("acc-1", 2000, "B 记的账")).unwrap();

    let mem = MemoryTransport::new();
    let mode = EnvelopeMode::Plaintext;
    run_round(&direct(&conn_a), &mem, &layout(), &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout(), &mode).unwrap();
    run_round(&direct(&conn_a), &mem, &layout(), &mode).unwrap();

    for conn in [&conn_a, &conn_b] {
        assert!(read_transaction(conn, &a_txn.id).is_some());
        assert!(read_transaction(conn, &b_txn.id).is_some());
        assert_eq!(read_ops(conn).unwrap().len(), 2, "两端日志等量");
        assert_balance_cache_matches_realtime(conn);
    }
    let ids_a: Vec<String> = read_ops(&conn_a)
        .unwrap()
        .into_iter()
        .map(|o| o.op_id)
        .collect();
    let ids_b: Vec<String> = read_ops(&conn_b)
        .unwrap()
        .into_iter()
        .map(|o| o.op_id)
        .collect();
    assert_eq!(ids_a, ids_b, "两端全序必须逐元素一致");
}

use tauri_app_lib::test_support::assert_balance_cache_matches_realtime;

#[test]
fn concurrent_offline_writes_all_survive_exchange() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 100, "A1")).unwrap();
    protocol::create(&conn_a, make_expense("acc-1", 200, "A2")).unwrap();
    protocol::create(&conn_b, make_expense("acc-1", 300, "B1")).unwrap();
    protocol::create(&conn_b, make_expense("acc-1", 400, "B2")).unwrap();

    let mem = MemoryTransport::new();
    let mode = EnvelopeMode::Plaintext;
    run_round(&direct(&conn_a), &mem, &layout(), &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout(), &mode).unwrap();
    run_round(&direct(&conn_a), &mem, &layout(), &mode).unwrap();

    assert_eq!(read_ops(&conn_a).unwrap().len(), 4);
    assert_eq!(read_ops(&conn_b).unwrap().len(), 4);
    let applied = apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();
    assert!(
        applied.iter().all(|r| r.outcome == OpOutcome::Skipped),
        "交换后双端已收敛：重放全部幂等跳过"
    );
}

#[test]
fn segments_split_by_capacity_and_all_apply() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    for (cents, note) in [(100, "一"), (200, "二"), (300, "三")] {
        protocol::create(&conn_a, make_expense("acc-1", cents, note)).unwrap();
    }

    let mem = MemoryTransport::new();
    let layout = layout();
    let options = ChannelOptions {
        segment_max_ops: 1,
        ..fast_options()
    };
    let report = run_round_with(
        &direct(&conn_a),
        &mem,
        &layout,
        &EnvelopeMode::Plaintext,
        &options,
    )
    .unwrap();
    assert_eq!(report.uploaded_segments, 3, "每段 1 条 op，3 段");

    let manifest: ChannelManifest =
        serde_json::from_slice(&mem.read_file(&layout.manifest_path()).unwrap().unwrap()).unwrap();
    let segments = &manifest.streams[0].segments;
    assert_eq!(segments.len(), 3);
    for (i, segment) in segments.iter().enumerate() {
        assert_eq!(segment.first_clock, i as i64 + 1);
        assert_eq!(segment.last_clock, i as i64 + 1);
        assert!(segment.file.contains(&format!("seg-{:010}", i + 1)));
    }

    let report = run_round_with(
        &direct(&conn_b),
        &mem,
        &layout,
        &EnvelopeMode::Plaintext,
        &options,
    )
    .unwrap();
    assert_eq!(report.downloaded_segments, 3);
    assert_eq!(report.applied, 3);
}

#[test]
fn tampered_segment_is_detected_by_manifest_hash() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 100, "一")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    run_round(&direct(&conn_a), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();

    let device = device_id_of(&conn_a);
    mem.write_file(&layout.segment_path(&device, 1, 1), b"corrupted-bytes")
        .unwrap();

    let err = run_round(&direct(&conn_b), &mem, &layout, &EnvelopeMode::Plaintext).unwrap_err();
    assert!(err.is_code("sync-channel.segment-corrupt"), "实际: {err:?}");
    assert!(read_transaction(&conn_b, "none").is_none());
}

#[test]
fn missing_segment_file_fails_loud() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 100, "一")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    run_round(&direct(&conn_a), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    let device = device_id_of(&conn_a);
    mem.files_remove(&layout.segment_path(&device, 1, 1));

    let err = run_round(&direct(&conn_b), &mem, &layout, &EnvelopeMode::Plaintext).unwrap_err();
    assert!(err.is_code("sync-channel.segment-missing"), "实际: {err:?}");
}

#[test]
fn corrupt_manifest_fails_loud() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 100, "一")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    run_round(&direct(&conn_a), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    mem.write_file(&layout.manifest_path(), b"{broken json")
        .unwrap();

    let err = run_round(&direct(&conn_b), &mem, &layout, &EnvelopeMode::Plaintext).unwrap_err();
    assert!(
        err.is_code("sync-channel.manifest-corrupt"),
        "实际: {err:?}"
    );
}

#[test]
fn encrypted_channel_exchanges_only_ciphertext() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    let created = protocol::create(&conn_a, make_expense("acc-1", 10000, "加密世界的账")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    let mode = EnvelopeMode::Encrypted {
        passphrase: "共享主口令",
    };
    let report = run_round_with(&direct(&conn_a), &mem, &layout, &mode, &fast_options()).unwrap();
    assert!(!report.plaintext_mode);

    let device = device_id_of(&conn_a);
    let raw = mem
        .read_file(&layout.segment_path(&device, 1, 1))
        .unwrap()
        .unwrap();
    assert!(is_sealed(&raw));
    assert!(
        !raw.windows(2).any(|w| w == b"op_"),
        "通道上不含明文载荷片段"
    );

    let wrong = EnvelopeMode::Encrypted {
        passphrase: "错误口令",
    };
    let err = run_round_with(&direct(&conn_b), &mem, &layout, &wrong, &fast_options()).unwrap_err();
    assert!(
        err.is_code("encryption.passphrase-incorrect"),
        "实际: {err:?}"
    );

    let err = run_round(&direct(&conn_b), &mem, &layout, &EnvelopeMode::Plaintext).unwrap_err();
    assert!(
        err.is_code("sync-channel.passphrase-required"),
        "实际: {err:?}"
    );

    let report = run_round_with(&direct(&conn_b), &mem, &layout, &mode, &fast_options()).unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(
        read_transaction(&conn_b, &created.id),
        read_transaction(&conn_a, &created.id)
    );
}

#[test]
fn checkpoint_publish_bootstrap_and_increment_over_channel() {
    let conn_a = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    let first = protocol::create(&conn_a, make_expense("acc-1", 10000, "快照前的账")).unwrap();
    protocol::create(&conn_a, make_expense("acc-1", 500, "快照前的第二笔")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    let mode = EnvelopeMode::Plaintext;
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let frozen = create_checkpoint(&conn_a).unwrap();
    let pointer = upload_checkpoint(&mem, &layout, &mode, &frozen).unwrap();
    assert_eq!(pointer.generation, 1);

    let raw = mem
        .read_file(&layout.checkpoint_file_path(&pointer.file))
        .unwrap()
        .unwrap();
    assert!(!is_sealed(&raw), "明文模式检查点为明文快照");

    let mut conn_c = test_support::open();
    let fetched = fetch_checkpoint(&mem, &layout, None).unwrap();
    assert!(!fetched.sealed, "明文模式检查点应回带未封包标记");
    assert!(!fetched.checkpoint.positions.is_empty());
    assert_eq!(fetched.checkpoint.positions[0].applied_through, 2);
    bootstrap_from_checkpoint(&mut conn_c, &fetched.checkpoint, None).unwrap();
    assert!(
        read_transaction(&conn_c, &first.id).is_some(),
        "快照业务数据就位"
    );

    let late = protocol::create(&conn_a, make_expense("acc-1", 700, "快照后的账")).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let report = run_round(&direct(&conn_c), &mem, &layout, &mode).unwrap();
    assert_eq!(report.applied, 1, "只重放位点之后的增量");
    assert_eq!(
        read_transaction(&conn_c, &late.id),
        read_transaction(&conn_a, &late.id)
    );

    let frozen2 = create_checkpoint(&conn_a).unwrap();
    let pointer2 = upload_checkpoint_with(&mem, &layout, &mode, &frozen2, &fast_options()).unwrap();
    assert_eq!(pointer2.generation, 2);
    assert_ne!(pointer2.file, pointer.file);
    let manifest: ChannelManifest =
        serde_json::from_slice(&mem.read_file(&layout.manifest_path()).unwrap().unwrap()).unwrap();
    assert_eq!(manifest.checkpoint.unwrap().file, pointer2.file);
}

#[test]
fn upload_publishes_frozen_snapshot_pairs_with_positions() {
    let conn_a = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    let first = protocol::create(&conn_a, make_expense("acc-1", 10000, "快照前")).unwrap();

    let frozen = create_checkpoint(&conn_a).unwrap();

    let late = protocol::create(&conn_a, make_expense("acc-1", 700, "上传期间")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    let pointer = upload_checkpoint(&mem, &layout, &EnvelopeMode::Plaintext, &frozen).unwrap();
    assert_eq!(pointer.generation, 1);

    let fetched = fetch_checkpoint(&mem, &layout, None).unwrap();
    assert_eq!(
        fetched.checkpoint.positions[0].applied_through, 1,
        "位点定格在快照时刻"
    );
    let mut conn_c = test_support::open();
    bootstrap_from_checkpoint(&mut conn_c, &fetched.checkpoint, None).unwrap();
    assert!(
        read_transaction(&conn_c, &first.id).is_some(),
        "快照内数据就位"
    );
    assert!(
        read_transaction(&conn_c, &late.id).is_none(),
        "快照后的写入不得随快照就位"
    );

    run_round(&direct(&conn_a), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    let report = run_round(&direct(&conn_c), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    assert_eq!(report.applied, 1, "只重放位点之后的增量");
    assert_eq!(
        read_transaction(&conn_c, &late.id),
        read_transaction(&conn_a, &late.id)
    );
}

#[test]
fn two_end_file_exchange_over_local_s3_stub() {
    let stub = spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle));
    let s3 = s3_transport(&stub, "team/ledger");

    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    let a_txn = protocol::create(&conn_a, make_expense("acc-1", 10000, "A 先记")).unwrap();

    let layout = layout();
    let mode = EnvelopeMode::Plaintext;
    let options = fast_options();

    let report = run_round_with(&direct(&conn_a), &s3, &layout, &mode, &options).unwrap();
    assert_eq!(report.uploaded_segments, 1);
    assert_eq!(report.uploaded_ops, 1);
    let manifest: ChannelManifest =
        serde_json::from_slice(&s3.read_file(&layout.manifest_path()).unwrap().unwrap()).unwrap();
    assert_eq!(
        manifest.streams.len(),
        1,
        "A 轮流发布后 manifest 记录自己的流"
    );
    assert_eq!(manifest.streams[0].device_id, device_id_of(&conn_a));

    let report = run_round_with(&direct(&conn_b), &s3, &layout, &mode, &options).unwrap();
    assert_eq!(report.downloaded_segments, 1);
    assert_eq!(report.applied, 1);
    assert_eq!(
        read_transaction(&conn_b, &a_txn.id),
        read_transaction(&conn_a, &a_txn.id)
    );

    let b_txn = protocol::create(&conn_b, make_expense("acc-1", 2500, "B 后记")).unwrap();
    let report = run_round_with(&direct(&conn_b), &s3, &layout, &mode, &options).unwrap();
    assert_eq!(report.uploaded_segments, 1);
    assert_eq!(
        report.downloaded_segments, 0,
        "位点已覆盖 A 流，不得重复下载"
    );
    let manifest: ChannelManifest =
        serde_json::from_slice(&s3.read_file(&layout.manifest_path()).unwrap().unwrap()).unwrap();
    assert_eq!(manifest.streams.len(), 2, "manifest 归并两侧流");

    let report = run_round_with(&direct(&conn_a), &s3, &layout, &mode, &options).unwrap();
    assert_eq!(report.downloaded_segments, 1);
    assert_eq!(report.applied, 1);
    for conn in [&conn_a, &conn_b] {
        assert_eq!(
            read_transaction(conn, &a_txn.id),
            read_transaction(&conn_a, &a_txn.id)
        );
        assert_eq!(
            read_transaction(conn, &b_txn.id),
            read_transaction(&conn_b, &b_txn.id)
        );
        assert_eq!(read_ops(conn).unwrap().len(), 2);
    }

    let frozen = create_checkpoint(&conn_a).unwrap();
    upload_checkpoint_with(&s3, &layout, &mode, &frozen, &options).unwrap();
    let mut conn_c = test_support::open();
    let fetched = fetch_checkpoint(&s3, &layout, None).unwrap();
    bootstrap_from_checkpoint(&mut conn_c, &fetched.checkpoint, None).unwrap();
    assert_eq!(
        read_transaction(&conn_c, &a_txn.id),
        read_transaction(&conn_a, &a_txn.id)
    );
    assert_eq!(read_ops(&conn_c).unwrap().len(), 2, "引导端含快照携带日志");

    let requests = stub.requests();
    assert!(
        requests.iter().any(
            |request| request.path.starts_with("/ledger-test/team/ledger/")
                && request.path.ends_with("/manifest.json")
        ),
        "同步文件应经配置前缀映射到对象键"
    );
    assert!(
        requests.iter().all(|request| request
            .header("authorization")
            .is_some_and(|value| value.starts_with("AWS4-HMAC-SHA256 "))),
        "轮次中的每个 S3 请求都应带 SigV4 Authorization"
    );
}

#[test]
fn encrypted_exchange_over_local_s3_stub() {
    let stub = spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle));
    let s3 = s3_transport(&stub, "");

    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    let created = protocol::create(&conn_a, make_expense("acc-1", 9900, "密文上云")).unwrap();

    let layout = layout();
    let mode = EnvelopeMode::Encrypted {
        passphrase: "两端共知的口令",
    };
    run_round_with(&direct(&conn_a), &s3, &layout, &mode, &fast_options()).unwrap();

    let manifest: ChannelManifest =
        serde_json::from_slice(&s3.read_file(&layout.manifest_path()).unwrap().unwrap()).unwrap();
    let segment = &manifest.streams[0].segments[0];
    let raw = s3
        .read_file(&layout.stream_file_path(&manifest.streams[0].device_id, &segment.file))
        .unwrap()
        .unwrap();
    assert!(is_sealed(&raw), "对象存储上必须是密文信封");

    let report = run_round_with(&direct(&conn_b), &s3, &layout, &mode, &fast_options()).unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(
        read_transaction(&conn_b, &created.id),
        read_transaction(&conn_a, &created.id)
    );

    let frozen = create_checkpoint(&conn_a).unwrap();
    upload_checkpoint_with(&s3, &layout, &mode, &frozen, &fast_options()).unwrap();
    let fetched = fetch_checkpoint(&s3, &layout, Some("两端共知的口令")).unwrap();
    assert!(fetched.sealed, "密文模式检查点应回带封包标记");
    assert!(!fetched.checkpoint.snapshot.is_empty());
}

#[test]
fn failed_round_leaves_local_ledger_untouched() {
    let stub =
        spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle).deny(S3Deny::InvalidAccessKeyId));
    let s3 = s3_transport(&stub, "");

    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    let before = protocol::create(&conn, make_expense("acc-1", 100, "失败前")).unwrap();

    let err = run_round(&direct(&conn), &s3, &layout(), &EnvelopeMode::Plaintext).unwrap_err();
    assert!(err.is_code("sync-channel.auth-failed"));

    let after = protocol::create(&conn, make_expense("acc-1", 200, "失败后")).unwrap();
    assert_eq!(
        read_transaction(&conn, &before.id).unwrap().amount_cents,
        100
    );
    assert_eq!(
        read_transaction(&conn, &after.id).unwrap().amount_cents,
        200
    );
    assert_balance_cache_matches_realtime(&conn);
}

struct SegmentedConn {
    conn: Arc<StdMutex<Connection>>,
}

impl RoundConn for SegmentedConn {
    fn with_connection<R, F>(
        &self,
        _segment: ConnSegment,
        use_connection: F,
    ) -> ledger_infra::error::Result<R>
    where
        F: FnOnce(&Connection) -> ledger_infra::error::Result<R>,
    {
        let guard = self.conn.lock().unwrap();
        use_connection(&guard)
    }

    fn round_key(&self) -> u64 {
        connection_round_key(&self.conn)
    }
}

struct LockProbeTransport {
    inner: Arc<MemoryTransport>,
    conn: Arc<StdMutex<Connection>>,
    violations: StdMutex<Vec<String>>,
    round_trips: StdMutex<usize>,
}

impl LockProbeTransport {
    fn new(inner: Arc<MemoryTransport>, conn: Arc<StdMutex<Connection>>) -> Self {
        Self {
            inner,
            conn,
            violations: StdMutex::new(Vec::new()),
            round_trips: StdMutex::new(0),
        }
    }

    fn probe(&self, op: &str, path: &str) {
        *self.round_trips.lock().unwrap() += 1;
        if self.conn.try_lock().is_err() {
            self.violations
                .lock()
                .unwrap()
                .push(format!("{op} {path}（网络往返期间连接锁被持有）"));
        }
    }
}

impl Transport for LockProbeTransport {
    fn ensure_dir(&self, path: &str) -> ledger_infra::error::Result<()> {
        self.inner.ensure_dir(path)
    }

    fn read_file(&self, path: &str) -> ledger_infra::error::Result<Option<Vec<u8>>> {
        self.probe("read", path);
        self.inner.read_file(path)
    }

    fn write_file(&self, path: &str, bytes: &[u8]) -> ledger_infra::error::Result<()> {
        self.probe("write", path);
        self.inner.write_file(path, bytes)
    }
}

#[test]
fn network_segments_hold_no_connection_lock() {
    let conn_a = Arc::new(StdMutex::new(test_support::open()));
    let conn_b = Arc::new(StdMutex::new(test_support::open()));
    seed_account(&conn_a.lock().unwrap(), "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b.lock().unwrap(), "acc-1", "现金", "cash", "CNY", 0);
    let created = protocol::create(
        &conn_a.lock().unwrap(),
        make_expense("acc-1", 10000, "A 先记"),
    )
    .unwrap();
    let store = Arc::new(MemoryTransport::new());
    let layout = layout();
    let mode = EnvelopeMode::Plaintext;
    let options = fast_options();

    let probe_a = LockProbeTransport::new(Arc::clone(&store), Arc::clone(&conn_a));
    let report = run_round_with(
        &SegmentedConn {
            conn: Arc::clone(&conn_a),
        },
        &probe_a,
        &layout,
        &mode,
        &options,
    )
    .unwrap();
    assert_eq!(report.uploaded_ops, 1, "A 端应发布一笔 op");
    let trips_a = *probe_a.round_trips.lock().unwrap();
    assert!(
        trips_a >= 2,
        "A 端应有 manifest 读 + 段上传等多次网络往返，实际 {trips_a}"
    );
    assert!(
        probe_a.violations.lock().unwrap().is_empty(),
        "A 端网络往返期间连接锁被持有：{:?}",
        probe_a.violations.lock().unwrap()
    );

    let probe_b = LockProbeTransport::new(Arc::clone(&store), Arc::clone(&conn_b));
    run_round_with(
        &SegmentedConn {
            conn: Arc::clone(&conn_b),
        },
        &probe_b,
        &layout,
        &mode,
        &options,
    )
    .unwrap();
    let trips_b = *probe_b.round_trips.lock().unwrap();
    assert!(
        trips_b >= 2,
        "B 端应有 manifest 读 + 段下载等多次网络往返，实际 {trips_b}"
    );
    assert!(
        probe_b.violations.lock().unwrap().is_empty(),
        "B 端网络往返期间连接锁被持有：{:?}",
        probe_b.violations.lock().unwrap()
    );

    assert!(
        read_transaction(&conn_b.lock().unwrap(), &created.id).is_some(),
        "B 端应经拉取重放收敛 A 端的交易"
    );
}

#[test]
fn checkpoint_upload_fetch_carries_paired_sync_component() {
    let mem = MemoryTransport::new();
    let layout = layout();
    let cp = crate::Checkpoint {
        positions: Vec::new(),
        snapshot: b"business-bytes".to_vec(),
        sync_snapshot: b"sync-bytes".to_vec(),
    };

    let pointer = upload_checkpoint(&mem, &layout, &EnvelopeMode::Plaintext, &cp).unwrap();

    assert_eq!(
        pointer.sync_file.as_deref(),
        Some("cp-000001-sync.enc"),
        "指针携带同步元数据件文件名（同代成对）"
    );
    assert_eq!(pointer.sync_size, Some(b"sync-bytes".len() as u64));
    let raw = mem.read_file(&layout.manifest_path()).unwrap().unwrap();
    assert!(String::from_utf8_lossy(&raw).contains("sync_file"));
    assert_eq!(
        mem.read_file(&layout.checkpoint_sync_path(1))
            .unwrap()
            .unwrap(),
        b"sync-bytes".to_vec()
    );

    let fetched = fetch_checkpoint(&mem, &layout, None).unwrap();
    assert_eq!(fetched.checkpoint.snapshot, b"business-bytes".to_vec());
    assert_eq!(
        fetched.checkpoint.sync_snapshot,
        b"sync-bytes".to_vec(),
        "拉取归一双件"
    );
}

#[test]
fn tampered_sync_component_is_detected_by_pointer_hash() {
    let mem = MemoryTransport::new();
    let layout = layout();
    let cp = crate::Checkpoint {
        positions: Vec::new(),
        snapshot: b"business-bytes".to_vec(),
        sync_snapshot: b"sync-bytes".to_vec(),
    };
    upload_checkpoint(&mem, &layout, &EnvelopeMode::Plaintext, &cp).unwrap();
    mem.write_file(&layout.checkpoint_sync_path(1), b"tampered-sync-bytes")
        .unwrap();

    let err = fetch_checkpoint(&mem, &layout, None).unwrap_err();
    assert!(
        err.is_code("sync-channel.checkpoint-corrupt"),
        "同步件 hash 不符按检查点损坏报错"
    );
}

#[test]
fn legacy_single_file_checkpoint_publishes_and_fetches_without_sync_fields() {
    let mem = MemoryTransport::new();
    let layout = layout();
    let cp = crate::Checkpoint {
        positions: Vec::new(),
        snapshot: b"legacy-business-bytes".to_vec(),
        sync_snapshot: Vec::new(),
    };

    let pointer = upload_checkpoint(&mem, &layout, &EnvelopeMode::Plaintext, &cp).unwrap();
    assert!(pointer.sync_file.is_none(), "旧形态指针无 sync_* 字段");
    let raw = mem.read_file(&layout.manifest_path()).unwrap().unwrap();
    assert!(
        !String::from_utf8_lossy(&raw).contains("sync_file"),
        "旧形态 manifest 不出现新字段"
    );
    assert!(
        mem.read_file(&layout.checkpoint_sync_path(1))
            .unwrap()
            .is_none()
    );

    let fetched = fetch_checkpoint(&mem, &layout, None).unwrap();
    assert_eq!(
        fetched.checkpoint.snapshot,
        b"legacy-business-bytes".to_vec()
    );
    assert!(
        fetched.checkpoint.sync_snapshot.is_empty(),
        "旧形态拉取归一为空同步件"
    );
}

#[test]
fn manifest_without_sync_fields_parses() {
    let raw = format!(
        r#"{{
  "version": 1,
  "streams": [],
  "checkpoint": {{
    "file": "cp-000001.enc",
    "generation": 1,
    "size": 128,
    "sha256": "abc123",
    "created_at": "{}"
  }}
}}"#,
        tauri_app_lib::test_support::FIXED_NOW
    );
    let manifest: ChannelManifest = serde_json::from_slice(raw.as_bytes()).unwrap();
    let pointer = manifest.checkpoint.expect("指针在场");
    assert_eq!(pointer.sync_file, None, "旧形态指针缺省 None");
    assert_eq!(pointer.sync_size, None);
    assert_eq!(pointer.sync_sha256, None);
}

fn stream_log_len(conn: &Connection, device_id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM sync_ops WHERE device_id = ?1",
        [device_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn position_of_stream(conn: &Connection, device_id: &str) -> i64 {
    stream_positions(conn)
        .unwrap()
        .into_iter()
        .find(|p| p.device_id == device_id)
        .expect("位点行在列")
        .applied_through
}

fn manifest_bytes(mem: &MemoryTransport, layout: &ChannelLayout) -> Vec<u8> {
    mem.read_file(&layout.manifest_path()).unwrap().unwrap()
}

#[test]
fn round_writeback_declares_applied_positions_in_manifest() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();
    protocol::create(&conn_a, make_expense("acc-1", 500, "咖啡")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    let mode = EnvelopeMode::Plaintext;

    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let dev_a = device_id_of(&conn_a);
    let own: ChannelManifest = serde_json::from_slice(&manifest_bytes(&mem, &layout)).unwrap();
    let own_entry = &own.streams[0];
    assert_eq!(own_entry.device_id, dev_a);
    assert_eq!(
        own_entry.applied_positions.get(&dev_a),
        Some(&2),
        "A 轮回写顺带声明自己流的已应用位点"
    );

    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    let dev_b = device_id_of(&conn_b);
    let declared: ChannelManifest = serde_json::from_slice(&manifest_bytes(&mem, &layout)).unwrap();
    let declared = declared.streams[0].applied_positions.clone();
    assert_eq!(
        declared.get(&dev_b),
        Some(&2),
        "B 轮回写顺带声明对 A 流的已应用位点"
    );

    protocol::create(&conn_a, make_expense("acc-1", 700, "打车")).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let after: ChannelManifest = serde_json::from_slice(&manifest_bytes(&mem, &layout)).unwrap();
    let after = after.streams[0].applied_positions.clone();
    assert_eq!(
        after.get(&dev_b),
        Some(&2),
        "流属主段覆写不得清除他人位点声明"
    );
    assert_eq!(after.get(&dev_a), Some(&3), "自己的声明随后续轮次推进");
}

#[test]
fn safe_watermark_truncates_own_stream_after_peer_declaration_and_checkpoint() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    let first = protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();
    protocol::create(&conn_a, make_expense("acc-1", 500, "咖啡")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    let mode = EnvelopeMode::Plaintext;

    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let dev_a = device_id_of(&conn_a);
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    let pointer =
        upload_checkpoint(&mem, &layout, &mode, &create_checkpoint(&conn_b).unwrap()).unwrap();
    assert_eq!(
        pointer.applied_positions.get(&dev_a),
        Some(&2),
        "指针携带产出端快照时刻的位点覆盖"
    );
    assert_eq!(stream_log_len(&conn_a, &dev_a), 2, "截断前日志完整");

    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        0,
        "水位之前的 op（整流）已截断"
    );
    assert_eq!(
        position_of_stream(&conn_a, &dev_a),
        2,
        "位点留存不回退（水位不因日志缩短而回退）"
    );
    assert!(
        read_transaction(&conn_a, &first.id).is_some(),
        "截断只删日志行，业务数据不受影响"
    );
    let dev_b = device_id_of(&conn_b);
    assert_eq!(
        serde_json::from_slice::<ChannelManifest>(&manifest_bytes(&mem, &layout))
            .unwrap()
            .streams[0]
            .applied_positions
            .get(&dev_b),
        Some(&2)
    );

    let late = protocol::create(&conn_a, make_expense("acc-1", 700, "打车")).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    assert_eq!(
        read_transaction(&conn_b, &late.id).unwrap().amount_cents,
        700,
        "截断后同步不断"
    );
}

#[test]
fn truncation_defers_until_every_participant_has_declared() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    let conn_c = test_support::open();
    for conn in [&conn_a, &conn_b, &conn_c] {
        seed_account(conn, "acc-1", "现金", "cash", "CNY", 0);
    }
    protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();
    protocol::create(&conn_a, make_expense("acc-1", 500, "咖啡")).unwrap();
    protocol::create(&conn_c, make_expense("acc-1", 300, "C 的账")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    let mode = EnvelopeMode::Plaintext;

    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let dev_a = device_id_of(&conn_a);
    run_round(&direct(&conn_c), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    let pointer =
        upload_checkpoint(&mem, &layout, &mode, &create_checkpoint(&conn_b).unwrap()).unwrap();
    assert_eq!(pointer.applied_positions.get(&dev_a), Some(&2));

    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        2,
        "参与端声明缺失 → 水位不可证，不截"
    );

    run_round(&direct(&conn_c), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_c), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        0,
        "全部参与端声明齐备后按水位截断"
    );
}

#[test]
fn truncation_is_bounded_by_stale_declaration_and_checkpoint_coverage() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 10000, "一")).unwrap();
    protocol::create(&conn_a, make_expense("acc-1", 500, "二")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    let mode = EnvelopeMode::Plaintext;

    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let dev_a = device_id_of(&conn_a);
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    upload_checkpoint(&mem, &layout, &mode, &create_checkpoint(&conn_b).unwrap()).unwrap();

    protocol::create(&conn_a, make_expense("acc-1", 700, "三")).unwrap();
    protocol::create(&conn_a, make_expense("acc-1", 900, "四")).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        2,
        "水位 = min(陈旧声明 2, 覆盖 2)：只截位点之前，新 op 不越界被截"
    );

    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    upload_checkpoint(&mem, &layout, &mode, &create_checkpoint(&conn_b).unwrap()).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(stream_log_len(&conn_a, &dev_a), 0, "证据推进后按新水位截断");
}

#[test]
fn single_device_world_never_truncates() {
    let conn_a = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();
    protocol::create(&conn_a, make_expense("acc-1", 500, "咖啡")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    let mode = EnvelopeMode::Plaintext;

    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let dev_a = device_id_of(&conn_a);
    upload_checkpoint(&mem, &layout, &mode, &create_checkpoint(&conn_a).unwrap()).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();

    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        2,
        "单端世界无对端声明，水位不可证，不截"
    );
}

#[test]
fn truncation_requires_reachable_checkpoint_coverage_evidence() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();
    protocol::create(&conn_a, make_expense("acc-1", 500, "咖啡")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    let mode = EnvelopeMode::Plaintext;

    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let dev_a = device_id_of(&conn_a);
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        2,
        "无检查点指针（无并入证据）→ 不截"
    );

    upload_checkpoint(
        &mem,
        &layout,
        &mode,
        &crate::Checkpoint {
            positions: Vec::new(),
            snapshot: b"legacy".to_vec(),
            sync_snapshot: Vec::new(),
        },
    )
    .unwrap();
    let manifest: ChannelManifest = serde_json::from_slice(&manifest_bytes(&mem, &layout)).unwrap();
    assert!(
        manifest
            .checkpoint
            .as_ref()
            .unwrap()
            .applied_positions
            .is_empty(),
        "空覆盖序列化时省略字段（旧形态指针无位点）"
    );
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        2,
        "旧形态指针覆盖不可证 → 不截"
    );
}

#[test]
fn parked_op_on_peer_pins_owner_truncation_watermark() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    let t1 = protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    let mode = EnvelopeMode::Plaintext;

    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let dev_a = device_id_of(&conn_a);
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    protocol::delete(&conn_b, &t1.id).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    protocol::update(&conn_a, &t1.id, make_expense("acc-1", 12000, "A 改")).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    assert_eq!(
        parked_ops(&conn_b).unwrap().len(),
        1,
        "A 的更新 op 在 B 挂起"
    );
    assert_eq!(
        position_of_stream(&conn_b, &dev_a),
        1,
        "B 对 A 流的位点被挂起 op 钉住"
    );

    upload_checkpoint(&mem, &layout, &mode, &create_checkpoint(&conn_b).unwrap()).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        1,
        "只截位点之前的 op；被 B 挂起的更新 op 不被截"
    );

    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1, "挂起行幂等覆盖");
    assert_eq!(position_of_stream(&conn_b, &dev_a), 1, "位点不越过挂起 op");
}

#[test]
fn failed_round_does_not_truncate() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    for note in ["一", "二", "三"] {
        protocol::create(&conn_a, make_expense("acc-1", 100, note)).unwrap();
    }

    let mem = MemoryTransport::new();
    let layout = layout();
    let mode = EnvelopeMode::Plaintext;

    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let dev_a = device_id_of(&conn_a);
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    let dev_b = device_id_of(&conn_b);
    protocol::create(&conn_b, make_expense("acc-1", 200, "B 的账")).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    upload_checkpoint(&mem, &layout, &mode, &create_checkpoint(&conn_b).unwrap()).unwrap();

    protocol::create(&conn_b, make_expense("acc-1", 300, "B 的第二笔")).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    mem.files_remove(&layout.segment_path(&dev_b, 2, 2));

    let err = run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap_err();
    assert!(err.is_code("sync-channel.segment-missing"), "拉取段失败");
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        3,
        "失败轮次不触发截断：若轮次成功，水位 3 会截掉 op1-3，失败后原样保留"
    );
}
