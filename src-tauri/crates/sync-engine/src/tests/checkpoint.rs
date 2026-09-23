//! Checkpoint 检查点快照与新端引导（issue #857 / ADR-0091 决策 9）：任意时刻可
//! 产出 Checkpoint（全量快照 + 各设备 op 流已应用位点）、新端凭 Checkpoint 引导
//! 后仅重放位点之后的 op 即达一致状态（确定性重建）、位点对挂起 op 的安全钉住、
//! 截断机制三硬约束（位点之前才可删、挂起 op 永不删、只有来源设备有权截断自己
//! 的流；v1 默认不启用）。
//!
//! 判据权威 = 同步引擎公开接口；双端场景 = 同进程两个引擎实例 + 内存假 Transport。

use std::collections::BTreeMap;
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

/// 读本机设备标识（测试判据用）。
fn device_of(conn: &rusqlite::Connection) -> String {
    conn.query_row("SELECT id FROM sync_device LIMIT 1", [], |r| r.get(0))
        .unwrap()
}

/// 来源设备（A）在本端的已应用位点：B 端同时持有**自己流**的位点行，位点清单
/// 又是「按 DeviceId 序」的多行集合——用 `[0]` 取 A 流会随设备标识取值漂移
/// （UUIDv7 同一毫秒内生成时，顺序由随机位决定；#1112 范围外修复，见该 PR）。
/// 一律按 device_id 定位，断言只钉住「A 流水位」这一语义。
fn position_of_stream(conn: &rusqlite::Connection, device_id: &str) -> i64 {
    stream_positions(conn)
        .unwrap()
        .into_iter()
        .find(|p| p.device_id == device_id)
        .expect("位点行在列")
        .applied_through
}

/// 交易行数（判据读取）。
fn txn_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap()
}

/// 共同基底：A 端种子账户并创建一笔交易，返回交易 id。
fn base_ledger(conn_a: &rusqlite::Connection) -> String {
    seed_account(conn_a, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(conn_a, make_expense("acc-1", 10000, "午饭"))
        .unwrap()
        .id
}

/// 挂起钉住场景：A 建 t1 同步到 B；B 删 t1；A 更新 t1（op2，B 端重放命中软删行
/// 而挂起）并建 t2（op3，B 端正常应用）。B 对 A 流的位点被挂起的 op2 钉在 1。
/// 返回 (A, B, t2)。
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

// ---------------------------------------------------------------------------
// 任意时刻可产出 Checkpoint（全量数据快照 + 各设备 op 流已应用位点）
// ---------------------------------------------------------------------------

/// 有活动日志的库任意时刻可产出 Checkpoint：位点覆盖已应用流、快照字节非空。
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

/// 无任何同步活动的库同样可产出 Checkpoint（位点为空，快照即当前全量状态）。
#[test]
fn checkpoint_on_quiescent_ledger_has_empty_positions() {
    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);

    let cp = create_checkpoint(&conn).unwrap();
    assert!(!cp.snapshot.is_empty());
    assert!(cp.positions.is_empty(), "无 op 流则无位点");
}

/// 位点读取接缝与 Checkpoint 位点同源（通道 manifest 上报位点的数据面）。
#[test]
fn stream_positions_reader_matches_checkpoint_positions() {
    let conn_a = test_support::open();
    base_ledger(&conn_a);

    let cp = create_checkpoint(&conn_a).unwrap();
    assert_eq!(stream_positions(&conn_a).unwrap(), cp.positions);
}

// ---------------------------------------------------------------------------
// 新端引导：Checkpoint + 位点之后的 op = 一致状态（确定性重建）
// ---------------------------------------------------------------------------

