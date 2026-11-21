use ledger_accounts as accounts;
use ledger_currencies as currencies;
use ledger_investment as investment;
use ledger_item as item;
use ledger_merchants as merchants;
use ledger_policy as policy;

pub fn install_all() {
    investment::install_transaction_hooks();
    merchants::install_merchant_hooks();
    currencies::install_base_currency_hook();
    item::install_source_hook();
    policy::install_source_hook();
    accounts::install_funding_account_hook();
}
