use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::super::{
    OpOutcome, apply_ops, bootstrap_from_checkpoint, create_checkpoint, ops_after_positions,
    parked_ops, read_ops, stream_positions, truncate_stream_before,
};
use super::common::{make_expense, read_transaction, wire_in, wire_out};
use crate::channel::{ChannelLayout, upload_checkpoint};
use crate::envelope::EnvelopeMode;
use crate::ops;
use crate::transport::Transport;
use ledger_sync_protocol::position as positions;
use ledger_transaction::write::protocol;
use tauri_app_lib::test_support::{
    self, ScratchDir, ScratchFile, assert_balance_cache_matches_realtime, seed_account,
};

fn device_of(conn: &rusqlite::Connection) -> String {
    conn.query_row("SELECT id FROM sync_device LIMIT 1", [], |r| r.get(0))
        .unwrap()
}

fn position_of_stream(conn: &rusqlite::Connection, device_id: &str) -> i64 {
    stream_positions(conn)
        .unwrap()
        .into_iter()
        .find(|p| p.device_id == device_id)
        .expect("位点行在列")
        .applied_through
}

fn txn_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap()
}

fn base_ledger(conn_a: &rusqlite::Connection) -> String {
    seed_account(conn_a, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(conn_a, make_expense("acc-1", 10000, "午饭"))
        .unwrap()
        .id
}

fn pinned_world() -> (rusqlite::Connection, rusqlite::Connection, String) {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    let id = base_ledger(&conn_a);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    wire_in(&conn_b, &wire_out(&conn_a));
    protocol::delete(&conn_b, &id).unwrap();
    protocol::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();
    let t2 = protocol::create(&conn_a, make_expense("acc-1", 2500, "咖啡"))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));
    (conn_a, conn_b, t2)
}

#[test]
fn checkpoint_anytime_captures_positions_and_snapshot() {
    let conn_a = test_support::open();
    base_ledger(&conn_a);
    let dev_a = device_of(&conn_a);

    let cp = create_checkpoint(&conn_a).unwrap();
    assert!(!cp.snapshot.is_empty(), "快照为整库字节，非空");
    assert_eq!(cp.positions.len(), 1, "位点覆盖已应用的来源流");
    assert_eq!(cp.positions[0].device_id, dev_a);
    assert_eq!(cp.positions[0].applied_through, 1, "A 流已应用到时钟 1");
}

#[test]
fn checkpoint_on_quiescent_ledger_has_empty_positions() {
    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);

    let cp = create_checkpoint(&conn).unwrap();
    assert!(!cp.snapshot.is_empty());
    assert!(cp.positions.is_empty(), "无 op 流则无位点");
}

#[test]
fn stream_positions_reader_matches_checkpoint_positions() {
    let conn_a = test_support::open();
    base_ledger(&conn_a);

    let cp = create_checkpoint(&conn_a).unwrap();
    assert_eq!(stream_positions(&conn_a).unwrap(), cp.positions);
}

