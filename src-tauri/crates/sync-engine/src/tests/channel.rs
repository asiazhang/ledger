//! 通道层（issue #859 / ADR-0091 决策 1/8）：通道目录布局与 manifest、同步
//! 轮次（发布自己流 + 拉取他人流）、SyncEnvelope 加密形态、Checkpoint 通道
//! 传递与新端引导，以及本地 S3 桩上的两端文件交换集成。
//!
//! 快速场景走内存假通道；HTTP 语义（建目录、凭据、两端真实文件交换）走
//! S3 桩（AC：本地通道桩上的两端文件交换集成测试）。

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

/// 测试用低 KDF 迭代选项（信封格式语义与派生成本无关；生产默认值已在
/// envelope 套件钉住）。
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

/// 可用 S3 传输（path-style 对本地桩；对象键带前缀）。
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

/// 读取本机设备标识（判定依据读取，非夹具）。
fn device_id_of(conn: &Connection) -> String {
    conn.query_row("SELECT id FROM sync_device LIMIT 1", [], |r| r.get(0))
        .unwrap()
}

/// A 端两笔账 → 单端发布：通道上出现自己的段文件，manifest 记录段清单
///（区间 + 尺寸 + hash），报告如实计数并标记明文模式。
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

    // manifest：一个流一条目，段区间与 hash 记录齐备。
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

/// B 端拉取：A 的交易落到 B；重跑轮次幂等（位点门拦截，无重复下载与重放）；
/// A 增量再发布后 B 只拉到新段。
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

    // 幂等：位点已覆盖全部段，重跑无下载、无重放。
    let report = run_round(&direct(&conn_b), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    assert_eq!(report.downloaded_segments, 0);
    assert_eq!(report.applied, 0);

    // 增量：A 新增一笔 → 新段；B 只拉新段。
    protocol::create(&conn_a, make_expense("acc-1", 700, "打车")).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    let report = run_round(&direct(&conn_b), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    assert_eq!(report.downloaded_segments, 1);
    assert_eq!(report.applied, 1);
    let ops_a = read_ops(&conn_a).unwrap();
    assert_eq!(ops_a.len(), 3);
    assert_eq!(read_ops(&conn_b).unwrap().len(), 3, "B 端日志与 A 端等量");
}

/// 双向交换收敛：两端各写一笔、通道交换后两端日志全序一致、业务状态一致、
/// 派生缓存自洽（AC：数据真正在设备之间流动）。
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
    run_round(&direct(&conn_b), &mem, &layout(), &mode).unwrap(); // B 拉 A + 发布自己
    run_round(&direct(&conn_a), &mem, &layout(), &mode).unwrap(); // A 拉 B

    for conn in [&conn_a, &conn_b] {
        assert!(read_transaction(conn, &a_txn.id).is_some());
        assert!(read_transaction(conn, &b_txn.id).is_some());
        assert_eq!(read_ops(conn).unwrap().len(), 2, "两端日志等量");
        assert_balance_cache_matches_realtime(conn);
    }
    // 全序一致：两端对同一批 op 排出唯一一致的顺序（read_ops 即全序返回，
    // 元素级比较同时证明集合相等与顺序一致）。
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

/// 双端先各自离线写、后一次性交换：并发 op 全部存活（零丢失经通道路径成立）。
#[test]
fn concurrent_offline_writes_all_survive_exchange() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    // 离线：两端各写两笔（互相不可见）。
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

/// 段容量切分：每次封包至多 N 条 op，按序号区间命名；对端分段拉取全部应用。
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

/// 段文件被篡改：hash/尺寸自校验拦截，报可重试的码化错误（内容自校验兜底
/// 通道弱原子性），不静默应用损坏内容。
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

/// manifest 记录的段在通道上缺失（清单先行/文件未到齐）：报明确错误，
/// 不静默跳过造成日志缺口（零丢失：缺口必须显性失败等待重试）。
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
    // 直接抹掉段文件（manifest 仍在）。
    mem.files_remove(&layout.segment_path(&device, 1, 1));

    let err = run_round(&direct(&conn_b), &mem, &layout, &EnvelopeMode::Plaintext).unwrap_err();
    assert!(err.is_code("sync-channel.segment-missing"), "实际: {err:?}");
}

/// manifest 损坏（非 JSON）：报 manifest 损坏，不静默按空清单处理
///（不静默兜底：按空处理会让对端误以为没有新数据）。
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

