//! 通道层（issue #859 / ADR-0091 决策 1/8）：通道目录布局与 manifest、同步
//! 轮次（发布自己流 + 拉取他人流）、SyncEnvelope 加密形态、Checkpoint 通道
//! 传递与新端引导，以及本地 WebDAV 桩上的两端文件交换集成。
//!
//! 快速场景走内存假通道；HTTP 语义（建目录、凭据、两端真实文件交换）走
//! WebDAV 桩（AC：本地 WebDAV 桩上的两端文件交换集成测试）。

use rusqlite::Connection;

use crate::sync_engine::channel::{
    ChannelLayout, ChannelManifest, ChannelOptions, fetch_checkpoint, publish_checkpoint,
    publish_checkpoint_with, run_round, run_round_with,
};
use crate::sync_engine::envelope::{EnvelopeMode, EnvelopeParams, is_sealed};
use crate::sync_engine::tests::common::{MemoryTransport, make_expense, read_transaction};
use crate::sync_engine::transport::Transport;
use crate::sync_engine::transport::webdav::{WebDavConfig, WebDavTransport};
use crate::sync_engine::{OpOutcome, apply_ops, bootstrap_from_checkpoint, read_ops};
use crate::test_support::spawn_webdav_stub;
use crate::test_support::{self, seed_account};
use crate::transaction::write::protocol;

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
    let report = run_round(&conn, &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
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
    run_round(&conn_a, &mem, &layout, &EnvelopeMode::Plaintext).unwrap();

    let report = run_round(&conn_b, &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    assert_eq!(report.downloaded_segments, 1);
    assert_eq!(report.applied, 2);
    assert_eq!(
        read_transaction(&conn_b, &created.id),
        read_transaction(&conn_a, &created.id),
        "B 端业务字段与 A 端一致"
    );

    // 幂等：位点已覆盖全部段，重跑无下载、无重放。
    let report = run_round(&conn_b, &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    assert_eq!(report.downloaded_segments, 0);
    assert_eq!(report.applied, 0);

    // 增量：A 新增一笔 → 新段；B 只拉新段。
    protocol::create(&conn_a, make_expense("acc-1", 700, "打车")).unwrap();
    run_round(&conn_a, &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    let report = run_round(&conn_b, &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
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
    run_round(&conn_a, &mem, &layout(), &mode).unwrap();
    run_round(&conn_b, &mem, &layout(), &mode).unwrap(); // B 拉 A + 发布自己
    run_round(&conn_a, &mem, &layout(), &mode).unwrap(); // A 拉 B

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

use crate::test_support::assert_balance_cache_matches_realtime;

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
    run_round(&conn_a, &mem, &layout(), &mode).unwrap();
    run_round(&conn_b, &mem, &layout(), &mode).unwrap();
    run_round(&conn_a, &mem, &layout(), &mode).unwrap();

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
    let report =
        run_round_with(&conn_a, &mem, &layout, &EnvelopeMode::Plaintext, &options).unwrap();
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

    let report =
        run_round_with(&conn_b, &mem, &layout, &EnvelopeMode::Plaintext, &options).unwrap();
    assert_eq!(report.downloaded_segments, 3);
    assert_eq!(report.applied, 3);
}

/// 段文件被篡改：hash/尺寸自校验拦截，报可重试的码化错误（内容自校验兜底
/// WebDAV 弱原子性），不静默应用损坏内容。
#[test]
fn tampered_segment_is_detected_by_manifest_hash() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 100, "一")).unwrap();

    let mem = MemoryTransport::new();
    let layout = layout();
    run_round(&conn_a, &mem, &layout, &EnvelopeMode::Plaintext).unwrap();

    let device = device_id_of(&conn_a);
    mem.write_file(&layout.segment_path(&device, 1, 1), b"corrupted-bytes")
        .unwrap();

    let err = run_round(&conn_b, &mem, &layout, &EnvelopeMode::Plaintext).unwrap_err();
    assert!(err.is_code("sync-channel.segment-corrupt"), "实际: {err:?}");
    assert!(read_transaction(&conn_b, "none").is_none());
}

/// manifest 记录的段在通道上缺失（网盘清单先行/文件未到齐）：报明确错误，
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
    run_round(&conn_a, &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    let device = device_id_of(&conn_a);
    // 直接抹掉段文件（manifest 仍在）。
    mem.files_remove(&layout.segment_path(&device, 1, 1));

    let err = run_round(&conn_b, &mem, &layout, &EnvelopeMode::Plaintext).unwrap_err();
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
    run_round(&conn_a, &mem, &layout, &EnvelopeMode::Plaintext).unwrap();
    mem.write_file(&layout.manifest_path(), b"{broken json")
        .unwrap();

    let err = run_round(&conn_b, &mem, &layout, &EnvelopeMode::Plaintext).unwrap_err();
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
    let report = run_round_with(&conn_a, &mem, &layout, &mode, &fast_options()).unwrap();
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
    let err = run_round_with(&conn_b, &mem, &layout, &wrong, &fast_options()).unwrap_err();
    assert!(
        err.is_code("encryption.passphrase-incorrect"),
        "实际: {err:?}"
    );

    // 明文模式对端拉到密文：缺口令报可重试错误（混合世界显性失败）。
    let err = run_round(&conn_b, &mem, &layout, &EnvelopeMode::Plaintext).unwrap_err();
    assert!(
        err.is_code("sync-channel.passphrase-required"),
        "实际: {err:?}"
    );

    // 正确口令：解开并应用。
    let report = run_round_with(&conn_b, &mem, &layout, &mode, &fast_options()).unwrap();
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
    run_round(&conn_a, &mem, &layout, &mode).unwrap();
    let pointer = publish_checkpoint(&conn_a, &mem, &layout, &mode).unwrap();
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
    run_round(&conn_a, &mem, &layout, &mode).unwrap();
    let report = run_round(&conn_c, &mem, &layout, &mode).unwrap();
    assert_eq!(report.applied, 1, "只重放位点之后的增量");
    assert_eq!(
        read_transaction(&conn_c, &late.id),
        read_transaction(&conn_a, &late.id)
    );

    // 第二次发布：代数推进、指针换到新文件。
    let pointer2 = publish_checkpoint_with(&conn_a, &mem, &layout, &mode, &fast_options()).unwrap();
    assert_eq!(pointer2.generation, 2);
    assert_ne!(pointer2.file, pointer.file);
    let manifest: ChannelManifest =
        serde_json::from_slice(&mem.read_file(&layout.manifest_path()).unwrap().unwrap()).unwrap();
    assert_eq!(manifest.checkpoint.unwrap().file, pointer2.file);
}

/// AC 集成：本地 WebDAV 桩上的两端文件交换——真实 HTTP 语义（逐级建目录、
/// PUT/GET）+ 双向交换收敛 + 检查点发布与第三端引导。
#[test]
fn two_end_file_exchange_over_local_webdav_stub() {
    let stub = spawn_webdav_stub(Some(("alice", "app-pass")));
    let dav = WebDavTransport::new(WebDavConfig {
        base_url: stub.base_url.clone(),
        username: "alice".into(),
        password: "app-pass".into(),
    })
    .unwrap();

    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    let a_txn = protocol::create(&conn_a, make_expense("acc-1", 10000, "A 桌面记的账")).unwrap();
    let b_txn = protocol::create(&conn_b, make_expense("acc-1", 2500, "B 手机记的账")).unwrap();

    let layout = layout();
    let mode = EnvelopeMode::Plaintext;
    run_round(&conn_a, &dav, &layout, &mode).unwrap();
    run_round(&conn_b, &dav, &layout, &mode).unwrap();
    run_round(&conn_a, &dav, &layout, &mode).unwrap();

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

    // 检查点经 WebDAV 传递：第三端引导后与 A 一致。
    publish_checkpoint_with(&conn_a, &dav, &layout, &mode, &fast_options()).unwrap();
    let mut conn_c = test_support::open();
    let fetched = fetch_checkpoint(&dav, &layout, None).unwrap();
    bootstrap_from_checkpoint(&mut conn_c, &fetched.checkpoint, None).unwrap();
    assert_eq!(
        read_transaction(&conn_c, &a_txn.id),
        read_transaction(&conn_a, &a_txn.id)
    );
    assert_eq!(read_ops(&conn_c).unwrap().len(), 2, "引导端含快照携带日志");
}

/// 加密模式的 WebDAV 端到端：通道上只有密文，对端凭口令解开（AC：同步包以
/// 加密信封整体上云、凭主口令在另一端解开）。
#[test]
fn encrypted_exchange_over_local_webdav_stub() {
    let stub = spawn_webdav_stub(None);
    let dav = WebDavTransport::new(WebDavConfig {
        base_url: stub.base_url.clone(),
        username: "u".into(),
        password: "p".into(),
    })
    .unwrap();

    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    let created = protocol::create(&conn_a, make_expense("acc-1", 9900, "密文上云")).unwrap();

    let layout = layout();
    let mode = EnvelopeMode::Encrypted {
        passphrase: "两端共知的口令",
    };
    run_round_with(&conn_a, &dav, &layout, &mode, &fast_options()).unwrap();

    let manifest: ChannelManifest =
        serde_json::from_slice(&dav.read_file(&layout.manifest_path()).unwrap().unwrap()).unwrap();
    let segment = &manifest.streams[0].segments[0];
    let raw = dav
        .read_file(&layout.stream_file_path(&manifest.streams[0].device_id, &segment.file))
        .unwrap()
        .unwrap();
    assert!(is_sealed(&raw), "WebDAV 上必须是密文信封");

    let report = run_round_with(&conn_b, &dav, &layout, &mode, &fast_options()).unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(
        read_transaction(&conn_b, &created.id),
        read_transaction(&conn_a, &created.id)
    );

    // 密文检查点拉取回带封包标记（引导端对齐本库加密形态的依据，#864）。
    publish_checkpoint_with(&conn_a, &dav, &layout, &mode, &fast_options()).unwrap();
    let fetched = fetch_checkpoint(&dav, &layout, Some("两端共知的口令")).unwrap();
    assert!(fetched.sealed, "密文模式检查点应回带封包标记");
    assert!(!fetched.checkpoint.snapshot.is_empty());
}

/// 同步失败不影响本地记账（AC：凭据/网络失败明确可重试，本地旁路不受扰）。
#[test]
fn failed_round_leaves_local_ledger_untouched() {
    let stub = spawn_webdav_stub(Some(("alice", "right-pass")));
    let dav = WebDavTransport::new(WebDavConfig {
        base_url: stub.base_url.clone(),
        username: "alice".into(),
        password: "wrong-pass".into(),
    })
    .unwrap();

    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    let before = protocol::create(&conn, make_expense("acc-1", 100, "失败前")).unwrap();

    let err = run_round(&conn, &dav, &layout(), &EnvelopeMode::Plaintext).unwrap_err();
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