#[test]
fn bootstrap_plus_ops_after_positions_reaches_source_state() {
    let conn_a = test_support::open();
    let mut conn_b = test_support::open();
    let id = base_ledger(&conn_a);

    let cp = create_checkpoint(&conn_a).unwrap();

    protocol::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();
    let t2 = protocol::create(&conn_a, make_expense("acc-1", 2500, "咖啡"))
        .unwrap()
        .id;

    bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap();
    assert_eq!(
        read_transaction(&conn_b, &id).unwrap().note.as_deref(),
        Some("午饭"),
        "引导后为快照时刻状态"
    );

    let after = ops_after_positions(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();
    assert_eq!(after.len(), 2, "位点之前的 op 无需重放");
    let reports = apply_ops(&conn_b, &after).unwrap();
    assert!(reports.iter().all(|r| r.outcome == OpOutcome::Applied));

    assert_eq!(
        read_transaction(&conn_b, &id).unwrap(),
        read_transaction(&conn_a, &id).unwrap()
    );
    assert_eq!(
        read_transaction(&conn_b, &t2).unwrap(),
        read_transaction(&conn_a, &t2).unwrap()
    );
    assert_eq!(read_ops(&conn_b).unwrap(), read_ops(&conn_a).unwrap());
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn deterministic_reconstruction_from_same_checkpoint() {
    let conn_a = test_support::open();
    let id = base_ledger(&conn_a);
    let cp = create_checkpoint(&conn_a).unwrap();
    protocol::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();

    let after = read_ops(&conn_a).unwrap();
    let rebuild = || {
        let mut fresh = test_support::open();
        bootstrap_from_checkpoint(&mut fresh, &cp, None).unwrap();
        apply_ops(&fresh, &ops_after_positions(&fresh, &after).unwrap()).unwrap();
        fresh
    };
    let b1 = rebuild();
    let b2 = rebuild();

    assert_eq!(
        read_transaction(&b1, &id),
        read_transaction(&b2, &id),
        "同一 Checkpoint + 其后 op 重建出同一状态"
    );
    assert_eq!(read_ops(&b1).unwrap(), read_ops(&b2).unwrap());
}

#[test]
fn bootstrap_swaps_device_identity_and_adopts_positions() {
    let conn_a = test_support::open();
    let mut conn_b = test_support::open();
    base_ledger(&conn_a);
    let dev_a = device_of(&conn_a);

    let cp = create_checkpoint(&conn_a).unwrap();
    bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap();

    let dev_b = device_of(&conn_b);
    assert_ne!(dev_b, dev_a, "引导后本机持有自己的设备标识");

    let t2 = protocol::create(&conn_b, make_expense("acc-1", 2500, "咖啡"))
        .unwrap()
        .id;
    let ops_b = read_ops(&conn_b).unwrap();
    assert_eq!(ops_b.len(), 2, "快照采纳的 A 流 op + 本机新 op");
    assert_eq!(ops_b[1].device_id, dev_b);
    assert_eq!(ops_b[1].clock, 1, "本机新流时钟从 1 起");
    let adopted = stream_positions(&conn_b)
        .unwrap()
        .into_iter()
        .find(|p| p.device_id == dev_a)
        .expect("快照来源流位点被采纳为外来流");
    assert_eq!(adopted.applied_through, 1);
    assert_eq!(read_transaction(&conn_b, &t2).unwrap().amount_cents, 2500);
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn bootstrap_rejects_non_fresh_target() {
    let conn_a = test_support::open();
    let mut conn_b = test_support::open();
    base_ledger(&conn_a);
    let cp = create_checkpoint(&conn_a).unwrap();

    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_b, make_expense("acc-1", 100, "已有账")).unwrap();

    let err = bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap_err();
    assert_eq!(err.code(), Some("sync-engine.bootstrap-not-fresh"));
}

#[test]
fn bootstrap_rejects_snapshot_from_newer_schema() {
    let conn_newer = test_support::open();
    conn_newer
        .execute("PRAGMA user_version = 999999", [])
        .unwrap();
    let cp = create_checkpoint(&conn_newer).unwrap();

    let mut conn_b = test_support::open();
    let err = bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap_err();
    assert_eq!(err.code(), Some("sync-engine.checkpoint-schema-newer"));
}

#[test]
fn redelivery_of_ops_at_or_below_position_skips() {
    let conn_a = test_support::open();
    let mut conn_b = test_support::open();
    let id = base_ledger(&conn_a);
    let cp = create_checkpoint(&conn_a).unwrap();
    protocol::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();

    bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap();

    let below: Vec<_> = read_ops(&conn_a)
        .unwrap()
        .into_iter()
        .filter(|op| op.clock <= 1)
        .collect();
    assert_eq!(below.len(), 1);
    let reports = apply_ops(&conn_b, &below).unwrap();
    assert!(
        reports.iter().all(|r| r.outcome == OpOutcome::Skipped),
        "位点已覆盖的 op 一律跳过"
    );
    assert_eq!(txn_count(&conn_b), 1, "不产生第二笔");
    assert_eq!(
        read_transaction(&conn_b, &id).unwrap().note.as_deref(),
        Some("午饭"),
        "快照时刻状态不被重投改写"
    );
}

#[test]
fn positions_pinned_below_parked_op() {
    let (conn_a, conn_b, t2) = pinned_world();

    let dev_a = device_of(&conn_a);
    assert_eq!(position_of_stream(&conn_b, &dev_a), 1);
    assert_eq!(
        read_transaction(&conn_b, &t2).unwrap().amount_cents,
        2500,
        "op3 正常应用"
    );
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1, "op2 挂起待裁决");
}

#[test]
fn truncation_is_owner_only_position_bounded_and_keeps_unapplied_ops() {
    let (conn_a, conn_b, _t2) = pinned_world();
    let dev_a = device_of(&conn_a);

    let err = truncate_stream_before(&conn_b, &dev_a, 99).unwrap_err();
    assert_eq!(err.code(), Some("sync-engine.truncate-not-owner"));

    let watermark = position_of_stream(&conn_b, &dev_a);
    let deleted = truncate_stream_before(&conn_a, &dev_a, watermark).unwrap();
    assert_eq!(deleted, 1, "只删位点之前（时钟 ≤ 1）的 op");

    assert_eq!(
        read_ops(&conn_a).unwrap().len(),
        2,
        "挂起 op 与其后 op 不受截断影响"
    );
    assert_balance_cache_matches_realtime(&conn_a);

    assert_eq!(position_of_stream(&conn_b, &dev_a), watermark);
}

#[test]
fn sync_continues_after_truncation_without_resurrecting_truncated_ops() {
    let (conn_a, conn_b, _t2) = pinned_world();
    let dev_a = device_of(&conn_a);

    let watermark = position_of_stream(&conn_b, &dev_a);
    truncate_stream_before(&conn_a, &dev_a, watermark).unwrap();

    let reports = wire_in(&conn_a, &wire_out(&conn_b));
    assert!(
        reports
            .iter()
            .all(|r| matches!(r.outcome, OpOutcome::Skipped | OpOutcome::Superseded)),
        "位点已覆盖的 op 跳过、LWW 输者压制，无重放"
    );
    assert_eq!(txn_count(&conn_a), 2, "不复活、不重复");

    let t3 = protocol::create(&conn_a, make_expense("acc-1", 9900, "打车"))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(
        read_transaction(&conn_b, &t3).unwrap().amount_cents,
        9900,
        "截断后同步继续"
    );
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1, "挂起裁决不受影响");
}