/// 加密通道：通道上只有密文（段文件带信封魔数），对端凭主口令解开并应用；
/// 缺口令/错口令报可重试的码化错误（AC：整包加密、主口令不随信封走）。
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

    // 通道上是密文：不是合法 JSON 的 op 明文，且带信封魔数。
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

    // 错口令：合并口径码化错误（与密文备份恢复同款）。
    let wrong = EnvelopeMode::Encrypted {
        passphrase: "错误口令",
    };
    let err = run_round_with(&direct(&conn_b), &mem, &layout, &wrong, &fast_options()).unwrap_err();
    assert!(
        err.is_code("encryption.passphrase-incorrect"),
        "实际: {err:?}"
    );

    // 明文模式对端拉到密文：缺口令报可重试错误（混合世界显性失败）。
    let err = run_round(&direct(&conn_b), &mem, &layout, &EnvelopeMode::Plaintext).unwrap_err();
    assert!(
        err.is_code("sync-channel.passphrase-required"),
        "实际: {err:?}"
    );

    // 正确口令：解开并应用。
    let report = run_round_with(&direct(&conn_b), &mem, &layout, &mode, &fast_options()).unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(
        read_transaction(&conn_b, &created.id),
        read_transaction(&conn_a, &created.id)
    );
}

/// Checkpoint 通道传递：A 发布检查点（代数自增、manifest 换指针）→ 全新端
/// 拉取并引导 → 引导后继续增量同步（AC：凭主口令在另一端解开/新端引导经通道）。
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

    // 全新端：拉取 + 引导（目标尚未参与同步）。
    let mut conn_c = test_support::open();
    let fetched = fetch_checkpoint(&mem, &layout, None).unwrap();
    assert!(!fetched.sealed, "明文模式检查点应回带未封包标记");
    // 检查点两半同刻到达：快照字节 + 各流位点（A 端两笔 op 的流头）。
    assert!(!fetched.checkpoint.positions.is_empty());
    assert_eq!(fetched.checkpoint.positions[0].applied_through, 2);
    bootstrap_from_checkpoint(&mut conn_c, &fetched.checkpoint, None).unwrap();
    assert!(
        read_transaction(&conn_c, &first.id).is_some(),
        "快照业务数据就位"
    );

    // A 增量一笔 → C 只重放检查点之后的 op。
    let late = protocol::create(&conn_a, make_expense("acc-1", 700, "快照后的账")).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    let report = run_round(&direct(&conn_c), &mem, &layout, &mode).unwrap();
    assert_eq!(report.applied, 1, "只重放位点之后的增量");
    assert_eq!(
        read_transaction(&conn_c, &late.id),
        read_transaction(&conn_a, &late.id)
    );

    // 第二次发布：代数推进、指针换到新文件。
    let frozen2 = create_checkpoint(&conn_a).unwrap();
    let pointer2 = upload_checkpoint_with(&mem, &layout, &mode, &frozen2, &fast_options()).unwrap();
    assert_eq!(pointer2.generation, 2);
    assert_ne!(pointer2.file, pointer.file);
    let manifest: ChannelManifest =
        serde_json::from_slice(&mem.read_file(&layout.manifest_path()).unwrap().unwrap()).unwrap();
    assert_eq!(manifest.checkpoint.unwrap().file, pointer2.file);
}

