use serde::{Deserialize, Serialize};

use crate::amount::TransactionKind;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionListFilter {
    pub from: Option<String>,
    pub to: Option<String>,
    pub account_id: Option<String>,
    pub involving_account_id: Option<String>,
    pub merchant_id: Option<String>,
    pub category_id: Option<String>,
    pub uncategorized_only: Option<bool>,
    pub instrument_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_kind_set"
    )]
    pub kinds: Option<Vec<TransactionKind>>,
    pub limit: Option<i64>,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    pub hide_investment_related: Option<bool>,
}

fn deserialize_kind_set<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<TransactionKind>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct KindSetVisitor;

    impl<'de> serde::de::Visitor<'de> for KindSetVisitor {
        type Value = Option<Vec<TransactionKind>>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("kind 字符串数组或逗号分隔字符串")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
            let mut kinds = Vec::new();
            for s in v.split(',') {
                match TransactionKind::parse(s.trim()) {
                    Ok(k) => kinds.push(k),
                    Err(e) => return Err(serde::de::Error::custom(e.to_string())),
                }
            }
            Ok(Some(kinds))
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut kinds = Vec::new();
            while let Some(k) = seq.next_element::<TransactionKind>()? {
                kinds.push(k);
            }
            Ok(Some(kinds))
        }

        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }
    }

    deserializer.deserialize_any(KindSetVisitor)
}
