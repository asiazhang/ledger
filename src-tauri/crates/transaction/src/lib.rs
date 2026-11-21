#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

pub mod amount;
pub mod command;
pub mod read;
pub mod seams;
pub mod shared;
pub mod write;

mod model;

pub use model::{
    ConvertFields, CreateTransactionResult, NormalizedTransaction, OrderAccountContribution,
    SecurityOrigin, Transaction, TransactionBatchInput, TransactionFunding,
    TransactionFundingInput, TransactionInput, TransactionListFilter, TransactionListResult,
    TransactionOrderSummary, TransactionPurchase, TransactionPurchaseInput,
    TransactionSearchResult, TransactionSource, TransactionSourceKind, TransactionSourceStatus,
    UpdateTransactionInput,
};

pub use amount::{
    Measure, TransactionKind, TransferSide, account_flow_expr, contributing_kinds,
    contributing_kinds_sql, convert_to_native_current, convert_to_native_on_trade_date,
    default_currency_code, expense_gross_expr, expense_net_expr, funding_item_flow_expr,
    income_net_expr, policy_inflow_expr, policy_premium_expr, refund_gross_expr, signed_amount,
};
pub use command::{ConvertCommandFields, InvestmentCommandFields, TransactionCommand};
pub use read::search::{search_transactions, search_transactions_internal};
pub use read::{
    get_transaction, get_transaction_internal, get_transaction_order_summary, list_transactions,
    list_transactions_internal,
};
pub use shared::search_text::{
    is_subsequence, pinyin_initials, split_terms, term_matches, term_matches_text,
};
pub use write::batch::{
    BatchOutcome, DedupIdentity, TransactionBatch, compute_dedup_hash, dedup_identity,
};
pub use write::funding::validate_funding_account;
pub use write::protocol::replay_command;
pub use write::protocol::{
    TransactionWrite, create, create_transaction, create_transaction_internal, delete,
    delete_transaction, delete_transaction_internal, update, update_transaction,
    update_transaction_internal,
};

#[cfg(test)]
mod tests;
