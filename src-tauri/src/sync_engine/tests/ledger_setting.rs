//! 账本级设置（issue #858 / ADR-0091 决策 3/9）：本位币基准的 op 产出、并发
//! LWW 合并与同步后折算确定性。
//!
//! 双端场景 = 同进程两个引擎实例 + 内存假 Transport（[`super::common`] 同纪律）；
//! 设置写入走公开写入口（`currencies::set_base_currency`），折算确定性对准
//! Amount 接缝在两端的实际输出。

use super::super::{DomainCommand, OpOutcome, read_ops};
use super::common::{make_expense, read_transaction, seed_device, wire_in, wire_out};
use crate::currencies::{current_base_currency, set_base_currency};
use crate::test_support::{self, seed_account, seed_exchange_rate};
use crate::transaction::behavior;

/// 并发修改本位币基准：按全序取序末者（LWW），两端折算口径一致不分叉。
///
/// 固定 DeviceId（dev-a < dev-b）：两端并发各设一次基准（A→USD、B→EUR），
/// 全序判 B 的设置为序末者 → 两端收敛 EUR；输者（A 的设置）落日志可追溯、
/// 不执行。
#[test]
fn concurrent_base_currency_edits_converge_to_order_last() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    // 两端并发各设一次基准（同实体 ledger_setting/base_currency）。
    set_base_currency(&conn_a, "USD").unwrap();
    set_base_currency(&conn_b, "EUR").unwrap();

    wire_in(&conn_b, &wire_out(&conn_a));
    wire_in(&conn_a, &wire_out(&conn_b));

    // 两端收敛到序末者（B 的 EUR）：折算口径唯一，不产生分叉。
    assert_eq!(current_base_currency(&conn_a).unwrap(), "EUR");
    assert_eq!(current_base_currency(&conn_b).unwrap(), "EUR");

    // 两端日志一致；输者（A 的 USD 设置）仍在日志中可审计。
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
    assert!(
        read_ops(&conn_a).unwrap().iter().any(|op| matches!(
            &op.command,
            DomainCommand::LedgerSetting(
                crate::currencies::LedgerSettingCommand::SetBaseCurrency { code }
            ) if code == "USD"
        )),
        "输者的设置 op 仍可审计"
    );
}

/// 并发修改的逐条报告：序末者 Applied，输者 Superseded（与交易同实体 LWW 同协议）。
#[test]
fn concurrent_base_currency_reports_lww_outcomes() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    set_base_currency(&conn_a, "USD").unwrap();
    set_base_currency(&conn_b, "EUR").unwrap();

    let reports_on_b = wire_in(&conn_b, &wire_out(&conn_a));
    let reports_on_a = wire_in(&conn_a, &wire_out(&conn_b));

    // A 的设置 (1, dev-a) 在 B 端被序末者 (1, dev-b) 压制 → Superseded；
    // B 的设置在 A 端为序末者 → Applied。
    assert!(
        reports_on_b
            .iter()
            .any(|r| r.outcome == OpOutcome::Superseded),
        "A 的设置在 B 端应为 LWW 输者：{reports_on_b:?}"
    );
    assert!(
        reports_on_a.iter().any(|r| r.outcome == OpOutcome::Applied),
        "B 的设置在 A 端应为序末者执行：{reports_on_a:?}"
    );
}

/// 同步后折算确定性：基准设置收敛后，两端各记同一笔外币交易，折算按同一
/// 基准产出同一 native 口径（Amount 接缝实际输出相等）。
#[test]
fn conversion_is_deterministic_across_devices_after_sync() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    // A 端把基准改为 USD，同步到 B：折算基准全设备一致。
    set_base_currency(&conn_a, "USD").unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(current_base_currency(&conn_b).unwrap(), "USD");

    // 两端本地种入同一汇率（EUR→USD，行情数据本就端侧各自持有）后各记一笔
    // 同额 EUR 支出：native 均按 USD 基准折算，两端一致。
    seed_exchange_rate(&conn_a, "EUR", "USD", 1.1);
    seed_exchange_rate(&conn_b, "EUR", "USD", 1.1);
    let mut input = make_expense("acc-1", 10000, "外币支出");
    input.currency_code = "EUR".into();
    let id_a = behavior::create(&conn_a, input.clone()).unwrap().id;
    let id_b = behavior::create(&conn_b, input).unwrap().id;

    let row_a = read_transaction(&conn_a, &id_a).unwrap();
    let row_b = read_transaction(&conn_b, &id_b).unwrap();
    // 100 EUR × 1.1 = 110 USD（四舍五入到分）：两端同一折算口径，且确为 USD
    // 基准（若基准仍是 CNY，native 将按 CNY 汇率折出）。
    assert_eq!(row_a.amount_native_cents, 11000);
    assert_eq!(row_b.amount_native_cents, 11000);
}

/// 设置命令的重放幂等：同一 op 重复投递不产生第二次效果（Skipped）。
#[test]
fn base_currency_op_replay_is_idempotent() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_b, "dev-b");

    set_base_currency(&conn_a, "USD").unwrap();
    let wire = wire_out(&conn_a);
    wire_in(&conn_b, &wire);
    assert_eq!(current_base_currency(&conn_b).unwrap(), "USD");

    // 同一批 op 重投：全部 Skipped，基准不变。
    let reports = wire_in(&conn_b, &wire);
    assert!(
        reports.iter().all(|r| r.outcome == OpOutcome::Skipped),
        "重复投递应全部跳过：{reports:?}"
    );
    assert_eq!(current_base_currency(&conn_b).unwrap(), "USD");
}