/// 核心判据：新端凭 Checkpoint 引导后，仅重放位点之后的 op 即与源端一致。
#[test]
fn bootstrap_plus_ops_after_positions_reaches_source_state() {
    let conn_a = test_support::open();
    let mut conn_b = test_support::open();
    let id = base_ledger(&conn_a);

    // 快照时刻：A 只有 t1。
    let cp = create_checkpoint(&conn_a).unwrap();

    // 快照之后 A 继续记账：改 t1、建 t2（位点之后的新 op）。
    protocol::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();
    let t2 = protocol::create(&conn_a, make_expense("acc-1", 2500, "咖啡"))
        .unwrap()
        .id;

    // 新端 B 引导：拿到快照时刻状态（t1 = 「午饭」）与位点。
    bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap();
    assert_eq!(
        read_transaction(&conn_b, &id).unwrap().note.as_deref(),
        Some("午饭"),
        "引导后为快照时刻状态"
    );

    // 仅重放位点之后的 op。
    let after = ops_after_positions(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();
    assert_eq!(after.len(), 2, "位点之前的 op 无需重放");
    let reports = apply_ops(&conn_b, &after).unwrap();
    assert!(reports.iter().all(|r| r.outcome == OpOutcome::Applied));

    // 判据：B 与 A 状态一致（业务字段 + 日志），余额缓存自洽。
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

/// 确定性重建：同一 Checkpoint 引导的不同新端，重放其后 op 后状态彼此一致。
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

/// 引导换入本机设备身份：新端随后的 op 落在自己的新流（原流位点被采纳、不串流）。
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

    // B 随后记账：op 落在 B 自己的新流，时钟从 1 起；A 流位点原样采纳。
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

/// 引导守卫：目标已参与同步（有日志）时拒绝，不覆盖既有同步状态。
#[test]
fn bootstrap_rejects_non_fresh_target() {
    let conn_a = test_support::open();
    let mut conn_b = test_support::open();
    base_ledger(&conn_a);
    let cp = create_checkpoint(&conn_a).unwrap();

    // B 已有自己的日志（参与过同步）。
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_b, make_expense("acc-1", 100, "已有账")).unwrap();

    let err = bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap_err();
    assert_eq!(err.code(), Some("sync-engine.bootstrap-not-fresh"));
}

/// 引导守卫：快照来自更高版本的应用时拒绝（schema 偏斜，升级后再引导）。
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

/// 位点门防重执行：引导后对端全量重投（含位点之前的 op）不产生第二次效果。
#[test]
fn redelivery_of_ops_at_or_below_position_skips() {
    let conn_a = test_support::open();
    let mut conn_b = test_support::open();
    let id = base_ledger(&conn_a);
    let cp = create_checkpoint(&conn_a).unwrap();
    protocol::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();

    bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap();

    // 重投位点之前的 op（op1 已并入快照谱系）与位点上的 op：一律跳过。
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

// ---------------------------------------------------------------------------
// 位点对挂起 op 的安全钉住与截断机制
// ---------------------------------------------------------------------------

/// 挂起 op 钉住位点：流内存在未应用（挂起）op 时，位点停在其之前——
/// 「仅重放位点之后」的增量拉取仍会拿到它，重投递自然重试（不丢）。
#[test]
fn positions_pinned_below_parked_op() {
    let (conn_a, conn_b, t2) = pinned_world();

    // op3 已应用，但位点被挂起的 op2 钉在 1（不越过任何未应用 op）。
    let dev_a = device_of(&conn_a);
    assert_eq!(position_of_stream(&conn_b, &dev_a), 1);
    assert_eq!(
        read_transaction(&conn_b, &t2).unwrap().amount_cents,
        2500,
        "op3 正常应用"
    );
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1, "op2 挂起待裁决");
}

/// 截断三硬约束：只有来源设备有权截断自己的流；位点之前才可删；
/// 挂起队列中的 op（位点被钉住而保留在日志中）不被截断；位点表不受影响。
#[test]
fn truncation_is_owner_only_position_bounded_and_keeps_unapplied_ops() {
    let (conn_a, conn_b, _t2) = pinned_world();
    let dev_a = device_of(&conn_a);

    // 硬约束三：非来源设备无权截断该流。
    let err = truncate_stream_before(&conn_b, &dev_a, 99).unwrap_err();
    assert_eq!(err.code(), Some("sync-engine.truncate-not-owner"));

    // 截断水位 = 各端上报位点的最小值（此处 B 对 A 流的位点为 1，被挂起 op 钉住）。
    let watermark = position_of_stream(&conn_b, &dev_a);
    let deleted = truncate_stream_before(&conn_a, &dev_a, watermark).unwrap();
    assert_eq!(deleted, 1, "只删位点之前（时钟 ≤ 1）的 op");

    // 位点之后的 op 全部保留：其中包含被 B 挂起的 op2（未应用，永不删）。
    assert_eq!(
        read_ops(&conn_a).unwrap().len(),
        2,
        "挂起 op 与其后 op 不受截断影响"
    );
    assert_balance_cache_matches_realtime(&conn_a);

    // 截断不碰位点表：位点仍在，增量拉取口径不回退。
    assert_eq!(position_of_stream(&conn_b, &dev_a), watermark);
}

/// 截断后同步不断不复活：源端截掉自己的旧 op 后，对端全量重投不复活已删 op
/// （本机流位点门拦截），新 op 继续正常同步；对端挂起队列不受影响。
#[test]
fn sync_continues_after_truncation_without_resurrecting_truncated_ops() {
    let (conn_a, conn_b, _t2) = pinned_world();
    let dev_a = device_of(&conn_a);

    // A 截掉位点之前的自己流 op（水位 = B 的挂起钉住位点 1）。
    let watermark = position_of_stream(&conn_b, &dev_a);
    truncate_stream_before(&conn_a, &dev_a, watermark).unwrap();

    // B 全量重投自己的日志（含 A 已截掉的 op1）：A 靠本机流位点跳过不复活；
    // B 的删除 op 按 LWW 被压制（A 端更新为序末者）。
    let reports = wire_in(&conn_a, &wire_out(&conn_b));
    assert!(
        reports
            .iter()
            .all(|r| matches!(r.outcome, OpOutcome::Skipped | OpOutcome::Superseded)),
        "位点已覆盖的 op 跳过、LWW 输者压制，无重放"
    );
    assert_eq!(txn_count(&conn_a), 2, "不复活、不重复");

    // 截断后新 op 继续正常同步：B 应用 A 的新流 op。
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

/// 密文快照往返（ADR-0091 决策 8 同构）：加密源库的 Checkpoint 快照为密文
/// （VACUUM INTO 继承源库加密），新端凭主口令引导；缺口令与错口令分别报
/// 可重试码化错误，不裸上抛。
#[test]
fn encrypted_checkpoint_roundtrip_and_passphrase_guards() {
    // 暂存目录（ScratchDir guard，issue #1645）：drop（含 panic unwind）整棵删除。
    let dir = ScratchDir::new("cp-test");
    let db_path = dir.join("ledger.db");

    // 工厂库（内存、明文、已迁移）写入账目 → VACUUM INTO 产出已迁移文件库
    // → 整库转密文 → 凭主口令重开为加密源端。
    {
        let seed = test_support::open();
        seed_account(&seed, "acc-1", "现金", "cash", "CNY", 0);
        protocol::create(&seed, make_expense("acc-1", 10000, "午饭")).unwrap();
        seed.execute(
            "VACUUM INTO ?1",
            rusqlite::params![db_path.to_string_lossy()],
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
    // 缺口令：码化错误、可就地重试。
    let err = bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap_err();
    assert_eq!(
        err.code(),
        Some("sync-engine.checkpoint-passphrase-required")
    );
    // 错误口令：归一为可重试的口令错误（与密文备份恢复同形态）。
    let err = bootstrap_from_checkpoint(&mut conn_b, &cp, Some("wrong")).unwrap_err();
    assert_eq!(err.code(), Some("encryption.passphrase-incorrect"));
    // 正确口令：引导成功，业务数据在场。
    bootstrap_from_checkpoint(&mut conn_b, &cp, Some("correct horse")).unwrap();
    assert_eq!(read_transaction(&conn_b, &id).unwrap().amount_cents, 10000);
    assert_balance_cache_matches_realtime(&conn_b);
}

/// 首见流批次内含未裁决 op（wire 不可解挂起：不落日志、不触发推进）：位点
/// 从 0 起步、被缺口挡住，不越过任何未应用 op（首见若直接以裁决时钟建行会
/// 越过挂起 op，属不变量违例——回归钉）。
#[test]
fn first_sighting_with_pending_park_pins_position_below_it() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    base_ledger(&conn_a);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 2500, "咖啡")).unwrap();
    // A 流 [op1, op2]；投递时 op1 的 wire 原文被篡改为不可解（合成挂起），
    // op2 正常解析应用——B 对 A 流首见裁决即含未应用缺口。
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

// ---------------------------------------------------------------------------
// 引导编排的锁段形状（issue #1285，ADR-0120 判据同簇适用）：整库快照下载是
// 纯网络等待、在连接段之外完成（ADR-0069 决策 4）；段2 复验前置守卫把
// 「判空后被写」的并发本地写收敛为显式报错，不被快照静默覆盖（丢账）。
// ---------------------------------------------------------------------------

/// 共享字节通道：发布侧与引导侧各持传输替身、读写同一份文件。
type SharedFiles = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

/// 通道读钩子形态（入参为通道对象路径）。
type FetchHook = Box<dyn Fn(&str) + Send + Sync>;

/// 内存通道传输替身：引导侧挂「读通道文件」钩子——引导下载段的两次通道读
///（manifest、快照体）在钩子处触发，此刻即「网络在途」。
struct FetchObservedTransport {
    files: SharedFiles,
    on_read: Option<FetchHook>,
}

impl FetchObservedTransport {
    /// 发布侧实例：无钩子（发布的通道读不进引导的事件序）。
    fn publisher(files: &SharedFiles) -> Self {
        Self {
            files: files.clone(),
            on_read: None,
        }
    }

    /// 引导侧实例：每次通道读先执行钩子再应答。
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

/// 段接缝替身（同壳层实现的取锁形状：互斥体 + 每段短取、用毕即还）；段事件
/// 与「连接存活」标记共享给下载段钩子——下载期间互斥体必然空闲。
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

/// 测试用通道布局（固定空间 id，与其它用例隔离）。
fn bootstrap_layout() -> ChannelLayout {
    ChannelLayout::new("0197abcd-0000-7000-8000-000000000085").unwrap()
}

/// 来源端 A（账户 + 一笔「午饭」支出）把检查点发布到共享字节通道，返回快照
/// 携带的交易 id。
fn publish_source_checkpoint(files: &SharedFiles) -> String {
    let conn_a = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    let id = protocol::create(&conn_a, make_expense("acc-1", 10_000, "午饭"))
        .unwrap()
        .id;
    // 两段式发布（#1284 后形态）：产出段定格快照 + 发布段封包上通道。
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

/// 无注入钩子的引导侧通道：每次通道读记入事件序（manifest / 快照体）并
/// 断言此刻无连接存活（下载段不持连接）。
fn observed_channel(
    files: &SharedFiles,
    events: Arc<Mutex<Vec<&'static str>>>,
    conn_live: Arc<AtomicBool>,
) -> crate::SyncChannel {
    observed_channel_with_snapshot_hook(files, events, conn_live, || ())
}

/// 带注入钩子的引导侧通道：快照体读时（即下载在途）额外执行 `on_snapshot`
///（模拟下载在途时落进本机的并发写）；manifest 读只记事件不注入。
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
            ("fetch-snapshot", true)
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

/// 形态对齐守卫的本机库路径（域单测用内存库；明文 × 明文场景该路径只被
/// probe，不发生文件写入）。路径住暂存目录（issue #1645），guard 随语句析构。
fn probe_only_db_path() -> ScratchFile {
    ScratchFile::new(
        "bootstrap-domain",
        format!(
            "ledger-bootstrap-domain-{}.db",
            ledger_infra::db::new_uuid()
        ),
    )
}

/// 引导事件序的期望形态：段1（前置守卫）→ manifest 读 → 快照体读 →
/// 段2（复验 + 换入），两次通道读都严格落在两段之间。
fn assert_fetch_between_segments(events: &[&'static str]) {
    assert_eq!(
        events,
        [
            "seg-start",
            "seg-end",
            "fetch-manifest",
            "fetch-snapshot",
            "seg-start",
            "seg-end"
        ],
        "下载段必须整体位于两段之间（段1 前置守卫 → 锁外下载 → 段2 复验换入）"
    );
}

/// 核心形状（issue #1285）：前置守卫（段1）→ 整库快照下载（连接段外，纯网络
/// 等待）→ 复验 + 整库换入（段2）——下载不持连接，引导结果与整段持锁形状一致。
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

    // 引导结果与整段持锁形状一致：快照数据就位（判据权威 = 域公开接口）。
    let conn = harness.conn.lock().unwrap();
    assert_eq!(
        read_transaction(&conn, &source_txn).unwrap().amount_cents,
        10_000,
        "快照携带的交易随引导就位"
    );
}

/// 复验收敛（issue #1285）：下载期间落进的本机写在段2 复验被显式拒绝——
/// 不静默覆盖（换入是整库重建语义，覆盖即丢账），本机写原样保留。
#[test]
fn bootstrap_local_write_during_download_is_caught_by_recheck_not_overwritten() {
    let files: SharedFiles = Arc::new(Mutex::new(BTreeMap::new()));
    let source_txn = publish_source_checkpoint(&files);

    let harness = SegmentsStub::new(test_support::open());
    // 下载段钩子模拟「并发本地写」：快照体在途时往本机落一个用户事实行
    //（账户行即探针闭集成员；直置不经写入口，同前同步时代存量库守卫先例）。
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

    // 拒绝零副作用：本机写原样保留，快照未换入（来源数据不在场）。
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

/// 前置守卫 fail fast（段1）：本机已有用户业务数据时引导在下载前被拒——
/// 不浪费整库快照下载（通道读零次）。
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

/// 位点前滚吸收已补齐区段：挂起 op 补齐后落日志，下一次推进前滚跨过整段
/// 缺口，水位落到已裁决末端（「补齐后自愈」的机制根据；白盒走域内推进接缝）。
#[test]
fn advance_rolls_forward_across_backfilled_range() {
    let (conn_a, conn_b, _t2) = pinned_world();
    let dev_a = device_of(&conn_a);
    // pinned_world：B 对 A 流位点 = 1（op2 挂起钉住），日志持有 op1、op3。
    assert_eq!(position_of_stream(&conn_b, &dev_a), 1);
    // 模拟「op2 补齐后成功应用」：经域内接缝落日志（绕过重放分派，仅此白盒）。
    let op2 = read_ops(&conn_a)
        .unwrap()
        .into_iter()
        .find(|op| op.clock == 2)
        .expect("A 流时钟 2 的 op 在源日志");
    ops::insert_row(&conn_b, &op2).unwrap();
    // 下一次裁决落定的推进（任意该流时钟 > 1 的 op）把水位滚到已裁决末端。
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

/// 引导 schema 偏斜（较旧快照）：重建后对齐快照版本并前向迁移升级到本端
/// 最新版本（V022 位点表在场、V023 出资列在场、位点写入可用、业务数据完整）。
#[test]
fn bootstrap_migrates_older_schema_snapshot() {
    let conn_a = test_support::open();
    let id = base_ledger(&conn_a);
    let cp22 = create_checkpoint(&conn_a).unwrap();

    // 把快照化成 V021 时代的真实形态：卸下 V022 位点表、V023 出资列、V026 信用卡
    // 档案列与 V027 期初存量列（快照取自当前最新 schema，逐版回退到此时代才叫
    // 「V021 时代的真实形态」；新增迁移若往 V021 之后加对象，须在此同步卸下——
    // 否则前向迁移重放该条 DDL 会撞 duplicate column）
    //（先卸部分索引再卸列，SQLite 限制：索引列不可直接 DROP COLUMN）并回拨
    // user_version（user_version 以迁移条目计：V005 移除不回填，V022 = 第 21 条，
    // V021 时代 = 20）。V025 起迁移链含 DROP：模拟旧时代快照还须把「该时代
    // 在场、后被移除」的对象按原 DDL 复位（V025 删除的 6 索引，DDL 同
    // V001/V006），否则前向迁移到 V025 时 DROP 落空报 no such index。
    // 同理 V031（#1728）退役 V018 的 note_pinyin：派生列与两个索引在 V021 时代
    // 在场，前向重放 V031 的 DROP 需要它们在场——按 V018 原 DDL 复位。
    // 旧时代快照夹具（issue #1645）：散文件收进暂存目录，guard 随用例清理。
    let stale_path = ScratchFile::new(
        "v21",
        format!("ledger-v21-{}.db", ledger_infra::db::new_uuid()),
    );
    std::fs::write(&stale_path, &cp22.snapshot).unwrap();
    {
        let stale = ledger_infra::db::open_connection(&stale_path).unwrap();
        stale
            .execute("DROP TABLE sync_stream_positions", [])
            .unwrap();
        stale
            .execute("DROP INDEX IF EXISTS idx_transactions_funding", [])
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
            .execute("ALTER TABLE security_transactions DROP COLUMN origin", [])
            .unwrap();
        stale
            .execute(
                "ALTER TABLE instruments DROP COLUMN constant_unit_price",
                [],
            )
            .unwrap();
        // V029（issue #1548）：折算来源留痕两列。
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
        // V031（issue #1728）：复位 V018 引入、后被退役的 note_pinyin 派生列与两
        // 索引（DDL 同 V018 原文），否则前向重放到 V031 时 DROP 落空报 no such
        // index。先加列再建引用它的索引；快照里的搜索覆盖索引已是 V031 重建后的
        // 新形态（列集无 note_pinyin），先卸下再按 V018 原形态重建才是该时代形态。
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
    };

    let mut conn_b = test_support::open();
    bootstrap_from_checkpoint(&mut conn_b, &cp, None).unwrap();

    // 迁移升级完成：版本与全新库一致、位点表在场且位点写入生效、数据完整。
    let fresh = test_support::open();
    let expected: i64 = fresh
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    let actual: i64 = conn_b
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(actual, expected, "引导后迁移升级到本端最新 schema");
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

// ---------------------------------------------------------------------------
// 「加入即新库」守卫：library_has_user_data 探针（issue #864 壳层引导前置）
// ---------------------------------------------------------------------------

/// 全新空库（含种子行）无用户业务数据：种子以 device_id='seed' 排除，
/// 不误报——误报会让全新设备永远无法引导。
#[test]
fn fresh_library_has_no_user_data() {
    let conn = test_support::open();
    assert!(
        !crate::checkpoint::library_has_user_data(&conn).unwrap(),
        "全新库（含种子分类/币种/黑洞账户）不应判为已有业务数据"
    );
}

/// 任一业务域的用户事实行都触发探针：交易（主探针）与无交易的纯参考数据
///（用户建的账户/分类）同样判为已有数据——引导整库换入会覆盖它们。
#[test]
fn user_fact_rows_in_any_business_domain_trigger_probe() {
    // 交易在位（主探针）。
    let conn = test_support::open();
    base_ledger(&conn);
    assert!(
        crate::checkpoint::library_has_user_data(&conn).unwrap(),
        "有交易的库应判为已有业务数据"
    );

    // 纯参考数据：用户建的账户（device_id 非种子）而无任何交易。
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
