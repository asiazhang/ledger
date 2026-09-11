//! 写入协议的协议级对称断言（issue #1004 / ADR-0105）：Local / Replay 两形态
//! 跑同一断言集——kind 守卫（dividend 暂不支持）与 convert / split kind 变更
//! 禁止同码同文案、op 发射开关（重放不产本地 op）、id 来源（重放随命令携带）。
//! 本地形态经行为层既有编排入口驱动；重放形态经同步引擎 dispatch 的既有消费面
//! （`replay_command`）驱动。
//!
//! 断言强度（ADR-0087）：只断言外部可观察行为——错误码与文案、落库终态、
//! OpLog 行内容；形态对称性本身由编译期单正文构造保证，不靠测试兜底。
//!
//! 删除即变红：删除协议本体的 kind 守卫单点（`kind_unsupported` 构造或其在
//! 协议守卫段的调用）、convert 变更守卫，或任一形态的装配 / 落库 / op 分支，
//! 下列断言至少一条变红——
//! - 删 kind 守卫单点：Replay 伪造 dividend 载荷将落库或改报他码，
//!   `*_same_code_both_forms` 红；
//! - 删 convert 变更守卫：`update_convert_kind_change_same_code_both_forms` 红；
//! - 删 Replay 落库分支（insert_row_with_id）：
//!   `replay_create_uses_carried_id_and_emits_no_local_op` 红；
//! - 删 Replay 的 op 发射开关（误产 op）：同上测试的零 op 断言红。
//!
//! 存活校验分支的删除即变红由 `sync_engine/tests/parked.rs` 的
//! `replay_onto_deleted_account_parks_and_never_resurrects` 承接：create 与
//! update 重放双双命中账户存活守卫挂起。

use rusqlite::Connection;

use super::super::*;
use super::common::make_input;
use crate::error::AppError;
use crate::sync_engine::read_ops;
use crate::test_support::{self, seed_account, seed_instrument};
use crate::transaction::TransactionCommand;
use crate::transaction::amount::TransactionKind;

/// 伪造/重放载荷构造器：随命令携带的归一化行（协议 Replay 形态的装配输入）。
fn carried_row(kind: TransactionKind, account: &str, amount: i64) -> NormalizedTransaction {
    NormalizedTransaction {
        kind,
        amount_cents: amount,
        currency_code: "CNY".into(),
        amount_native_cents: amount,
        account_id: account.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        policy_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-05-04".into(),
    }
}

/// 码化错误拆包：取 (code, message) 供两形态同码同文案比对。
fn coded_of(err: AppError) -> (String, String) {
    match err {
        AppError::Coded { code, message, .. } => (code, message),
        other => panic!("应为码化错误，实际: {other:?}"),
    }
}

/// 投资场景铺垫：投资账户 + 转出/转入两标的（币种同账户，1:1 折算），
/// 经本地编排入口建一笔买入（10 份 @ 1.5 元）与一笔转换（10 → 10）。
fn seed_buy_and_convert(conn: &Connection) -> (String, String) {
    seed_account(conn, "acc-cv", "基金户", "investment", "CNY", 0);
    seed_instrument(conn, "inst-out", "006793", "转出基金", "CNY", "unknown");
    seed_instrument(conn, "inst-in", "519700", "转入基金", "CNY", "unknown");
    let buy_input = crate::transaction::TransactionInput {
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: "acc-cv".into(),
        date: "2026-01-10".into(),
        instrument_id: Some("inst-out".into()),
        quantity: Some(10.0),
        price_cents: Some(10_000),
        fee_cents: Some(0),
        ..make_input("acc-cv", TransactionKind::Buy, 0, "2026-01-10")
    };
    let buy_id = create_transaction_internal(conn, buy_input).unwrap().id;
    let convert_input = crate::transaction::TransactionInput {
        kind: TransactionKind::Convert,
        amount_cents: 0,
        account_id: "acc-cv".into(),
        date: "2026-02-01".into(),
        instrument_id: Some("inst-out".into()),
        quantity: Some(10.0),
        to_instrument_id: Some("inst-in".into()),
        to_quantity: Some(10.0),
        out_amount_cents: Some(1_100),
        in_amount_cents: Some(1_100),
        fee_cents: Some(0),
        ..make_input("acc-cv", TransactionKind::Convert, 0, "2026-02-01")
    };
    let convert_id = create_transaction_internal(conn, convert_input).unwrap().id;
    (buy_id, convert_id)
}

