//! op 信封 wire 形态：序列化往返稳定（通道上的搬运形态，#859 Transport 接线
//! 的前提）；实体判别键与 serde tag 同源。

use super::super::{DomainCommand, SyncOp};
use crate::transaction::TransactionCommand;

#[test]
fn sync_op_serialization_round_trip() {
    let op = SyncOp {
        op_id: "op-1".into(),
        device_id: "device-a".into(),
        clock: 7,
        schema_version: 20,
        command: DomainCommand::Transaction(TransactionCommand::Delete { id: "t-1".into() }),
    };

    let json = serde_json::to_string(&op).unwrap();
    let back: SyncOp = serde_json::from_str(&json).unwrap();
    assert_eq!(back, op, "序列化往返无损");
}

#[test]
fn entity_key_matches_serde_tag() {
    let command = DomainCommand::Transaction(crate::transaction::TransactionCommand::Delete {
        id: "t-1".into(),
    });
    let json = serde_json::to_string(&command).unwrap();
    assert!(
        json.contains("\"entity\":\"transaction\""),
        "entity 列取值与 serde tag 同源，实际: {json}"
    );
    assert_eq!(command.entity(), "transaction");
}