#[test]
fn encrypted_checkpoint_roundtrip_and_passphrase_guards() {
    let dir = ScratchDir::new("cp-test");
    let db_path = dir.join("ledger.db");

    {
        let seed = test_support::open();
        seed_account(&seed, "acc-1", "现金", "cash", "CNY", 0);
        protocol::create(&seed, make_expense("acc-1", 10000, "午饭")).unwrap();
        seed.execute(
            "VACUUM INTO ?1",
            rusqlite::params![db_path.to_string_lossy()],
        )
        .unwrap();
        seed.execute(
            "VACUUM sync INTO ?1",
            rusqlite::params![dir.join("sync.db").to_string_lossy()],
        )
        .unwrap();
    }
    ledger_infra::db::encryption::enable_encryption_for_file(&db_path, "correct horse").unwrap();
    let conn_a =
        ledger_infra::db::open_connection_with_passphrase(&db_path, "correct horse").unwrap();
    let id = read_ops(&conn_a)
        .unwrap()
        .iter()
        .find_map(|op| op.command.subject().1.map(|eid| eid.into_owned()))
        .expect("种子交易 op 在场");

    let cp = create_checkpoint(&conn_a).unwrap();
    drop(conn_a);
    assert_eq!(
        ledger_infra::db::encryption::probe_file_kind(&{
            let p = dir.join("snap-probe.db");
            std::fs::write(&p, &cp.snapshot).unwrap();
            p
        })
        .unwrap(),
        ledger_infra::db::encryption::DbFileKind::Encrypted,
        "快照继承源库加密形态"
    );

    let mut conn_b = test_support::open();
    let err = bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap_err();
    assert_eq!(
        err.code(),
        Some("sync-engine.checkpoint-passphrase-required")
    );
    let err = bootstrap_from_checkpoint(&mut conn_b, &cp, Some("wrong")).unwrap_err();
    assert_eq!(err.code(), Some("encryption.passphrase-incorrect"));
    bootstrap_from_checkpoint(&mut conn_b, &cp, Some("correct horse")).unwrap();
    assert_eq!(read_transaction(&conn_b, &id).unwrap().amount_cents, 10000);
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn first_sighting_with_pending_park_pins_position_below_it() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    base_ledger(&conn_a);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 2500, "咖啡")).unwrap();
    let mut wire = wire_out(&conn_a);
    assert_eq!(wire.len(), 2);
    wire[0] = "{not-json".to_string();
    wire_in(&conn_b, &wire);

    let dev_a = device_of(&conn_a);
    let pos = stream_positions(&conn_b)
        .unwrap()
        .into_iter()
        .find(|p| p.device_id == dev_a)
        .expect("首见裁决建行");
    assert_eq!(
        pos.applied_through, 0,
        "首见建行被挂起缺口挡在 0，不越过未应用 op"
    );
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1, "op1 挂起待裁决");
}

type SharedFiles = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

type FetchHook = Box<dyn Fn(&str) + Send + Sync>;

struct FetchObservedTransport {
    files: SharedFiles,
    on_read: Option<FetchHook>,
}

impl FetchObservedTransport {
    fn publisher(files: &SharedFiles) -> Self {
        Self {
            files: files.clone(),
            on_read: None,
        }
    }

    fn observer(files: &SharedFiles, on_read: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Self {
            files: files.clone(),
            on_read: Some(Box::new(on_read)),
        }
    }
}

impl Transport for FetchObservedTransport {
    fn ensure_dir(&self, _path: &str) -> ledger_infra::error::Result<()> {
        Ok(())
    }

    fn read_file(&self, path: &str) -> ledger_infra::error::Result<Option<Vec<u8>>> {
        if let Some(on_read) = &self.on_read {
            on_read(path);
        }
        Ok(self.files.lock().unwrap().get(path).cloned())
    }

    fn write_file(&self, path: &str, bytes: &[u8]) -> ledger_infra::error::Result<()> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), bytes.to_vec());
        Ok(())
    }
}

struct SegmentsStub {
    conn: Arc<Mutex<rusqlite::Connection>>,
    events: Arc<Mutex<Vec<&'static str>>>,
    conn_live: Arc<AtomicBool>,
}