/// kind 守卫（dividend 暂不支持）在创建协议按形态裁决，且均不落库（拒绝先于
/// 任何写入）。
#[test]
fn create_rejects_dividend_same_code_both_forms() {
    for kind in [TransactionKind::Dividend] {
        // Local：行为层创建编排入口。
        let conn_local = test_support::open();
        seed_account(&conn_local, "acc-g", "现金", "cash", "CNY", 0);
        let err_local =
            create_transaction_internal(&conn_local, make_input("acc-g", kind, 500, "2026-05-04"))
                .unwrap_err();

        // Replay：同步引擎 dispatch 的既有消费面（伪造 dividend 载荷——
        // 本地 plan 从不产出该命令）。
        let conn_replay = test_support::open();
        seed_account(&conn_replay, "acc-g", "现金", "cash", "CNY", 0);
        let err_replay = replay_command(
            &conn_replay,
            &TransactionCommand::Create {
                id: "sync-div-1".into(),
                row: carried_row(kind, "acc-g", 500),
                investment: None,
                split: None,
                convert: None,
            },
        )
        .unwrap_err();

        // 同码同文案：两端对同一违规操作返回同一裁决提示（spec 用户故事 2）。
        let (code_local, msg_local) = coded_of(err_local);
        let (code_replay, msg_replay) = coded_of(err_replay);
        assert_eq!(code_local, "transaction.kind-unsupported");
        assert_eq!(code_local, code_replay, "{kind} 两形态应同码");
        assert_eq!(msg_local, msg_replay, "{kind} 两形态应同文案");

        // 两端均不落库。
        for conn in [&conn_local, &conn_replay] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "{kind} 拒绝后不应落库");
        }
    }
}

/// kind 守卫在修改协议两形态同码同文案：改挂 dividend 显式拒绝（暂不支持）、
/// 改挂 split 由 kind 变更守卫拒绝（ADR-0106 决策 5），两形态同码同文案、
/// 原行保持不变。
#[test]
fn update_rejects_dividend_split_same_code_both_forms() {
    let expected_code = |kind: TransactionKind| match kind {
        TransactionKind::Split => "trade.split-kind-change-forbidden",
        _ => "transaction.kind-unsupported",
    };
    for kind in [TransactionKind::Dividend, TransactionKind::Split] {
        // Local：既有 expense 行，修改为 dividend/split。
        let conn_local = test_support::open();
        seed_account(&conn_local, "acc-g", "现金", "cash", "CNY", 0);
        let id_local = create_transaction_internal(
            &conn_local,
            make_input("acc-g", TransactionKind::Expense, 500, "2026-01-01"),
        )
        .unwrap()
        .id;
        let err_local = update_transaction_internal(
            &conn_local,
            &id_local,
            make_input("acc-g", kind, 500, "2026-05-04"),
        )
        .unwrap_err();

        // Replay：对 expense 行投递改挂 dividend/split 的修改命令（伪造载荷）。
        let conn_replay = test_support::open();
        seed_account(&conn_replay, "acc-g", "现金", "cash", "CNY", 0);
        let id_replay = create_transaction_internal(
            &conn_replay,
            make_input("acc-g", TransactionKind::Expense, 500, "2026-01-01"),
        )
        .unwrap()
        .id;
        let err_replay = replay_command(
            &conn_replay,
            &TransactionCommand::Update {
                id: id_replay.clone(),
                row: carried_row(kind, "acc-g", 500),
                investment: None,
                split: None,
                convert: None,
            },
        )
        .unwrap_err();

        let (code_local, msg_local) = coded_of(err_local);
        let (code_replay, msg_replay) = coded_of(err_replay);
        assert_eq!(code_local, expected_code(kind));
        assert_eq!(code_local, code_replay, "{kind} 两形态应同码");
        assert_eq!(msg_local, msg_replay, "{kind} 两形态应同文案");

        // 原行保持不变（拒绝即回滚，无半套副作用）。
        for (conn, id) in [(&conn_local, id_local), (&conn_replay, id_replay)] {
            let t = get_transaction_internal(conn, &id).unwrap();
            assert_eq!(t.kind, TransactionKind::Expense);
            assert_eq!(t.amount_cents, 500);
        }
    }
}

/// convert kind 变更守卫（ADR-0099 决策 5）在修改协议两形态同码：
/// 改为 convert 与改出 convert 均拒绝，重放端不得由伪造 op 绕过；守卫先于
/// 命令字段解包（字段缺失不影响守卫裁决——先裁决 kind 合法性、后装配）。
#[test]
fn update_convert_kind_change_same_code_both_forms() {
    // Local：改出 / 改入 convert。
    let conn_local = test_support::open();
    let (buy_id_l, convert_id_l) = seed_buy_and_convert(&conn_local);
    let err_local_in = update_transaction_internal(
        &conn_local,
        &buy_id_l,
        make_input("acc-cv", TransactionKind::Convert, 0, "2026-03-01"),
    )
    .unwrap_err();
    let err_local_out = update_transaction_internal(
        &conn_local,
        &convert_id_l,
        make_input("acc-cv", TransactionKind::Buy, 0, "2026-03-01"),
    )
    .unwrap_err();

    // Replay：同一违规操作投递为修改命令（转换字段 None——守卫先于装配）。
    let conn_replay = test_support::open();
    let (buy_id_r, convert_id_r) = seed_buy_and_convert(&conn_replay);
    let err_replay_in = replay_command(
        &conn_replay,
        &TransactionCommand::Update {
            id: buy_id_r,
            row: carried_row(TransactionKind::Convert, "acc-cv", 1_000),
            investment: None,
            split: None,
            convert: None,
        },
    )
    .unwrap_err();
    let err_replay_out = replay_command(
        &conn_replay,
        &TransactionCommand::Update {
            id: convert_id_r,
            row: carried_row(TransactionKind::Buy, "acc-cv", 0),
            investment: None,
            split: None,
            convert: None,
        },
    )
    .unwrap_err();

    // 四向同码：trade.convert-kind-change-forbidden。
    for err in [
        &err_local_in,
        &err_local_out,
        &err_replay_in,
        &err_replay_out,
    ] {
        assert!(
            matches!(err, AppError::Coded { code, .. } if code == "trade.convert-kind-change-forbidden"),
            "改为/改出 convert 应同码拒绝，实际: {err:?}"
        );
    }

    // 拒绝不留半套副作用：两端各两行原样在场、无软删。
    for conn in [&conn_local, &conn_replay] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "守卫拒绝后原行原样在场");
    }
}

