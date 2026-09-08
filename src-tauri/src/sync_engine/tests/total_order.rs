//! TotalOrder（跨端全序，ADR-0091 决策 4）：按 (逻辑时钟, DeviceId) 排出
//! 确定全序——各端对同一批 op 排出唯一一致的顺序，同钟以 DeviceId tiebreak。

use super::super::{DomainCommand, SyncOp, total_order};
use crate::transaction::TransactionCommand;

/// 构造一个任意 clock/device 的 op（排序判据不需要真实落库）。
fn op_with(clock: i64, device_id: &str) -> SyncOp {
    SyncOp {
        op_id: format!("op-{device_id}-{clock}"),
        device_id: device_id.to_string(),
        clock,
        schema_version: 20,
        command: DomainCommand::Transaction(TransactionCommand::Delete { id: "t-1".into() }),
    }
}

#[test]
fn sorts_by_clock_then_device_id() {
    let mut ops = vec![
        op_with(3, "device-b"),
        op_with(1, "device-b"),
        op_with(2, "device-a"),
        op_with(1, "device-a"),
    ];
    total_order(&mut ops);
    let keys: Vec<(i64, &str)> = ops
        .iter()
        .map(|o| (o.clock, o.device_id.as_str()))
        .collect();
    assert_eq!(
        keys,
        vec![
            (1, "device-a"),
            (1, "device-b"),
            (2, "device-a"),
            (3, "device-b")
        ],
        "先按逻辑时钟、同钟按 DeviceId 字典序"
    );
}

#[test]
fn order_is_deterministic_regardless_of_input_order() {
    let devices = ["device-a", "device-b", "device-c"];
    let mut forward: Vec<SyncOp> = devices
        .iter()
        .flat_map(|d| [op_with(1, d), op_with(2, d)])
        .collect();
    let mut reversed = forward.clone();
    reversed.reverse();

    total_order(&mut forward);
    total_order(&mut reversed);
    assert_eq!(
        forward.iter().map(|o| o.op_id.as_str()).collect::<Vec<_>>(),
        reversed
            .iter()
            .map(|o| o.op_id.as_str())
            .collect::<Vec<_>>(),
        "任意输入顺序排出同一全序"
    );
}
