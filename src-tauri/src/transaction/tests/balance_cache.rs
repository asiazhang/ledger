//! 余额与净资产持久化缓存一致性测试（issue #491 / ADR-0067）。
//!
//! 不变量：任何写入入口落库后，`account_balance_cache` 必须与实时计算
//! （`compute_balance` / `account_flow_expr` 单一口径矩阵）逐账户一致；
//! 违约读路径报码化错误引导审计，净资产读探针指纹失配自愈。
//!
//! 覆盖写入入口（全部收敛 Writer 接缝与行为层删除，ADR-0033）：
//! 创建（income/expense/transfer/refund/buy/sell）、修改（含跨账户移动）、
//! 删除、批量导入、Writer 直写（定时引擎例外接缝）、余额调整、账户增删；
//! 五出口回归、缓存缺失码化错误、净资产探针（回填/命中/自愈）与审计修复。

use rusqlite::{Connection, params};

use super::super::*;
use super::common::{insert_account, make_buy_input, make_input, setup, setup_investment_account};
use crate::accounts::balance::{compute_all_balances_with_visibility, compute_balance};
use crate::accounts::{
    AccountBalanceAdjustInput, AccountInput, AccountType, adjust_account_balance,
    audit_balance_cache, create_account, delete_account, list_account_balances_for_api,
    list_account_balances_with_visibility as domain_list_balances,
};
use crate::dashboard::query_dashboard_overview;
use crate::test_support::assert_balance_cache_matches_realtime;
use crate::transaction::TransactionBatch;
use crate::transaction::amount::TransactionKind;

/// 缓存行种子（模拟 V017 迁移回填）：测试脚手架用裸 SQL 建账户（绕过
/// `create_account` 域钩子），生产语义下存量账户由迁移一次性回填，此处
/// 用同一整体重算接缝补齐，保证「每账户必有缓存行」不变量成立。
fn backfill_scaffold_account(conn: &Connection, account_id: &str) {
    crate::accounts::balance::refresh_account_balances(conn, &[account_id]).unwrap();
}

// 一致性对拍断言上收共享断言库（issue #751 / ADR-0084 决策 6）：本地断言体删除，
// 改调 test_support::assert_balance_cache_matches_realtime（全库唯一维护点）。

// ---------------------------------------------------------------------------
// 创建入口参数化：六 kind 全覆盖
// ---------------------------------------------------------------------------

/// 创建各 kind 交易后缓存与实时一致：
/// income/refund 为 +、expense 为 −、transfer 双侧、buy/sell 投资路径。
#[test]
fn create_all_kinds_keep_cache_consistent() {
    let conn = setup();
    insert_account(&conn, "acc-cash", "现金", "cash", "CNY");
    setup_investment_account(&conn, "acc-inv", "inst-k");
    backfill_scaffold_account(&conn, "acc-cash");
    backfill_scaffold_account(&conn, "acc-inv");

    // income +
    create_transaction_internal(
        &conn,
        make_input("acc-cash", TransactionKind::Income, 10000, "2026-01-01"),
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);

    // expense −
    create_transaction_internal(
        &conn,
        make_input("acc-cash", TransactionKind::Expense, 2000, "2026-01-02"),
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);

    // transfer：转出侧 −、转入侧 +
    create_account(
        &conn,
        AccountInput {
            name: "目标".into(),
            kind: AccountType::Bank,
            currency_code: "CNY".into(),
            initial_balance_cents: None,
        },
    )
    .unwrap();
    let to_id: String = conn
        .query_row("SELECT id FROM accounts WHERE name='目标'", [], |r| {
            r.get(0)
        })
        .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            to_account_id: Some(to_id.clone()),
            ..make_input("acc-cash", TransactionKind::Transfer, 500, "2026-01-03")
        },
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);

    // refund +（挂真实 expense）
    let expense_id = create_transaction_internal(
        &conn,
        make_input("acc-cash", TransactionKind::Expense, 300, "2026-01-04"),
    )
    .unwrap()
    .id;
    create_transaction_internal(
        &conn,
        TransactionInput {
            refund_of_transaction_id: Some(expense_id),
            ..make_input("acc-cash", TransactionKind::Refund, 100, "2026-01-05")
        },
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);

    // buy − / sell +（投资账户）
    create_transaction_internal(&conn, make_buy_input("acc-inv", "inst-k", 2.0, 100000, 100))
        .unwrap();
    assert_balance_cache_matches_realtime(&conn);
    let mut sell = make_buy_input("acc-inv", "inst-k", 1.0, 110000, 0);
    sell.kind = TransactionKind::Sell;
    sell.date = "2026-01-06".into();
    create_transaction_internal(&conn, sell).unwrap();
    assert_balance_cache_matches_realtime(&conn);
}

