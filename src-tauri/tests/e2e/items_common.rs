use ledger_item::ItemInput;

pub fn build_input(name: &str, date: String, cost_cents: i64, currency: &str) -> ItemInput {
    ItemInput {
        name: name.into(),
        purchase_date: date,
        total_cost_cents: cost_cents,
        currency_code: currency.into(),
        note: None,
        purchase_transaction_id: None,
    }
}
