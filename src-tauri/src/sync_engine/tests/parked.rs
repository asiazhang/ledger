//! ParkedOp 挂起场景（issue #856 / ADR-0091 决策 6）：外键依赖失败（engine.rs
//! 已覆盖）之外的挂起面——schema 版本偏斜双向与不可支持命令。
//!
//! 双端场景 = 同进程两个引擎实例 + 内存假 Transport（wire 形态 JSON 字符串）；
//! 断言权威 = 同步引擎公开接口。

use super::super::{OpOutcome, ingest_ops, parked_ops, read_ops};
use super::common::{make_expense, read_transaction, wire_in, wire_out};
use crate::accounts::delete_account;
use crate::test_support::{self, seed_account};
use crate::transaction::write::protocol;

/// 旧端收到新命令（op 产生时 schema 版本超前本端）：挂起并提示升级，不静默
/// 丢弃，批内其余 op 不受阻。
#[test]
fn schema_ahead_op_parks_with_upgrade_hint() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    let created = protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭"))
        .unwrap()
        .id;
    let mut op = read_ops(&conn_a).unwrap().remove(0);
    op.schema_version += 1; // 合成「产生自更新版本」的信封
    let ahead = serde_json::to_string(&op).unwrap();

    // 偏斜 op 与正常 op 同批：偏斜者挂起，正常者照常落地。
    protocol::create(&conn_a, make_expense("acc-1", 500, "咖啡")).unwrap();
    let good_op = wire_out(&conn_a)[1].clone();
    let reports = ingest_ops(&conn_b, &[good_op, ahead]).unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, OpOutcome::Applied, "正常 op 不受阻");
    assert!(
        matches!(
            &reports[1].outcome,
            OpOutcome::Parked { code, .. } if code == "sync-engine.schema-ahead"
        ),
        "schema 超前挂起并携带码化原因"
    );

    // 挂起队列可查：op 身份保留、载荷原样（升级后可重放），账本无其效果。
    let parked = parked_ops(&conn_b).unwrap();
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].op_id, op.op_id);
    assert_eq!(parked[0].schema_version, op.schema_version);
    assert!(
        read_transaction(&conn_b, &created).is_none(),
        "未执行不落地"
    );
}

/// 旧命令重放到新 schema（wire 载荷不可解码）与信封损坏：按信封可读性挂起，
/// 不中断整批、不静默丢弃。
#[test]
fn undecodable_wire_ops_park_and_never_drop() {
    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);

    // 未知实体 tag（旧端载荷在新 schema 上不可解 / 新端命令发到旧端）：
    // 信封可读，身份保留。
    let unknown_entity = r#"{"op_id":"op-future","device_id":"dev-x","clock":9,"schema_version":20,"command":{"entity":"future-entity","payload":{"anything":1}}}"#;
    // 信封损坏：合成身份（parked- 前缀），原文入队。
    let garbage = "not-json-at-all";

    let reports = ingest_ops(&conn, &[unknown_entity.into(), garbage.into()]).unwrap();
    assert_eq!(reports.len(), 2);
    for report in &reports {
        assert!(
            matches!(
                &report.outcome,
                OpOutcome::Parked { code, .. } if code == "sync-engine.op-undecodable"
            ),
            "不可解码挂起并携带码化原因"
        );
    }

    let parked = parked_ops(&conn).unwrap();
    assert_eq!(parked.len(), 2, "全部入队，零静默丢弃");
    let future = parked
        .iter()
        .find(|p| p.op_id == "op-future")
        .expect("信封可读则身份保留");
    assert_eq!(future.entity, "future-entity");
    assert_eq!(
        future.payload, unknown_entity,
        "载荷原样保存（供升级后重放与裁决）"
    );
    let synthetic = parked
        .iter()
        .find(|p| p.op_id != "op-future")
        .expect("信封损坏另有条目");
    assert!(
        synthetic.op_id.starts_with("parked-"),
        "信封不可读则合成 id"
    );
    assert_eq!(synthetic.payload, garbage);
    assert!(read_ops(&conn).unwrap().is_empty(), "挂起不进日志");
    // 不可解码挂起同样携带插值参数（issue #957）：详情即 params[0]，
    // 前端按 sync-engine.op-undecodable 模板插出解析详情而非空悬冒号。
    for row in &parked {
        assert_eq!(row.code, "sync-engine.op-undecodable");
        assert_eq!(row.params.len(), 1, "解析详情应作唯一插值参数");
        assert!(!row.params[0].is_empty());
        assert!(
            row.message.ends_with(&row.params[0]),
            "message 已渲染：应以 params[0] 结尾，实际 {:?}",
            row.message
        );
    }
}

/// 往已删账户记账（AC3 旗舰场景）：重放命中账户存活守卫，码化挂起、不落地、
/// 不复活已删账户；依赖恢复（重开账户）后重投递自然重试成功即出队。
#[test]
fn replay_onto_deleted_account_parks_and_never_resurrects() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_a, "acc-x", "将删账户", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-x", "将删账户", "cash", "CNY", 0);

    // A 先在 acc-x 上记一笔并同步（两端一致、账户存活）。
    let id = protocol::create(&conn_a, make_expense("acc-x", 10000, "午饭"))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));

    // B 端账户被删（真实世界对应 #860 同步到达账户删除 op；此处经公开写入口合成）。
    delete_account(&conn_b, "acc-x").unwrap();

    // A 继续在 acc-x 上改、记：重放到 B 全部命中账户存活守卫。
    protocol::update(&conn_a, &id, make_expense("acc-x", 10000, "午饭（改）")).unwrap();
    protocol::create(&conn_a, make_expense("acc-x", 500, "咖啡")).unwrap();
    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(reports[0].outcome, OpOutcome::Skipped, "已应用 op 幂等跳过");
    for report in &reports[1..] {
        assert!(
            matches!(&report.outcome,
                OpOutcome::Parked { code, .. } if code == "account.not-found"),
            "往已删账户记账挂起并发码化错误"
        );
    }

    // 不静默落地、不自动复活：账本无新效果，账户保持已删，挂起队列可查。
    assert_eq!(
        read_transaction(&conn_b, &id).unwrap().note.as_deref(),
        Some("午饭"),
        "修改未落地"
    );
    let count: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "新创建未落地");
    let alive: i64 = conn_b
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE id='acc-x' AND is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(alive, 0, "已删账户不复活");
    let parked = parked_ops(&conn_b).unwrap();
    assert_eq!(parked.len(), 2);
    // 码化原因三要素同源落库（issue #957）：params 承载动态值（账户 id），
    // 前端才能按 errors.<code> 模板插出完整句（否则渲染成「账户不存在或已删除: 」）。
    for row in &parked {
        assert_eq!(row.code, "account.not-found");
        assert_eq!(
            row.params,
            vec!["acc-x".to_string()],
            "params 应携带账户 id"
        );
        assert!(
            row.message.contains("acc-x"),
            "message 是已渲染完整句，应含动态值，实际 {:?}",
            row.message
        );
    }

    // 重复失败投递：按 op 幂等覆盖挂起行，不堆积。
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 2, "重投递不堆积挂起行");
    let count: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "仍不落地");
}

// 投资命令重放自 #861 起由同步引擎完整执行（buy/sell 三件套），挂起场景
// （标的不存在、可卖数量依赖倒挂）收敛在 tests/investment.rs。