// ---------------------------------------------------------------------------
// 修改 / 删除入口：旧∪新账户引用并集重算
// ---------------------------------------------------------------------------

/// 修改把交易移到别的账户：旧∪新三账户缓存都要与实时一致（update_row 并集重算）。
#[test]
fn update_cross_account_refreshes_old_and_new_union() {
    let conn = setup();
    insert_account(&conn, "acc-u1", "甲", "cash", "CNY");
    insert_account(&conn, "acc-u2", "乙", "cash", "CNY");
    insert_account(&conn, "acc-u3", "丙", "cash", "CNY");
    for id in ["acc-u1", "acc-u2", "acc-u3"] {
        backfill_scaffold_account(&conn, id);
    }

    let id = create_transaction_internal(
        &conn,
        make_input("acc-u1", TransactionKind::Expense, 400, "2026-01-01"),
    )
    .unwrap()
    .id;
    assert_balance_cache_matches_realtime(&conn);

    // expense(acc-u1) → transfer(acc-u1→acc-u2)：两侧并集。
    update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            to_account_id: Some("acc-u2".into()),
            ..make_input("acc-u1", TransactionKind::Transfer, 400, "2026-01-01")
        },
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);

    // transfer → expense(acc-u3)：旧引用（u1/u2）与新引用（u3）并集。
    update_transaction_internal(
        &conn,
        &id,
        make_input("acc-u3", TransactionKind::Expense, 900, "2026-01-02"),
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);
}

/// 删除 transfer 交易：两侧账户缓存回到初始（delete_within_transaction 重算）。
#[test]
fn delete_transfer_restores_both_sides() {
    let conn = setup();
    insert_account(&conn, "acc-d1", "甲", "cash", "CNY");
    insert_account(&conn, "acc-d2", "乙", "cash", "CNY");
    for id in ["acc-d1", "acc-d2"] {
        backfill_scaffold_account(&conn, id);
    }

    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            to_account_id: Some("acc-d2".into()),
            ..make_input("acc-d1", TransactionKind::Transfer, 700, "2026-01-01")
        },
    )
    .unwrap()
    .id;
    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(compute_balance(&conn, "acc-d1").unwrap(), -700);
    assert_eq!(compute_balance(&conn, "acc-d2").unwrap(), 700);

    delete_transaction_internal(&conn, &id).unwrap();
    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(compute_balance(&conn, "acc-d1").unwrap(), 0);
    assert_eq!(compute_balance(&conn, "acc-d2").unwrap(), 0);
}

// ---------------------------------------------------------------------------
// 批量导入与 Writer 直写（定时引擎例外接缝）
// ---------------------------------------------------------------------------

/// 批量导入（HTTP 导入路径，`TransactionBatch::run`）落库后缓存一致。
#[test]
fn batch_import_keeps_cache_consistent() {
    let conn = setup();
    insert_account(&conn, "acc-batch", "现金", "cash", "CNY");
    backfill_scaffold_account(&conn, "acc-batch");

    TransactionBatch::run(
        &conn,
        vec![
            make_input("acc-batch", TransactionKind::Income, 8000, "2026-02-01"),
            make_input("acc-batch", TransactionKind::Expense, 2500, "2026-02-02"),
        ],
        true,
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);
}