impl SegmentsStub {
    fn new(conn: rusqlite::Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
            events: Arc::new(Mutex::new(Vec::new())),
            conn_live: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl crate::BootstrapConnSegments for SegmentsStub {
    fn with_conn<T>(
        &self,
        use_conn: impl FnOnce(&mut rusqlite::Connection) -> ledger_infra::error::Result<T>,
    ) -> ledger_infra::error::Result<T> {
        self.events.lock().unwrap().push("seg-start");
        let mut guard = self.conn.lock().unwrap();
        self.conn_live.store(true, Ordering::SeqCst);
        let result = use_conn(&mut guard);
        self.conn_live.store(false, Ordering::SeqCst);
        drop(guard);
        self.events.lock().unwrap().push("seg-end");
        result
    }
}

fn bootstrap_layout() -> ChannelLayout {
    ChannelLayout::new("0197abcd-0000-7000-8000-000000000085").unwrap()
}

fn publish_source_checkpoint(files: &SharedFiles) -> String {
    let conn_a = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    let id = protocol::create(&conn_a, make_expense("acc-1", 10_000, "午饭"))
        .unwrap()
        .id;
    let frozen = create_checkpoint(&conn_a).unwrap();
    let publisher = FetchObservedTransport::publisher(files);
    upload_checkpoint(
        &publisher,
        &bootstrap_layout(),
        &EnvelopeMode::Plaintext,
        &frozen,
    )
    .unwrap();
    id
}

fn observed_channel(
    files: &SharedFiles,
    events: Arc<Mutex<Vec<&'static str>>>,
    conn_live: Arc<AtomicBool>,
) -> crate::SyncChannel {
    observed_channel_with_snapshot_hook(files, events, conn_live, || ())
}

fn observed_channel_with_snapshot_hook(
    files: &SharedFiles,
    events: Arc<Mutex<Vec<&'static str>>>,
    conn_live: Arc<AtomicBool>,
    on_snapshot: impl Fn() + Send + Sync + 'static,
) -> crate::SyncChannel {
    let on_snapshot = Mutex::new(on_snapshot);
    let hook = move |path: &str| {
        let (event, is_snapshot) = if path.ends_with("/manifest.json") {
            ("fetch-manifest", false)
        } else {
            ("fetch-snapshot", !path.ends_with("-sync.enc"))
        };
        if is_snapshot {
            (on_snapshot.lock().unwrap())();
        }
        events.lock().unwrap().push(event);
        assert!(
            !conn_live.load(Ordering::SeqCst),
            "{event} 必须发生在连接段之外（下载段不持连接，ADR-0069 决策 4）"
        );
    };
    crate::SyncChannel::from_parts(
        Box::new(FetchObservedTransport::observer(files, hook)),
        bootstrap_layout(),
    )
}

fn probe_only_db_path() -> ScratchFile {
    ScratchFile::new(
        "bootstrap-domain",
        format!(
            "ledger-bootstrap-domain-{}.db",
            ledger_infra::db::new_uuid()
        ),
    )
}

fn assert_fetch_between_segments(events: &[&'static str]) {
    assert_eq!(
        events,
        [
            "seg-start",
            "seg-end",
            "fetch-manifest",
            "fetch-snapshot",
            "fetch-snapshot",
            "seg-start",
            "seg-end"
        ],
        "下载段必须整体位于两段之间（段1 前置守卫 → 锁外下载 → 段2 复验换入）"
    );
}

#[test]
fn bootstrap_fetch_runs_between_conn_segments_without_live_connection() {
    let files: SharedFiles = Arc::new(Mutex::new(BTreeMap::new()));
    let source_txn = publish_source_checkpoint(&files);

    let harness = SegmentsStub::new(test_support::open());
    let channel = observed_channel(&files, harness.events.clone(), harness.conn_live.clone());

    let outcome =
        crate::bootstrap_from_channel(&harness, &probe_only_db_path(), &channel, None).unwrap();
    assert_eq!(outcome.generation, 1, "采纳通道当前检查点代数");
    assert!(outcome.size > 0);
    assert!(!outcome.reencrypted, "明文快照 × 明文本机无转换");

    let conn = harness.conn.lock().unwrap();
    assert_eq!(
        read_transaction(&conn, &source_txn).unwrap().amount_cents,
        10_000,
        "快照携带的交易随引导就位"
    );
}

#[test]
fn bootstrap_local_write_during_download_is_caught_by_recheck_not_overwritten() {
    let files: SharedFiles = Arc::new(Mutex::new(BTreeMap::new()));
    let source_txn = publish_source_checkpoint(&files);

    let harness = SegmentsStub::new(test_support::open());
    let racer_conn = harness.conn.clone();
    let channel = observed_channel_with_snapshot_hook(
        &files,
        harness.events.clone(),
        harness.conn_live.clone(),
        move || {
            let conn = racer_conn.lock().unwrap();
            seed_account(&conn, "acc-race", "下载期间落的账", "cash", "CNY", 0);
        },
    );

    let err =
        crate::bootstrap_from_channel(&harness, &probe_only_db_path(), &channel, None).unwrap_err();
    assert_eq!(
        err.code(),
        Some("sync-channel.bootstrap-library-not-empty"),
        "段2 复验把「判空后被写」收敛为显式报错"
    );
    assert_fetch_between_segments(&harness.events.lock().unwrap());

    let conn = harness.conn.lock().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM accounts WHERE id = 'acc-race'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1,
        "下载期间落进的本机写不被快照覆盖"
    );
    assert!(
        read_transaction(&conn, &source_txn).is_none(),
        "复验拒绝先于整库换入，快照内容不得就位"
    );
}

#[test]
fn bootstrap_preflight_failure_skips_download_entirely() {
    let files: SharedFiles = Arc::new(Mutex::new(BTreeMap::new()));
    let _source_txn = publish_source_checkpoint(&files);

    let conn_b = test_support::open();
    seed_account(&conn_b, "acc-local", "本地已有数据", "cash", "CNY", 0);
    let harness = SegmentsStub::new(conn_b);
    let channel = observed_channel(&files, harness.events.clone(), harness.conn_live.clone());

    let err =
        crate::bootstrap_from_channel(&harness, &probe_only_db_path(), &channel, None).unwrap_err();
    assert_eq!(err.code(), Some("sync-channel.bootstrap-library-not-empty"));
    assert_eq!(
        *harness.events.lock().unwrap(),
        ["seg-start", "seg-end"],
        "前置守卫在段1 拒绝，下载段不发生（通道读零次）"
    );
}

#[test]
fn advance_rolls_forward_across_backfilled_range() {
    let (conn_a, conn_b, _t2) = pinned_world();
    let dev_a = device_of(&conn_a);
    assert_eq!(position_of_stream(&conn_b, &dev_a), 1);
    let op2 = read_ops(&conn_a)
        .unwrap()
        .into_iter()
        .find(|op| op.clock == 2)
        .expect("A 流时钟 2 的 op 在源日志");
    ops::insert_row(&conn_b, &op2).unwrap();
    positions::advance(&conn_b, &dev_a, 3).unwrap();
    let pos = stream_positions(&conn_b)
        .unwrap()
        .into_iter()
        .find(|p| p.device_id == dev_a)
        .unwrap();
    assert_eq!(
        pos.applied_through, 3,
        "前滚跨过已补齐区段，水位落到连续已裁决末端"
    );
}

#[test]
fn bootstrap_migrates_older_schema_snapshot() {
    let conn_a = test_support::open();
    let id = base_ledger(&conn_a);
    let cp22 = create_checkpoint(&conn_a).unwrap();

    let stale_path = ScratchFile::new(
        "v21",
        format!("ledger-v21-{}.db", ledger_infra::db::new_uuid()),
    );
    std::fs::write(&stale_path, &cp22.snapshot).unwrap();
    {
        let stale = ledger_infra::db::open_connection(&stale_path).unwrap();
        stale
            .execute_batch(
                "CREATE TABLE sync_device (\n                 id TEXT PRIMARY KEY,\n                 logical_clock INTEGER NOT NULL DEFAULT 0,\n                 created_at TEXT NOT NULL,\n                 updated_at TEXT NOT NULL\n             );\n             CREATE TABLE sync_ops (\n                 op_id TEXT PRIMARY KEY,\n                 device_id TEXT NOT NULL,\n                 clock INTEGER NOT NULL,\n                 schema_version INTEGER NOT NULL,\n                 entity TEXT NOT NULL,\n                 entity_id TEXT NOT NULL DEFAULT '',\n                 payload TEXT NOT NULL,\n                 recorded_at TEXT NOT NULL\n             );\n             CREATE UNIQUE INDEX idx_sync_ops_device_clock ON sync_ops (device_id, clock);\n             CREATE INDEX idx_sync_ops_entity_id ON sync_ops (entity, entity_id);\n             CREATE TABLE sync_parked_ops (\n                 op_id TEXT PRIMARY KEY,\n                 device_id TEXT NOT NULL,\n                 clock INTEGER NOT NULL,\n                 schema_version INTEGER NOT NULL,\n                 entity TEXT NOT NULL,\n                 entity_id TEXT NOT NULL DEFAULT '',\n                 payload TEXT NOT NULL,\n                 park_code TEXT NOT NULL,\n                 park_params TEXT NOT NULL DEFAULT '[]',\n                 park_message TEXT NOT NULL,\n                 parked_at TEXT NOT NULL\n             );",
            )
            .unwrap();
        stale
            .execute("DROP INDEX IF EXISTS idx_transactions_funding", [])
            .unwrap();
        stale
            .execute("DROP INDEX IF EXISTS idx_transactions_kind_count", [])
            .unwrap();
        stale
            .execute(
                "ALTER TABLE transactions DROP COLUMN funding_account_id",
                [],
            )
            .unwrap();
        for column in ["credit_limit_cents", "statement_day", "due_day"] {
            stale
                .execute(&format!("ALTER TABLE accounts DROP COLUMN {column}"), [])
                .unwrap();
        }
        stale
            .execute("ALTER TABLE accounts DROP COLUMN is_archived", [])
            .unwrap();
        stale
            .execute("ALTER TABLE security_transactions DROP COLUMN origin", [])
            .unwrap();
        stale
            .execute(
                "ALTER TABLE instruments DROP COLUMN constant_unit_price",
                [],
            )
            .unwrap();
        for column in ["fx_rate_used", "fx_rate_source"] {
            stale
                .execute(
                    &format!("ALTER TABLE transactions DROP COLUMN {column}"),
                    [],
                )
                .unwrap();
        }
        stale
            .execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_transactions_account ON transactions(account_id);\n                 CREATE INDEX IF NOT EXISTS idx_transactions_category ON transactions(category_id);\n                 CREATE INDEX IF NOT EXISTS idx_transactions_refund ON transactions(refund_of_transaction_id);\n                 CREATE INDEX IF NOT EXISTS idx_transactions_sync ON transactions(updated_at, device_id);\n                 CREATE INDEX IF NOT EXISTS idx_transactions_deleted ON transactions(is_deleted, updated_at);\n                 CREATE INDEX IF NOT EXISTS idx_transactions_amount ON transactions(amount_cents);",
            )
            .unwrap();
        stale
            .execute("ALTER TABLE transactions ADD COLUMN note_pinyin TEXT", [])
            .unwrap();
        stale
            .execute("DROP INDEX IF EXISTS idx_transactions_note_search", [])
            .unwrap();
        stale
            .execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_transactions_note_pinyin_backlog ON transactions(id) WHERE note_pinyin IS NULL AND note IS NOT NULL;\n                 CREATE INDEX IF NOT EXISTS idx_transactions_note_search ON transactions(date, created_at, id, note, note_pinyin, account_id, merchant_id, category_id) WHERE is_deleted = 0;",
            )
            .unwrap();
        stale.execute("PRAGMA user_version = 20", []).unwrap();
    }
    let cp = super::super::Checkpoint {
        positions: cp22.positions,
        snapshot: std::fs::read(&stale_path).unwrap(),
        sync_snapshot: Vec::new(),
    };