/// 发布段只消费已定格的快照字节（#1284 判据：成对约束只为产出段而立）：产出
/// 段放锁后连接上的新写入不进本次发布的快照，位点与快照仍同刻成对——引导端
/// 不重放快照内的 op，快照后的增量照常重放（快照 + 其后 op = 一致状态）。
#[test]
fn upload_publishes_frozen_snapshot_pairs_with_positions() {
    let conn_a = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    let first = protocol::create(&conn_a, make_expense("acc-1", 10000, "快照前")).unwrap();

    // 产出段（真实调用方由主连接锁保证互斥）：快照与位点在此同刻定格。
    let frozen = create_checkpoint(&conn_a).unwrap();

    // 模拟封包/上传期间的并发写入（发布段不持锁，本地写不被挡）：这笔 op 晚于
    // 快照，不得进本次发布的快照与位点。
    let late = protocol::create(&conn_a, make_expense("acc-1", 700, "上传期间")).unwrap();

    // 发布段只拿快照字节：不触连接，快照字节不随后续写入漂移。
    let mem = MemoryTransport::new();
    let layout = layout();
    let pointer = upload_checkpoint(&mem, &layout, &EnvelopeMode::Plaintext, &frozen).unwrap();
    assert_eq!(pointer.generation, 1);

    // 引导端拿到的是定格快照：快照前那笔在、上传期间那笔不在；位点定格在快照
    // 时刻的流头（两半同刻成对）。
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

    // 快照后的增量经下一轮重放照常到达：成对性不因两段拆分而破。
    run_round(&direct(&conn_a), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    let report = run_round(&direct(&conn_c), &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    assert_eq!(report.applied, 1, "只重放位点之后的增量");
    assert_eq!(
        read_transaction(&conn_c, &late.id),
        read_transaction(&conn_a, &late.id)
    );
}

/// AC 集成：本地 S3 桩上的同步轮次——段发布、manifest 归并、增量拉取与
/// 检查点发布/第三端引导都跑在真实 HTTP 对象存储语义上。
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

    // A 发布：段文件 + manifest 归并上对象存储。
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

    // B 增量拉取 A 的段并重放。
    let report = run_round_with(&direct(&conn_b), &s3, &layout, &mode, &options).unwrap();
    assert_eq!(report.downloaded_segments, 1);
    assert_eq!(report.applied, 1);
    assert_eq!(
        read_transaction(&conn_b, &a_txn.id),
        read_transaction(&conn_a, &a_txn.id)
    );

    // B 回发自己的段：manifest 归并出两条流；A 已覆盖的段不再下载。
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

    // A 增量拉取 B 的段。
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

    // 检查点经 S3 发布并由第三端引导。
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

/// 加密模式的 S3 端到端：通道上只有密文，对端凭口令解开（AC：同步包以
/// 加密信封整体上云、凭主口令在另一端解开）。
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

    // 密文检查点拉取回带封包标记（引导端对齐本库加密形态的依据，#864）。
    let frozen = create_checkpoint(&conn_a).unwrap();
    upload_checkpoint_with(&s3, &layout, &mode, &frozen, &fast_options()).unwrap();
    let fetched = fetch_checkpoint(&s3, &layout, Some("两端共知的口令")).unwrap();
    assert!(fetched.sealed, "密文模式检查点应回带封包标记");
    assert!(!fetched.checkpoint.snapshot.is_empty());
}

/// 同步失败不影响本地记账（AC：凭据/网络失败明确可重试，本地旁路不受扰）。
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

    // 本地记账照常：同步失败后写入成功、既有数据原样。
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

// ---------------------------------------------------------------------------
// 网络段出锁（issue #1339 / ADR-0120 决策 1/2）：网络段不持连接锁——负向判据
// ---------------------------------------------------------------------------

/// 测试用轮次连接源：每段对共享互斥体短取一次锁（生产 `AutoRoundConn` 同型，
/// 阻塞等待无放弃口径），用完即还。
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

/// 探测型 Transport 包装：每次网络往返现场试取一次连接锁——锁被持有即记一次
/// 违例（「网络段持连接锁」的直接证据）。同时记录往返次数，防断言空转。
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

/// 网络段不持连接锁（ADR-0120 决策 1/2 负向判据，ADR-0087）：发布与拉取的
/// 每一次网络往返（manifest 读写、段上传、段下载）现场试取连接锁都必须可得
/// ——把网络段移回整轮持锁（本票修复前的形状），违规清单非空，本测试即红。
/// 往返计数同时防断言空转（通道上没有任何网络往返时「零违例」是假绿）。
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

    // A 端发布：manifest 读 + 段上传 + manifest 写回，全程网络段出锁。
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

    // B 端拉取：manifest 读 + 段下载（网络段出锁），重放段各自短取锁。
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

    // 数据语义不因分段取锁改变：A 端的 op 经拉取 + 重放在 B 端收敛。
    assert!(
        read_transaction(&conn_b.lock().unwrap(), &created.id).is_some(),
        "B 端应经拉取重放收敛 A 端的交易"
    );
}

// ---------------------------------------------------------------------------
// 双文件成对检查点（票 04 / ADR-0139 决策 5）：manifest 双指针、双件独立封包
// 上通道、拉取归一；旧形态单文件指针与新字段缺省的双向兼容。
// ---------------------------------------------------------------------------

/// 双文件成对发布与拉取：指针携带 sync_* 字段、同步件独立成通道文件、拉取
/// 所得 Checkpoint 携带双件字节（明文模式字节直通，封包归 envelope 套件）。
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
    // manifest 原文携带新字段（旧版端忽略之，互操作不破坏——serde 未知字段容忍）。
    let raw = mem.read_file(&layout.manifest_path()).unwrap().unwrap();
    assert!(String::from_utf8_lossy(&raw).contains("sync_file"));
    // 同步件独立成文件、字节即同步件（明文直通）。
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

/// 同步件通道损坏（hash 不符）：与业务件同款校验，报检查点损坏，不静默。
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