/// Writer 接缝直写（定时交易引擎例外接缝，ADR-0033 登记的唯一绕行为层入口）：
/// 引擎 `execute_within_transaction` 直调 `writer::insert_row`，缓存刷新挂本接缝。
#[test]
fn writer_insert_row_direct_seam_refreshes_cache() {
    let conn = setup();
    insert_account(&conn, "acc-eng", "现金", "cash", "CNY");
    backfill_scaffold_account(&conn, "acc-eng");

    let input = writer::Input {
        kind: TransactionKind::Income,
        amount_cents: 1200,
        currency_code: "CNY".into(),
        account_id: "acc-eng".into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        existing_merchant_id: None,
        policy_id: None,
        existing_policy_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-03-01".into(),
    };
    let row = writer::normalize(&conn, &input).unwrap();
    writer::insert_row(&conn, &row).unwrap();
    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(compute_balance(&conn, "acc-eng").unwrap(), 1200);
}

// ---------------------------------------------------------------------------
// 账户入口：创建建行 / 删除保持一致 / 余额调整（读缓存取数）
// ---------------------------------------------------------------------------

/// 创建账户即建缓存行（初始余额 + 零流水）；软删后缓存仍与实时一致。
#[test]
fn account_create_and_delete_maintain_cache_rows() {
    let conn = setup();
    let id = create_account(
        &conn,
        AccountInput {
            name: "钱包".into(),
            kind: AccountType::Ewallet,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(6800),
        },
    )
    .unwrap();
    let cached: i64 = conn
        .query_row(
            "SELECT balance_cents FROM account_balance_cache WHERE account_id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cached, 6800, "创建账户应即建缓存行（初始余额）");

    delete_account(&conn, &id).unwrap();
    assert_balance_cache_matches_realtime(&conn);
}

/// 余额调整（ADR-0026）取数走缓存（五出口之一）：调整后余额精确等于目标值，
/// 缓存与实时一致；正向（黑洞转入）与反向（转出至黑洞）两个方向。
#[test]
fn adjust_balance_targets_exact_value_via_cache() {
    let conn = setup();
    let id = create_account(
        &conn,
        AccountInput {
            name: "工资卡".into(),
            kind: AccountType::Bank,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(1000),
        },
    )
    .unwrap();

    // 0 触发黑洞新建：1000 → 5600。
    let adjust = |target: i64| {
        adjust_account_balance(
            &conn,
            &id,
            &AccountBalanceAdjustInput {
                target_balance_cents: target,
                date: "2026-04-01".into(),
                note: None,
            },
        )
        .unwrap()
    };
    adjust(5600);
    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(compute_balance(&conn, &id).unwrap(), 5600);

    // 反向：5600 → 4300。
    adjust(4300);
    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(compute_balance(&conn, &id).unwrap(), 4300);

    // 黑洞账户自身缓存也应一致（转账对方）。
    assert_balance_cache_matches_realtime(&conn);
}

// ---------------------------------------------------------------------------
// 五出口回归：切缓存后读值与实时一致
// ---------------------------------------------------------------------------

/// 五出口回归：账户余额清单（UI）、含黑洞口径（AI/HTTP）、dashboard 余额腿、
/// 余额调整取数（cached_balance）、财务自由度分子读到的值都与实时计算一致。
/// （HTTP 列表经同一 `list_account_balances_with_visibility` 入口，结构上同源，
/// 不重复铺世界。）
#[test]
fn five_outlets_return_realtime_consistent_values() {
    let conn = setup();
    insert_account(&conn, "acc-o1", "现金", "cash", "CNY");
    setup_investment_account(&conn, "acc-o2", "inst-o");
    backfill_scaffold_account(&conn, "acc-o1");
    backfill_scaffold_account(&conn, "acc-o2");
    create_transaction_internal(
        &conn,
        make_input("acc-o1", TransactionKind::Income, 1000000, "2026-05-01"),
    )
    .unwrap();
    // 转入投资账户 600000，买入花费 4000（万分位刻度：1.0×400000/100），
    // 投资账户留现金 596000（财务自由度现金腿）。
    create_transaction_internal(
        &conn,
        TransactionInput {
            to_account_id: Some("acc-o2".into()),
            ..make_input("acc-o1", TransactionKind::Transfer, 600000, "2026-05-02")
        },
    )
    .unwrap();
    create_transaction_internal(&conn, make_buy_input("acc-o2", "inst-o", 1.0, 400000, 0)).unwrap();

    // 出口 1/2：UI 清单（不含黑洞）与 AI/HTTP 清单（含黑洞）逐行 == 实时。
    let realtime_visible = compute_all_balances_with_visibility(&conn, false).unwrap();
    for ab in domain_list_balances(&conn, false).unwrap() {
        assert_eq!(
            Some(ab.balance_cents),
            realtime_visible.get(&ab.account.id).copied(),
            "UI 清单出口应与实时一致: {}",
            ab.account.id
        );
    }
    let realtime_all = compute_all_balances_with_visibility(&conn, true).unwrap();
    for ab in list_account_balances_for_api(&conn).unwrap() {
        assert_eq!(
            Some(ab.balance_cents),
            realtime_all.get(&ab.account.id).copied(),
            "HTTP/AI 清单出口应与实时一致: {}",
            ab.account.id
        );
    }

    // 出口 3：dashboard 余额腿（非投资账户合计，1:1 汇率）。
    let overview = query_dashboard_overview(&conn).unwrap();
    let expected_accounts_sum: i64 = realtime_visible
        .iter()
        .filter(|(id, _)| {
            *id != "acc-o2" // 投资账户余额不计入（市值腿承载，避免重复计算）
        })
        .map(|(_, v)| v)
        .sum();
    assert_eq!(
        overview.accounts_balance_cents, expected_accounts_sum,
        "dashboard 余额腿应与实时合计一致"
    );
    assert_eq!(
        overview.net_worth_cents,
        overview.accounts_balance_cents
            + overview.holdings_market_value_cents
            + overview.physical_assets_value_cents
    );

    // 出口 4：余额调整取数（cached_balance）与实时一致。
    assert_eq!(
        crate::accounts::balance::cached_balance(&conn, "acc-o1").unwrap(),
        compute_balance(&conn, "acc-o1").unwrap()
    );

    // 出口 5：财务自由度分子（投资账户现金腿 + 持仓市值，经同一缓存入口取数）。
    // 现金腿 = acc-o2 余额 596000（600000 转入 − 4000 买入，1:1 汇率）；
    // 持仓未录价按空值语义跳过（0）。分母预算为空 → 分子仍须与实时口径一致。
    let freedom = crate::investment::query_financial_freedom(&conn).unwrap();
    assert_eq!(
        freedom.numerator_cents, 596000,
        "财务自由度分子应与实时口径一致（投资账户现金腿读缓存）"
    );
}

// ---------------------------------------------------------------------------
// 缓存缺失：码化错误引导审计（不静默回退）
// ---------------------------------------------------------------------------

/// 缓存行被外部破坏（缺失）时，读路径报码化错误（balance.cache-row-missing）
/// 引导审计修复，不静默回退实时计算。
#[test]
fn missing_cache_row_raises_coded_error() {
    let conn = setup();
    insert_account(&conn, "acc-miss", "现金", "cash", "CNY");
    backfill_scaffold_account(&conn, "acc-miss");

    conn.execute(
        "DELETE FROM account_balance_cache WHERE account_id='acc-miss'",
        [],
    )
    .unwrap();
    let err = domain_list_balances(&conn, false).unwrap_err();
    assert!(
        err.to_string().contains("余额缓存审计"),
        "缺失缓存行应引导审计修复，实际: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 净资产读探针：首次回填 → 命中直读 → 源变更自愈
// ---------------------------------------------------------------------------

/// 探针三态：首读回填缓存行；命中直接返回缓存（不再实时聚合）；
/// 源表变更（指纹失配）后重算自愈。
#[test]
fn net_worth_probe_backfills_hits_and_self_heals() {
    let conn = setup();
    insert_account(&conn, "acc-p1", "现金", "cash", "CNY");
    backfill_scaffold_account(&conn, "acc-p1");
    create_transaction_internal(
        &conn,
        make_input("acc-p1", TransactionKind::Income, 10000, "2026-06-01"),
    )
    .unwrap();

    // 首读：回填（迁移不回填净资产缓存，首读即自愈完成首次回填）。
    let first = query_dashboard_overview(&conn).unwrap();
    assert_eq!(first.net_worth_cents, 10000);
    let fp = crate::dashboard::net_worth::current_fingerprint(&conn).unwrap();
    let cached = crate::dashboard::net_worth::read_valid(&conn, &fp).unwrap();
    assert!(cached.is_some(), "首读后应有指纹匹配的缓存行");
    assert_eq!(cached.unwrap().net_worth_cents, 10000);

    // 命中：篡改缓存值（指纹不变）→ 仍读出被篡改值，证明走的是缓存非重算。
    conn.execute(
        "UPDATE net_worth_cache SET net_worth_cents = 777 WHERE id = 1",
        [],
    )
    .unwrap();
    let hit = query_dashboard_overview(&conn).unwrap();
    assert_eq!(hit.net_worth_cents, 777, "指纹命中应直读缓存终值");

    // 源变更：再写一笔交易 → 指纹失配 → 重算自愈（777 被正确值覆盖）。
    create_transaction_internal(
        &conn,
        make_input("acc-p1", TransactionKind::Expense, 2500, "2026-06-02"),
    )
    .unwrap();
    let healed = query_dashboard_overview(&conn).unwrap();
    assert_eq!(
        healed.net_worth_cents, 7500,
        "源变更后应重算自愈，不再返回陈旧缓存"
    );
}

// ---------------------------------------------------------------------------
// 手动审计：污染 → 报告差异 → 修复；复检干净
// ---------------------------------------------------------------------------

/// 审计命令：污染（值漂移 + 缓存行缺失）→ 报告全部差异 → 修复；
/// 复检干净（repaired=false、无 drift）。
#[test]
fn audit_polluted_cache_reports_then_repairs() {
    let conn = setup();
    insert_account(&conn, "acc-a1", "甲", "cash", "CNY");
    insert_account(&conn, "acc-a2", "乙", "cash", "CNY");
    for id in ["acc-a1", "acc-a2"] {
        backfill_scaffold_account(&conn, id);
    }
    create_transaction_internal(
        &conn,
        TransactionInput {
            to_account_id: Some("acc-a2".into()),
            ..make_input("acc-a1", TransactionKind::Transfer, 1500, "2026-07-01")
        },
    )
    .unwrap();

    // 污染两种形态：漂移 + 缺行。
    conn.execute(
        "UPDATE account_balance_cache SET balance_cents = balance_cents + 100 WHERE account_id='acc-a1'",
        [],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM account_balance_cache WHERE account_id='acc-a2'",
        [],
    )
    .unwrap();

    let report = audit_balance_cache(&conn).unwrap();
    assert!(report.repaired, "存在差异应触发修复");
    // 2 个脚手架账户 + V004 种子默认账户 2 个（审计巡检全部未删除账户）。
    assert_eq!(report.accounts_checked, 4);
    assert_eq!(report.drifts.len(), 2, "漂移与缺行都应入报告");
    let missing = report
        .drifts
        .iter()
        .find(|d| d.account_id == "acc-a2")
        .expect("缺行账户应入差异报告");
    assert_eq!(missing.cached_cents, None, "缓存缺失记 None");
    assert_eq!(missing.actual_cents, 1500);

    // 修复后：缓存 == 实时，复检干净。
    assert_balance_cache_matches_realtime(&conn);
    let recheck = audit_balance_cache(&conn).unwrap();
    assert!(!recheck.repaired, "修复后复检应无差异");
    assert!(recheck.drifts.is_empty());
}