    let mut conn_b = test_support::open();
    bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap();

    let fresh = test_support::open();
    let expected: i64 = fresh
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    let actual: i64 = conn_b
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(actual, expected, "引导后迁移升级到本端最新 schema");

    let schema_of = |conn: &rusqlite::Connection| -> Vec<(String, String, String, String)> {
        let mut stmt = conn
            .prepare(
                "SELECT type, name, tbl_name, sql FROM sqlite_master \
                 WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
            )
            .unwrap();
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?
                    .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
                    .unwrap_or_default(),
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    assert_eq!(
        schema_of(&conn_b),
        schema_of(&fresh),
        "旧快照前向迁移终态应与全新 to_latest 库 schema 全等"
    );

    let dev_a = device_of(&conn_a);
    let pos = stream_positions(&conn_b)
        .unwrap()
        .into_iter()
        .find(|p| p.device_id == dev_a)
        .expect("位点表已随迁移重建，位点写入生效");
    assert_eq!(pos.applied_through, 1);
    assert_eq!(
        read_transaction(&conn_b, &id).unwrap().note.as_deref(),
        Some("午饭"),
        "业务数据完整"
    );
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn fresh_library_has_no_user_data() {
    let conn = test_support::open();
    assert!(
        !crate::checkpoint::library_has_user_data(&conn).unwrap(),
        "全新库（含种子分类/币种/黑洞账户）不应判为已有业务数据"
    );
}

#[test]
fn user_fact_rows_in_any_business_domain_trigger_probe() {
    let conn = test_support::open();
    base_ledger(&conn);
    assert!(
        crate::checkpoint::library_has_user_data(&conn).unwrap(),
        "有交易的库应判为已有业务数据"
    );

    let conn = test_support::open();
    seed_account(&conn, "acc-user", "钱包", "cash", "CNY", 0);
    assert_ne!(
        conn.query_row(
            "SELECT device_id FROM accounts WHERE id = 'acc-user'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "seed",
        "测试前置：种子账户判定应成立"
    );
    assert!(
        crate::checkpoint::library_has_user_data(&conn).unwrap(),
        "只有用户自建账户（无交易）也应判为已有业务数据"
    );
}

#[test]
fn checkpoint_produces_paired_sync_component() {
    let conn = test_support::open();
    base_ledger(&conn);
    let dev = device_of(&conn);

    let cp = create_checkpoint(&conn).unwrap();
    assert!(!cp.snapshot.is_empty());
    assert!(!cp.sync_snapshot.is_empty(), "同步元数据件成对产出，非空");

    let dir = ScratchDir::new("cp-sync-probe");
    let sync_file = dir.join("sync-component.db");
    std::fs::write(&sync_file, &cp.sync_snapshot).unwrap();
    let probe = ledger_infra::db::open_connection_unmounted(&sync_file).unwrap();
    for (table, source_count) in [
        ("sync_device", 1),
        ("sync_ops", read_ops(&conn).unwrap().len() as i64),
        ("sync_parked_ops", 0),
        (
            "sync_stream_positions",
            stream_positions(&conn).unwrap().len() as i64,
        ),
    ] {
        let count: i64 = probe
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, source_count, "同步件 {table} 行数与源对等");
    }
    let probe_dev: String = probe
        .query_row("SELECT id FROM sync_device", [], |r| r.get(0))
        .unwrap();
    assert_eq!(probe_dev, dev, "同步件携带源设备身份");
}

#[test]
fn dual_db_source_produces_business_and_sync_files_split() {
    let src_dir = ScratchDir::new("cp-dual-src");
    let src_main = src_dir.join("ledger.db");
    let src_sync = src_dir.join("sync.db");
    make_business_only_main(&src_main);
    make_sync_only_db(&src_sync);
    let conn_a = ledger_infra::db::open_connection(&src_main).unwrap();
    let mounted: i64 = conn_a
        .query_row(
            "SELECT count(*) FROM pragma_database_list WHERE name = 'sync'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mounted, 1, "前置：源端 attached 侧挂载在位");
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 10_000, "午饭")).unwrap();

    let cp = create_checkpoint(&conn_a).unwrap();

    assert_business_file_has_no_sync_tables(&cp.snapshot);
    let sync_ops_count = assert_sync_file_carries_sync_tables(&cp.sync_snapshot);
    assert_eq!(sync_ops_count, 1, "同步件承载源端 op");

    assert_eq!(stream_positions(&conn_a).unwrap(), cp.positions);
}

#[test]
fn legacy_single_file_snapshot_lands_tables_in_two_dbs_and_converges() {
    use ledger_infra::db::{SYNC_SPLIT_USER_VERSION, migrations, open_connection_unmounted};

    let _wire = test_support::open();

    let dir = ScratchDir::new("cp-legacy-world");
    let legacy_path = dir.path().join("legacy.db");
    let mut legacy = open_connection_unmounted(&legacy_path).unwrap();
    migrations()
        .to_version(&mut legacy, (SYNC_SPLIT_USER_VERSION - 1) as usize)
        .unwrap();

    seed_account(&legacy, "acc-1", "现金", "cash", "CNY", 0);
    let t1 = protocol::create(&legacy, make_expense("acc-1", 10_000, "午饭"))
        .unwrap()
        .id;
    let dev_src = device_of(&legacy);
    legacy
        .execute(
            &format!(
                "INSERT INTO sync_stream_positions (device_id, applied_through, updated_at) \
                 VALUES ('dev-foreign', 3, '{}')",
                test_support::FIXED_NOW
            ),
            [],
        )
        .unwrap();
    legacy
        .execute(
            &format!(
                "INSERT INTO sync_parked_ops (op_id, device_id, clock, schema_version, entity, entity_id, payload, park_code, park_params, park_message, parked_at) \
                 VALUES ('pop-1', 'dev-foreign', 2, 34, 'transaction', 'txn-parked', '{{}}', 'sync-engine.op-foreign-key', '[]', '挂起', '{}')",
                test_support::FIXED_NOW
            ),
            [],
        )
        .unwrap();

    let snap_path = dir.path().join("legacy-snapshot.db");
    legacy
        .execute(
            "VACUUM INTO ?1",
            rusqlite::params![snap_path.to_string_lossy()],
        )
        .unwrap();
    let positions_at_snapshot = stream_positions(&legacy).unwrap();
    assert_eq!(
        positions_at_snapshot.len(),
        2,
        "前置：本机流与外来流位点都在列"
    );

    protocol::update(&legacy, &t1, make_expense("acc-1", 10_000, "A 改")).unwrap();
    let t2 = protocol::create(&legacy, make_expense("acc-1", 2_500, "咖啡"))
        .unwrap()
        .id;
    let source_ops = read_ops(&legacy).unwrap();
    let source_positions = stream_positions(&legacy).unwrap();
    let src_t1 = read_transaction(&legacy, &t1).unwrap();
    let src_t2 = read_transaction(&legacy, &t2).unwrap();

    let cp = super::super::Checkpoint {
        positions: positions_at_snapshot.clone(),
        snapshot: std::fs::read(&snap_path).unwrap(),
        sync_snapshot: Vec::new(),
    };
    drop(legacy);

    let (mut conn_b, _bdir) = test_support::open_file_scratch("cp-legacy-target");
    bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap();

    for table in SPLIT_WORLD_SYNC_TABLES {
        let in_main: i64 = conn_b
            .query_row(
                "SELECT COUNT(*) FROM main.sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(in_main, 0, "主库不应残留同步表 {table}");
    }
    let attached_ops: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM sync.sync_ops", [], |r| r.get(0))
        .unwrap();
    assert_eq!(attached_ops, 1, "快照携带的 op 归位 attached");
    let attached_parked: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM sync.sync_parked_ops", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(attached_parked, 1, "快照携带的挂起行归位 attached");
    assert_eq!(
        stream_positions(&conn_b).unwrap(),
        positions_at_snapshot,
        "位点随 Checkpoint 就位"
    );
    let dev_b = device_of(&conn_b);
    assert_ne!(dev_b, dev_src, "本机持有自己的设备标识");
    let dev_rows: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM sync.sync_device", [], |r| r.get(0))
        .unwrap();
    assert_eq!(dev_rows, 1, "只留本机身份一行");
    assert_eq!(
        read_transaction(&conn_b, &t1).unwrap().note.as_deref(),
        Some("午饭"),
        "引导后为快照时刻状态"
    );
    let fresh = test_support::open();
    let latest: i64 = fresh
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    let ver: i64 = conn_b
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ver, latest, "引导后版本收敛到本端最新");

    let after = ops_after_positions(&conn_b, &source_ops).unwrap();
    assert_eq!(after.len(), 2, "位点之前的 op 无需重放");
    apply_ops(&conn_b, &after).unwrap();
    assert_eq!(read_transaction(&conn_b, &t1).unwrap(), src_t1);
    assert_eq!(read_transaction(&conn_b, &t2).unwrap(), src_t2);
    assert_eq!(read_ops(&conn_b).unwrap(), source_ops);
    assert_eq!(stream_positions(&conn_b).unwrap(), source_positions);
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn newer_form_checkpoint_on_pre_split_world_target_is_coded_schema_newer() {
    use ledger_infra::db::{SYNC_SPLIT_USER_VERSION, migrations, open_connection_unmounted};

    let _wire = test_support::open();

    let conn_a = test_support::open();
    let id = base_ledger(&conn_a);
    let cp = create_checkpoint(&conn_a).unwrap();
    assert!(!cp.sync_snapshot.is_empty(), "前置：新形态快照带同步件");

    let dir = ScratchDir::new("cp-pre-split-target");
    let target_path = dir.path().join("pre-split-target.db");
    let mut target = open_connection_unmounted(&target_path).unwrap();
    migrations()
        .to_version(&mut target, (SYNC_SPLIT_USER_VERSION - 1) as usize)
        .unwrap();
    let version_before: i64 = target
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();

    let err = bootstrap_from_checkpoint(&mut target, &cp, None).unwrap_err();
    assert_eq!(
        err.code(),
        Some("sync-engine.checkpoint-schema-newer"),
        "对端版本过旧必须显式报码化错误，非模糊失败"
    );

    let in_main: i64 = target
        .query_row(
            "SELECT COUNT(*) FROM main.sqlite_master WHERE type = 'table' AND name = 'sync_ops'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(in_main, 1, "主库同步表未被拆除（换入未发生）");
    let landed: i64 = target
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(landed, 0, "快照业务数据不得就位");
    let version_after: i64 = target
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version_after, version_before, "目标库版本未被改动");
}

#[test]
fn bootstrap_consumes_sync_component_into_attached() {
    let conn_a = test_support::open();
    base_ledger(&conn_a);
    let dev_a = device_of(&conn_a);
    let cp = create_checkpoint(&conn_a).unwrap();

    let dst_dir = ScratchDir::new("cp-dual-dst");
    let dst_main = dst_dir.join("ledger.db");
    let dst_sync = dst_dir.join("sync.db");
    {
        let factory = test_support::open();
        factory
            .execute(
                "VACUUM INTO ?1",
                rusqlite::params![dst_main.to_string_lossy()],
            )
            .unwrap();
    }
    make_sync_only_db(&dst_sync);
    let mut conn_b = ledger_infra::db::open_connection(&dst_main).unwrap();

    bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap();

    let attached_ops: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM sync.sync_ops", [], |r| r.get(0))
        .unwrap();
    assert_eq!(attached_ops, 1, "同步件换入 attached：op 行就位");
    let attached_positions: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM sync.sync_stream_positions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(attached_positions, 1, "同步件换入 attached：位点行就位");
    let attached_position: i64 = conn_b
        .query_row(
            "SELECT applied_through FROM sync.sync_stream_positions WHERE device_id = ?1",
            [&dev_a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attached_position, 1);
    let main_txns: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM main.transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(main_txns, 1, "业务件换入 main");
}

const SPLIT_WORLD_SYNC_TABLES: [&str; 4] = [
    "sync_device",
    "sync_ops",
    "sync_parked_ops",
    "sync_stream_positions",
];

fn make_sync_only_db(sync_path: &Path) {
    let factory = test_support::open();
    factory
        .execute(
            "VACUUM sync INTO ?1",
            rusqlite::params![sync_path.to_string_lossy()],
        )
        .unwrap();
}

fn make_business_only_main(main_path: &Path) {
    let factory = test_support::open();
    factory
        .execute(
            "VACUUM INTO ?1",
            rusqlite::params![main_path.to_string_lossy()],
        )
        .unwrap();
}

fn assert_business_file_has_no_sync_tables(bytes: &[u8]) {
    let dir = ScratchDir::new("cp-dual-probe-biz");
    let path = dir.join("biz.db");
    std::fs::write(&path, bytes).unwrap();
    let conn = ledger_infra::db::open_connection_unmounted(&path).unwrap();
    let sync_tables: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name LIKE 'sync_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sync_tables, 0, "业务件不含同步表");
}

fn assert_sync_file_carries_sync_tables(bytes: &[u8]) -> i64 {
    let dir = ScratchDir::new("cp-dual-probe-sync");
    let path = dir.join("sync.db");
    std::fs::write(&path, bytes).unwrap();
    let conn = ledger_infra::db::open_connection_unmounted(&path).unwrap();
    for table in SPLIT_WORLD_SYNC_TABLES {
        let present: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(present, 1, "同步件含 {table}");
    }
    let business_tables: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' \
             AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'sync_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(business_tables, 0, "同步件不含业务表");
    conn.query_row("SELECT COUNT(*) FROM sync_ops", [], |r| r.get(0))
        .unwrap()
}