/// 旧形态单文件检查点（同步件为空，票 04 前产物 / 兼容替身）：发布退化为旧
/// 形态——不写 sync 件、指针无 sync_* 字段；拉取按旧形态归一（同步件为空，
/// 引导按单库形态分支）。
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

/// manifest 向后兼容（验收判据）：只有原字段的旧清单不受影响——checkpoint
/// 指针缺 sync_* 字段按 None 容忍（serde 缺省回落）。
#[test]
fn manifest_without_sync_fields_parses() {
    // created_at 走工厂固定时刻（ADR-0084：时刻值收敛 test_support）。
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

// ---------------------------------------------------------------------------
// OpLog 截断启用（#1874 / ADR-0139 决策 8）：manifest 位点声明（写接线）+
// 安全水位（读接线 = 各端声明最小值 ∩ 对端可达 Checkpoint 覆盖）+ 成功轮次
// 落库段自动截断。判据一律对准可观察结果（本机日志行数、manifest 声明面、
// 位点留存），不对准函数调用形状（ADR-0087）。
// ---------------------------------------------------------------------------

/// 指定来源设备在本机日志中的 op 数（截断判据读取）。
fn stream_log_len(conn: &Connection, device_id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM sync_ops WHERE device_id = ?1",
        [device_id],
        |r| r.get(0),
    )
    .unwrap()
}

/// 本端对指定流的已应用位点（位点留存判据读取；按 device_id 定位，
/// #1112 同款纪律——位点清单按 DeviceId 序，下标随 UUID 生成顺序漂移）。
fn position_of_stream(conn: &Connection, device_id: &str) -> i64 {
    stream_positions(conn)
        .unwrap()
        .into_iter()
        .find(|p| p.device_id == device_id)
        .expect("位点行在列")
        .applied_through
}

/// 读通道 manifest 原文字节（判据读取）：解析在调用点以类型标注承载——
/// `-> ChannelManifest {` 形态是测试守门规则 4（自建通道线格式，文本级扫描）
/// 的命中形态，判据读取不复用该形态。
fn manifest_bytes(mem: &MemoryTransport, layout: &ChannelLayout) -> Vec<u8> {
    mem.read_file(&layout.manifest_path()).unwrap().unwrap()
}

/// 位点声明的写接线（负向判据，ADR-0087）：各端每轮回写 manifest 顺带声明本机
/// 对各流的已应用位点——A 发布后声明自己流的位点；B 拉取后把自己对 A 流的
/// 位点声明写上 manifest；A 下一轮覆写自己流（段清单本地权威）不得清除 B 的
/// 声明。删除轮次内的声明调用，本测试红。
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
    // 声明采集在轮回写点（拉取段之前）：首轮拉取的位点在下一轮回写时声明
    // （一轮滞后，安全侧保守）。
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    let dev_b = device_id_of(&conn_b);
    let declared: ChannelManifest = serde_json::from_slice(&manifest_bytes(&mem, &layout)).unwrap();
    let declared = declared.streams[0].applied_positions.clone();
    assert_eq!(
        declared.get(&dev_b),
        Some(&2),
        "B 轮回写顺带声明对 A 流的已应用位点"
    );

    // A 覆写自己流：段清单本地权威，他人声明原样保留。
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

/// 达到安全水位自动截断自己的流（负向判据，ADR-0087）：B 声明 + Checkpoint
/// 覆盖（位点上 manifest）构成水位证据，A 成功轮次落库段按水位截断自己的流
/// ——位点之前全删、位点留存不回退、业务数据不受影响、截断后同步继续。
/// 删除轮次内的截断调用，本测试红。
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
    // B 发布检查点：位点覆盖随指针上 manifest（截断的「已并入对端可达」证据）。
    let pointer =
        upload_checkpoint(&mem, &layout, &mode, &create_checkpoint(&conn_b).unwrap()).unwrap();
    assert_eq!(
        pointer.applied_positions.get(&dev_a),
        Some(&2),
        "指针携带产出端快照时刻的位点覆盖"
    );
    assert_eq!(stream_log_len(&conn_a, &dev_a), 2, "截断前日志完整");

    // A 成功轮次：水位 = min(B 声明 2, 覆盖 2) = 2 → 自动截断自己的流。
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
    // 他人声明在 A 的覆写与截断轮次后仍在（水位证据持续）。
    let dev_b = device_id_of(&conn_b);
    assert_eq!(
        serde_json::from_slice::<ChannelManifest>(&manifest_bytes(&mem, &layout))
            .unwrap()
            .streams[0]
            .applied_positions
            .get(&dev_b),
        Some(&2)
    );

    // 截断后同步继续：A 新 op 正常发布（自截掉的时钟之后），B 照常应用。
    let late = protocol::create(&conn_a, make_expense("acc-1", 700, "打车")).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    assert_eq!(
        read_transaction(&conn_b, &late.id).unwrap().amount_cents,
        700,
        "截断后同步不断"
    );
}

