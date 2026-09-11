//! Checkpoint 检查点快照与新端引导（issue #857 / ADR-0091 决策 9）：任意时刻可
//! 产出 Checkpoint（全量快照 + 各设备 op 流已应用位点）、新端凭 Checkpoint 引导
//! 后仅重放位点之后的 op 即达一致状态（确定性重建）、位点对挂起 op 的安全钉住、
//! 截断机制三硬约束（位点之前才可删、挂起 op 永不删、只有来源设备有权截断自己
//! 的流；v1 默认不启用）。
//!
//! 判据权威 = 同步引擎公开接口；双端场景 = 同进程两个引擎实例 + 内存假 Transport。

use super::super::{
    OpOutcome, apply_ops, bootstrap_from_checkpoint, create_checkpoint, ops_after_positions,
    parked_ops, read_ops, stream_positions, truncate_stream_before,
};
use super::common::{make_expense, read_transaction, wire_in, wire_out};
use crate::sync_engine::{ops, positions};
use crate::test_support::{self, assert_balance_cache_matches_realtime, seed_account};
use crate::transaction::behavior;

/// 读本机设备标识（测试判据用）。
fn device_of(conn: &rusqlite::Connection) -> String {
    conn.query_row("SELECT id FROM sync_device LIMIT 1", [], |r| r.get(0))
        .unwrap()
}

/// 交易行数（判据读取）。
fn txn_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap()
}

/// 共同基底：A 端种子账户并创建一笔交易，返回交易 id。
fn base_ledger(conn_a: &rusqlite::Connection) -> String {
    seed_account(conn_a, "acc-1", "现金", "cash", "CNY", 0);
    behavior::create(conn_a, make_expense("acc-1", 10000, "午饭"))
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
    behavior::delete(&conn_b, &id).unwrap();
    behavior::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();
    let t2 = behavior::create(&conn_a, make_expense("acc-1", 2500, "咖啡"))
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
    behavior::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();
    let t2 = behavior::create(&conn_a, make_expense("acc-1", 2500, "咖啡"))
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
    behavior::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();

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
    let t2 = behavior::create(&conn_b, make_expense("acc-1", 2500, "咖啡"))
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
    behavior::create(&conn_b, make_expense("acc-1", 100, "已有账")).unwrap();

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
    behavior::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();

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
    assert_eq!(stream_positions(&conn_b).unwrap()[0].applied_through, 1);
    assert_eq!(stream_positions(&conn_b).unwrap()[0].device_id, dev_a);
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
    let watermark = stream_positions(&conn_b).unwrap()[0].applied_through;
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
    assert_eq!(
        stream_positions(&conn_b).unwrap()[0].applied_through,
        watermark
    );
}

/// 截断后同步不断不复活：源端截掉自己的旧 op 后，对端全量重投不复活已删 op
/// （本机流位点门拦截），新 op 继续正常同步；对端挂起队列不受影响。
#[test]
fn sync_continues_after_truncation_without_resurrecting_truncated_ops() {
    let (conn_a, conn_b, _t2) = pinned_world();
    let dev_a = device_of(&conn_a);

    // A 截掉位点之前的自己流 op（水位 = B 的挂起钉住位点 1）。
    let watermark = stream_positions(&conn_b).unwrap()[0].applied_through;
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
    let t3 = behavior::create(&conn_a, make_expense("acc-1", 9900, "打车"))
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
    let dir = std::env::temp_dir().join(format!("ledger-cp-test-{}", crate::db::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("ledger.db");

    // 工厂库（内存、明文、已迁移）写入账目 → VACUUM INTO 产出已迁移文件库
    // → 整库转密文 → 凭主口令重开为加密源端。
    {
        let seed = test_support::open();
        seed_account(&seed, "acc-1", "现金", "cash", "CNY", 0);
        behavior::create(&seed, make_expense("acc-1", 10000, "午饭")).unwrap();
        seed.execute(
            "VACUUM INTO ?1",
            rusqlite::params![db_path.to_string_lossy()],
        )
        .unwrap();
    }
    crate::db::encryption::enable_encryption_for_file(&db_path, "correct horse").unwrap();
    let conn_a = crate::db::open_connection_with_passphrase(&db_path, "correct horse").unwrap();
    let id = read_ops(&conn_a)
        .unwrap()
        .iter()
        .find_map(|op| op.command.subject().1.map(|eid| eid.into_owned()))
        .expect("种子交易 op 在场");

    let cp = create_checkpoint(&conn_a).unwrap();
    drop(conn_a);
    assert_eq!(
        crate::db::encryption::probe_file_kind(&{
            let p = dir.join("snap-probe.db");
            std::fs::write(&p, &cp.snapshot).unwrap();
            p
        })
        .unwrap(),
        crate::db::encryption::DbFileKind::Encrypted,
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

    std::fs::remove_dir_all(&dir).ok();
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
    behavior::create(&conn_a, make_expense("acc-1", 2500, "咖啡")).unwrap();
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

/// 位点前滚吸收已补齐区段：挂起 op 补齐后落日志，下一次推进前滚跨过整段
/// 缺口，水位落到已裁决末端（「补齐后自愈」的机制根据；白盒走域内推进接缝）。
#[test]
fn advance_rolls_forward_across_backfilled_range() {
    let (conn_a, conn_b, _t2) = pinned_world();
    let dev_a = device_of(&conn_a);
    // pinned_world：B 对 A 流位点 = 1（op2 挂起钉住），日志持有 op1、op3。
    assert_eq!(stream_positions(&conn_b).unwrap()[0].applied_through, 1);
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

    // 把快照化成 V021 时代的真实形态：卸下 V022 位点表与 V023 出资列
    //（先卸部分索引再卸列，SQLite 限制：索引列不可直接 DROP COLUMN）并回拨
    // user_version（user_version 以迁移条目计：V005 移除不回填，V022 = 第 21 条，
    // V021 时代 = 20）。
    let stale_path = std::env::temp_dir().join(format!("ledger-v21-{}.db", crate::db::new_uuid()));
    std::fs::write(&stale_path, &cp22.snapshot).unwrap();
    {
        let stale = crate::db::open_connection(&stale_path).unwrap();
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
        stale.execute("PRAGMA user_version = 20", []).unwrap();
    }
    let cp = super::super::Checkpoint {
        positions: cp22.positions,
        snapshot: std::fs::read(&stale_path).unwrap(),
    };
    crate::fs_util::cleanup(&stale_path);

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
        !crate::sync_engine::checkpoint::library_has_user_data(&conn).unwrap(),
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
        crate::sync_engine::checkpoint::library_has_user_data(&conn).unwrap(),
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
        crate::sync_engine::checkpoint::library_has_user_data(&conn).unwrap(),
        "只有用户自建账户（无交易）也应判为已有业务数据"
    );
}
