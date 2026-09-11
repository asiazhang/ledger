//! op 信封 wire 形态：序列化往返稳定（通道上的搬运形态，#859 Transport 接线
//! 的前提）；实体判别键与 serde tag 同源（门 a 双源门，ADR-0101 决策 4a）。

use std::collections::BTreeSet;

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

/// 门 (a) 样本表（ADR-0101 决策 4a）：每个可重放变体一条最小样本，键 = serde
/// entity tag。`other => panic!` 是「新增变体漏补样本」的运行期红：tag 全集由
/// [`serde_entity_tags`]（serde derive 依 enum 生成，与样本表相互独立的第二来源）
/// 给出，漏挂样本的新变体一到此即 panic 并列出变体名。
fn sample(entity: &str) -> DomainCommand {
    match entity {
        "transaction" => {
            DomainCommand::Transaction(TransactionCommand::Delete { id: "t-1".into() })
        }
        "scheduled" => DomainCommand::Scheduled(
            crate::scheduled_transactions::ScheduledCommand::ExpandOccurrences {
                plan_id: "p-1".into(),
            },
        ),
        "ledger_setting" => {
            DomainCommand::LedgerSetting(crate::currencies::LedgerSettingCommand::SetBaseCurrency {
                code: "CNY".into(),
            })
        }
        "account" => {
            DomainCommand::Account(crate::accounts::AccountCommand::Delete { id: "a-1".into() })
        }
        "category" => {
            DomainCommand::Category(crate::categories::CategoryCommand::Delete { id: "c-1".into() })
        }
        "merchant" => {
            DomainCommand::Merchant(crate::merchants::MerchantCommand::Delete { id: "m-1".into() })
        }
        "budget" => {
            DomainCommand::Budget(crate::budget::BudgetCommand::Delete { id: "b-1".into() })
        }
        "policy" => {
            DomainCommand::Policy(crate::policy::PolicyCommand::Delete { id: "po-1".into() })
        }
        "insurer" => {
            DomainCommand::Insurer(crate::policy::InsurerCommand::Delete { id: "in-1".into() })
        }
        "item" => DomainCommand::Item(crate::item::ItemCommand::Delete { id: "it-1".into() }),
        "physical_asset" => {
            DomainCommand::PhysicalAsset(crate::physical_asset::PhysicalAssetCommand::Delete {
                id: "pa-1".into(),
            })
        }
        "instrument" => DomainCommand::Instrument(crate::investment::InstrumentCommand::Delete {
            id: "i-1".into(),
        }),
        "exchange_rate" => {
            DomainCommand::ExchangeRate(crate::investment::ExchangeRateCommand::Upsert {
                id: "er-1".into(),
                base_code: "EUR".into(),
                quote_code: "CNY".into(),
                rate: 8.0,
                priced_at: "2026-01-10".into(),
                source: None,
            })
        }
        "price" => DomainCommand::Price(crate::investment::PriceCommand::MarketPrice {
            instrument_id: "i-1".into(),
            price_cents: 100,
            currency_code: "CNY".into(),
            priced_at: "2026-01-10".into(),
            source: None,
        }),
        other => panic!("DomainCommand 变体未登记重放样本：{other}（门 a，ADR-0101）"),
    }
}

/// serde derive 自报的实体 tag 全集（第二来源）：以未知 tag 反序列化
/// `DomainCommand`，从 serde 的错误信息取回「expected one of …」清单——该清单由
/// `#[derive(Deserialize)]` 依 enum 变体生成，与样本表相互独立。格式漂移即在此
/// fail loud（非静默通过）。
fn serde_entity_tags() -> Vec<String> {
    let error = serde_json::from_str::<DomainCommand>(r#"{"entity":"__probe__","payload":{}}"#)
        .expect_err("探针 tag 不应可反序列化");
    let message = error.to_string();
    let listed = message
        .split("expected one of")
        .nth(1)
        .unwrap_or_else(|| panic!("serde 未列出 DomainCommand 变体清单：{message}"));
    listed
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// 门 (a)：每个变体的 serde entity tag == 注册表组装的裁决域标签（`subject().0`），
/// 且样本表覆盖 serde 自报的全部变体——新增变体漏补样本即运行期红并列出变体名。
#[test]
fn wire_entity_tag_matches_registry_binding_for_every_variant() {
    let tags = serde_entity_tags();
    assert!(
        tags.len() >= 14,
        "serde 自报变体清单不齐（{tags:?}）——门 a 的完备性基准失效"
    );
    let mut seen = BTreeSet::new();
    for tag in &tags {
        // 样本表缺失该变体 → 兜底 panic 臂列出变体名（门 a 运行期红）。
        let command = sample(tag);
        let json = serde_json::to_value(&command).unwrap();
        let wire_tag = json
            .get("entity")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("样本 {tag} 序列化后无 entity tag：{json}"));
        assert_eq!(wire_tag, tag, "样本 {tag} 的 serde tag 与样本表键漂移");
        assert_eq!(
            wire_tag,
            command.subject().0,
            "变体 {tag}：serde tag 与注册表 ENTITY 双源漂移"
        );
        seen.insert(wire_tag.to_string());
    }
    assert_eq!(
        seen.len(),
        tags.len(),
        "样本表 tag 去重后与 serde 清单不齐：{seen:?}"
    );
}