/// 形态级专属（Replay）：id 随命令携带（两端对同一笔交易收敛到同一行），
/// 重放不产本地 op（op 发射开关的 Replay 臂）。对照 Local 形态：id 本端生成、
/// 恰产一条 op（oplog.rs 为 Local 侧断言权威）。
#[test]
fn replay_create_uses_carried_id_and_emits_no_local_op() {
    let conn = test_support::open();
    seed_account(&conn, "acc-r", "现金", "cash", "CNY", 0);

    replay_command(
        &conn,
        &TransactionCommand::Create {
            id: "sync-tx-1".into(),
            row: carried_row(TransactionKind::Expense, "acc-r", 6_400),
            investment: None,
            split: None,
            convert: None,
        },
    )
    .unwrap();

    // id 收敛：行落在命令携带的 id 上，不重新生成。
    let t = get_transaction_internal(&conn, "sync-tx-1").unwrap();
    assert_eq!(t.kind, TransactionKind::Expense);
    assert_eq!(t.amount_cents, 6_400);
    // 重放不产本地 op：本机 OpLog 恒空（外来 op 由引擎在重放事务内落日志）。
    assert!(
        read_ops(&conn).unwrap().is_empty(),
        "重放形态不得追加本地 op"
    );
}

/// split 的创建协议两形态（ADR-0106 / #1049 / #1053）：Local 经投资域
/// prepare_split 守卫（本场景缺标的 → trade.split-instrument-required，非 kind
/// 拒绝）；Replay 经投资域重放装配——旧载荷缺份额调整字段由 kind 防御臂码化
/// 拒绝（引擎 ParkedOp 承接），有字段但零在用持仓由本地同码守卫拒绝，均不落
/// 半套副作用。
#[test]
fn create_split_local_and_replay_enter_domain_guards() {
    // Local：split 进入投资域守卫（缺标的 → trade.split-instrument-required）。
    let conn_local = test_support::open();
    seed_account(&conn_local, "acc-split", "现金", "cash", "CNY", 0);
    let err_local = create_transaction_internal(
        &conn_local,
        make_input("acc-split", TransactionKind::Split, 0, "2026-05-04"),
    )
    .unwrap_err();
    assert_eq!(
        coded_of(err_local).0,
        "trade.split-instrument-required",
        "Local 的 split 应进投资域守卫而非 kind 拒绝"
    );

    // Replay（旧载荷，缺份额调整字段）：kind 防御臂码化拒绝，引擎挂起承接。
    let conn_replay = test_support::open();
    seed_account(&conn_replay, "acc-split", "现金", "cash", "CNY", 0);
    let err_replay = replay_command(
        &conn_replay,
        &TransactionCommand::Create {
            id: "sync-split-1".into(),
            row: carried_row(TransactionKind::Split, "acc-split", 0),
            investment: None,
            split: None,
            convert: None,
        },
    )
    .unwrap_err();
    assert_eq!(
        coded_of(err_replay).0,
        "transaction.split-fields-missing",
        "缺份额调整字段的旧 split 载荷应码化挂起（不静默落半套副作用）"
    );

    // Replay（有字段但非投资账户）：本地同码守卫拒绝，重放不得绕开不变量。
    let err_replay = replay_command(
        &conn_replay,
        &TransactionCommand::Create {
            id: "sync-split-2".into(),
            row: carried_row(TransactionKind::Split, "acc-split", 0),
            investment: None,
            split: Some(crate::transaction::command::SplitCommandFields {
                instrument_id: "inst-missing".into(),
                delta_quantity: 5.0,
                final_quantity: 5.0,
                total_cost_cents: 0,
            }),
            convert: None,
        },
    )
    .unwrap_err();
    assert_eq!(
        coded_of(err_replay).0,
        "trade.split-instrument-not-found",
        "Replay 的 split 应进投资域重放守卫而非 kind 拒绝"
    );
}