/// 未达标不截、缺失声明冻结（AC：离线端声明缺失 / 冻结时截断延迟，不越过其
/// 水位）：从未应用 A 流的参与端（C，未声明）把水位钉在 0——A 不截；C 补齐
/// 应用并声明后，A 的后续轮次才截到声明水位。
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

    // C 有自己的流（参与端）但从未应用 A 流（无声明）→ 水位按 0 冻结，A 不截。
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        2,
        "参与端声明缺失 → 水位不可证，不截"
    );

    // C 应用 A 流（补齐）并经下一轮声明位点后，A 才按水位截断。
    run_round(&direct(&conn_c), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_c), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        0,
        "全部参与端声明齐备后按水位截断"
    );
}

/// 水位不超过任何对端的（含陈旧的）声明，也不超过 Checkpoint 覆盖（AC：未达标
/// 不截 + 覆盖校正上界）：B 的声明与覆盖停在 2 时，A 新增的 op3-4 不被截；证据
/// 推进（声明 4 + 新覆盖 4）后下一轮才截到 4。
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

    // A 新增 op3-4（证据仍停在 2）：A 的轮次只截到 2，op3-4 保留。
    protocol::create(&conn_a, make_expense("acc-1", 700, "三")).unwrap();
    protocol::create(&conn_a, make_expense("acc-1", 900, "四")).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        2,
        "水位 = min(陈旧声明 2, 覆盖 2)：只截位点之前，新 op 不越界被截"
    );

    // 证据推进到 4（B 声明 + 重新发布覆盖 4 的检查点）：下一轮截到 4。
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    upload_checkpoint(&mem, &layout, &mode, &create_checkpoint(&conn_b).unwrap()).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(stream_log_len(&conn_a, &dev_a), 0, "证据推进后按新水位截断");
}

/// 单端 / 未配同步世界不发生截断（AC）：世界内只有自己的流时无对端声明，即便
/// 自己发布过检查点（自证不算数——自己流位点恒为自己时钟头），也不截。
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

/// 覆盖证据缺席即不截（AC：无并入 Checkpoint 证据不删）：有对端声明但无检查点
/// 指针、或指针为旧形态（无位点字段，票 04 前产物）时，覆盖不可证，截断冻结。
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

    // 有声明、无指针：不截。
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

    // 旧形态指针（空位点 → manifest 无位点字段）：仍不截。
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

/// 挂起 op 不被截断（AC：位点钉住复验，通道轮次形态）：B 端对 A 流的位点被
/// 挂起 op 钉住 → B 的声明钉住 A 的截断水位；被挂起的 op 保留在 A 的日志与
/// 通道段中，B 重投递仍拿到它（幂等覆盖挂起行），不复活不丢失。
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
    // B 删 t1（自己的 op）；A 并发更新 t1 → B 重放 A 的更新命中软删行而挂起，
    // B 对 A 流的位点被钉在 1。
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

    // B 发布检查点（位点覆盖 A 流 = 1）：A 的水位 = min(B 声明 1, 覆盖 1) = 1。
    upload_checkpoint(&mem, &layout, &mode, &create_checkpoint(&conn_b).unwrap()).unwrap();
    run_round(&direct(&conn_a), &mem, &layout, &mode).unwrap();
    assert_eq!(
        stream_log_len(&conn_a, &dev_a),
        1,
        "只截位点之前的 op；被 B 挂起的更新 op 不被截"
    );

    // B 重投递：被挂起的 op 仍在（幂等覆盖挂起行，不复活不丢失）。
    run_round(&direct(&conn_b), &mem, &layout, &mode).unwrap();
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1, "挂起行幂等覆盖");
    assert_eq!(position_of_stream(&conn_b, &dev_a), 1, "位点不越过挂起 op");
}

/// 失败轮次不触发截断（AC：只在成功轮次落库段检查）：水位证据已齐备，但本轮
/// 在拉取段失败（段缺失）——中途返回不达落库段，自己的流原样保留。
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

    // B 再发布一段并从通道上取走段文件：A 的下一轮在拉取段失败（发布与声明
    // 已完成——若轮次成功，水位 3 会截掉 op1-3）。
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
