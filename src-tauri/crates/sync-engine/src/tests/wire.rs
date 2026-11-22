use std::collections::BTreeSet;

use super::super::{DomainCommand, SyncOp};
use ledger_transaction::TransactionCommand;

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

fn sample(entity: &str) -> DomainCommand {
    match entity {
        "transaction" => {
            DomainCommand::Transaction(TransactionCommand::Delete { id: "t-1".into() })
        }
        "scheduled" => {
            DomainCommand::Scheduled(ledger_scheduled::ScheduledCommand::ExpandOccurrences {
                plan_id: "p-1".into(),
            })
        }
        "ledger_setting" => {
            DomainCommand::LedgerSetting(ledger_currencies::LedgerSettingCommand::SetBaseCurrency {
                code: "CNY".into(),
            })
        }
        "account" => {
            DomainCommand::Account(ledger_accounts::AccountCommand::Delete { id: "a-1".into() })
        }
        "category" => {
            DomainCommand::Category(ledger_categories::CategoryCommand::Delete { id: "c-1".into() })
        }
        "merchant" => {
            DomainCommand::Merchant(ledger_merchants::MerchantCommand::Delete { id: "m-1".into() })
        }
        "budget" => {
            DomainCommand::Budget(ledger_budget::BudgetCommand::Delete { id: "b-1".into() })
        }
        "policy" => {
            DomainCommand::Policy(ledger_policy::PolicyCommand::Delete { id: "po-1".into() })
        }
        "insurer" => {
            DomainCommand::Insurer(ledger_policy::InsurerCommand::Delete { id: "in-1".into() })
        }
        "item" => DomainCommand::Item(ledger_item::ItemCommand::Delete { id: "it-1".into() }),
        "physical_asset" => {
            DomainCommand::PhysicalAsset(ledger_physical_asset::PhysicalAssetCommand::Delete {
                id: "pa-1".into(),
            })
        }
        "goal" => DomainCommand::Goal(ledger_savings_goal::SavingsGoalCommand::Delete {
            id: "g-1".into(),
        }),
        "instrument" => DomainCommand::Instrument(ledger_investment::InstrumentCommand::Delete {
            id: "i-1".into(),
        }),
        "exchange_rate" => {
            DomainCommand::ExchangeRate(ledger_investment::ExchangeRateCommand::Upsert {
                id: "er-1".into(),
                base_code: "EUR".into(),
                quote_code: "CNY".into(),
                rate: 8.0,
                priced_at: "2026-01-10".into(),
                source: None,
            })
        }
        "price" => DomainCommand::Price(ledger_investment::PriceCommand::MarketPrice {
            instrument_id: "i-1".into(),
            price_cents: 100,
            currency_code: "CNY".into(),
            priced_at: "2026-01-10".into(),
            source: None,
        }),
        other => panic!("DomainCommand 变体未登记重放样本：{other}（门 a，ADR-0101）"),
    }
}

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

#[test]
fn wire_entity_tag_matches_registry_binding_for_every_variant() {
    let tags = serde_entity_tags();
    assert!(
        tags.len() >= 15,
        "serde 自报变体清单不齐（{tags:?}）——门 a 的完备性基准失效"
    );
    let mut seen = BTreeSet::new();
    for tag in &tags {
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
